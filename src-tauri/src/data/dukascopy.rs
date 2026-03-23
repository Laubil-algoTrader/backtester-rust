use std::io::Write as IoWrite;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use byteorder::{BigEndian, ReadBytesExt};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use polars::prelude::*;
use tracing::{debug, info, warn};

use crate::errors::AppError;
use crate::models::config::TickStorageFormat;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Options controlling download behavior.
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    /// If true, skip hours that return an empty file (no ticks).
    pub ignore_flats: bool,
    /// If true, retry a failed HTTP request once before giving up.
    pub retry_on_empty: bool,
    /// If true, cache the decompressed bi5 bytes on disk.
    pub use_cache: bool,
    /// Directory for the bi5 cache. Required when `use_cache` is true.
    pub cache_dir: Option<PathBuf>,
    /// Maximum number of concurrent HTTP requests (unused — sequential for now).
    pub max_concurrent: usize,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            ignore_flats: true,
            retry_on_empty: true,
            use_cache: true,
            cache_dir: None,
            max_concurrent: 8,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal tick record
// ─────────────────────────────────────────────────────────────────────────────

/// A single parsed tick from a Dukascopy bi5 record.
struct Bi5Tick {
    timestamp_us: i64,
    ask: f64,
    bid: f64,
    ask_vol: f32,
    bid_vol: f32,
}

// ─────────────────────────────────────────────────────────────────────────────
// HTTP client
// ─────────────────────────────────────────────────────────────────────────────

fn build_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError::DownloadError(format!("Failed to build HTTP client: {}", e)))
}

fn duka_url(symbol: &str, year: i32, month: u32, day: u32, hour: u32) -> String {
    // Dukascopy months are 0-indexed
    format!(
        "https://datafeed.dukascopy.com/datafeed/{}/{}/{:02}/{:02}/{:02}h_ticks.bi5",
        symbol,
        year,
        month - 1,
        day,
        hour
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Cache helpers
// ─────────────────────────────────────────────────────────────────────────────

fn cache_path(
    cache_dir: &Path,
    symbol: &str,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
) -> PathBuf {
    cache_dir
        .join(symbol)
        .join(year.to_string())
        .join(format!("{:02}", month))
        .join(format!("{:02}", day))
        .join(format!("{:02}h.raw", hour))
}

fn load_from_cache(cache_dir: &Path, symbol: &str, year: i32, month: u32, day: u32, hour: u32) -> Option<Vec<u8>> {
    let p = cache_path(cache_dir, symbol, year, month, day, hour);
    std::fs::read(&p).ok()
}

fn save_to_cache(
    cache_dir: &Path,
    symbol: &str,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    data: &[u8],
) {
    let p = cache_path(cache_dir, symbol, year, month, day, hour);
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&p, data);
}

// ─────────────────────────────────────────────────────────────────────────────
// Bi5 fetching and parsing
// ─────────────────────────────────────────────────────────────────────────────

/// Download and decompress a single bi5 file. Returns decompressed bytes or None if empty.
async fn fetch_bi5(
    client: &reqwest::Client,
    symbol: &str,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    opts: &DownloadOptions,
) -> Result<Option<Vec<u8>>, AppError> {
    // Check cache first
    if opts.use_cache {
        if let Some(cache_dir) = &opts.cache_dir {
            if let Some(cached) = load_from_cache(cache_dir, symbol, year, month, day, hour) {
                return Ok(if cached.is_empty() { None } else { Some(cached) });
            }
        }
    }

    let url = duka_url(symbol, year, month, day, hour);
    debug!("Fetching: {}", url);

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| AppError::DownloadError(format!("HTTP request failed for {}: {}", url, e)))?;

    if response.status() == reqwest::StatusCode::NOT_FOUND
        || response.status() == reqwest::StatusCode::NO_CONTENT
    {
        // Store empty marker in cache to avoid re-requesting
        if opts.use_cache {
            if let Some(cache_dir) = &opts.cache_dir {
                save_to_cache(cache_dir, symbol, year, month, day, hour, &[]);
            }
        }
        return Ok(None);
    }

    if !response.status().is_success() {
        return Err(AppError::DownloadError(format!(
            "HTTP {} for {}",
            response.status(),
            url
        )));
    }

    let compressed = response
        .bytes()
        .await
        .map_err(|e| AppError::DownloadError(format!("Failed to read response body: {}", e)))?;

    if compressed.is_empty() {
        if opts.use_cache {
            if let Some(cache_dir) = &opts.cache_dir {
                save_to_cache(cache_dir, symbol, year, month, day, hour, &[]);
            }
        }
        return Ok(None);
    }

    // Decompress LZMA
    let mut decompressed: Vec<u8> = Vec::new();
    lzma_rs::lzma_decompress(&mut std::io::BufReader::new(compressed.as_ref()), &mut decompressed)
        .map_err(|e| AppError::DownloadError(format!("LZMA decompress failed for {}: {}", url, e)))?;

    if decompressed.is_empty() {
        return Ok(None);
    }

    // Save to cache
    if opts.use_cache {
        if let Some(cache_dir) = &opts.cache_dir {
            save_to_cache(cache_dir, symbol, year, month, day, hour, &decompressed);
        }
    }

    Ok(Some(decompressed))
}

/// Parse decompressed bi5 bytes into tick records.
///
/// Each record is 20 bytes big-endian:
/// - offset_ms: u32 (milliseconds from start of hour)
/// - ask: u32  (price × point_value)
/// - bid: u32
/// - ask_vol: f32
/// - bid_vol: f32
fn parse_bi5(
    data: &[u8],
    hour_start_us: i64,
    point_value: f64,
    ignore_flats: bool,
) -> Vec<Bi5Tick> {
    let record_size = 20usize;
    let n = data.len() / record_size;
    let mut ticks = Vec::with_capacity(n);

    let mut cursor = std::io::Cursor::new(data);

    for _ in 0..n {
        let offset_ms = match cursor.read_u32::<BigEndian>() {
            Ok(v) => v,
            Err(_) => break,
        };
        let ask_raw = match cursor.read_u32::<BigEndian>() {
            Ok(v) => v,
            Err(_) => break,
        };
        let bid_raw = match cursor.read_u32::<BigEndian>() {
            Ok(v) => v,
            Err(_) => break,
        };
        let ask_vol = match cursor.read_f32::<BigEndian>() {
            Ok(v) => v,
            Err(_) => break,
        };
        let bid_vol = match cursor.read_f32::<BigEndian>() {
            Ok(v) => v,
            Err(_) => break,
        };

        if ignore_flats && ask_raw == 0 && bid_raw == 0 {
            continue;
        }

        let ask = ask_raw as f64 / point_value;
        let bid = bid_raw as f64 / point_value;

        let timestamp_us = hour_start_us + (offset_ms as i64 * 1000);

        ticks.push(Bi5Tick {
            timestamp_us,
            ask,
            bid,
            ask_vol,
            bid_vol,
        });
    }

    ticks
}

// ─────────────────────────────────────────────────────────────────────────────
// Hour enumeration
// ─────────────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
fn hour_start_us(year: i32, month: u32, day: u32, hour: u32) -> Option<i64> {
    NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|d| d.and_hms_opt(hour, 0, 0))
        .map(|ndt| ndt.and_utc().timestamp_micros())
}

struct HourIter {
    current: NaiveDateTime,
    end: NaiveDateTime,
}

impl HourIter {
    fn new(start: NaiveDate, end: NaiveDate) -> Self {
        let current = start.and_hms_opt(0, 0, 0).unwrap();
        let end_dt = end.and_hms_opt(0, 0, 0).unwrap(); // exclusive
        Self { current, end: end_dt }
    }
}

impl Iterator for HourIter {
    type Item = (i32, u32, u32, u32, i64); // (year, month, day, hour, hour_start_us)

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.end {
            return None;
        }
        let y = self.current.year();
        let mo = self.current.month();
        let d = self.current.day();
        let h = self.current.hour();
        let us = self.current.and_utc().timestamp_micros();
        self.current += chrono::Duration::hours(1);
        Some((y, mo, d, h, us))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Progress helpers
// ─────────────────────────────────────────────────────────────────────────────

fn total_hours(start: NaiveDate, end: NaiveDate) -> u64 {
    let days = (end - start).num_days().max(0) as u64;
    days * 24
}

// ─────────────────────────────────────────────────────────────────────────────
// YearBuffer — accumulates ticks in memory per calendar year
// ─────────────────────────────────────────────────────────────────────────────

struct YearBuffer {
    timestamps: Vec<i64>,
    bids: Vec<f64>,
    asks: Vec<f64>,
    bid_vols: Vec<f32>,
    ask_vols: Vec<f32>,
}

impl YearBuffer {
    fn new() -> Self {
        Self {
            timestamps: Vec::new(),
            bids: Vec::new(),
            asks: Vec::new(),
            bid_vols: Vec::new(),
            ask_vols: Vec::new(),
        }
    }

    fn push(&mut self, t: &Bi5Tick) {
        self.timestamps.push(t.timestamp_us);
        self.bids.push(t.bid);
        self.asks.push(t.ask);
        self.bid_vols.push(t.bid_vol);
        self.ask_vols.push(t.ask_vol);
    }

    fn is_empty(&self) -> bool {
        self.timestamps.is_empty()
    }

    fn len(&self) -> usize {
        self.timestamps.len()
    }
}

/// Flush a year's worth of data to tick_dir and tick_raw_dir.
/// Returns (min_ts_us, max_ts_us).
fn flush_year(
    year: i32,
    buf: &YearBuffer,
    tick_dir: &Path,
    tick_raw_dir: &Path,
    storage_format: TickStorageFormat,
    tz_offset_us: i64,
) -> Result<(i64, i64), AppError> {
    if buf.is_empty() {
        return Ok((i64::MAX, i64::MIN));
    }

    // Apply timezone offset
    let timestamps: Vec<i64> = buf
        .timestamps
        .iter()
        .map(|&ts| ts - tz_offset_us)
        .collect();

    let min_ts = *timestamps.iter().min().unwrap();
    let max_ts = *timestamps.iter().max().unwrap();

    // OHLCV tick file: mid price as open=high=low=close
    let mids: Vec<f64> = buf
        .bids
        .iter()
        .zip(buf.asks.iter())
        .map(|(b, a)| (b + a) / 2.0)
        .collect();

    let volumes: Vec<f64> = buf
        .bid_vols
        .iter()
        .zip(buf.ask_vols.iter())
        .map(|(b, a)| (*b + *a) as f64)
        .collect();

    let tick_path = tick_dir.join(format!("{}.parquet", year));
    let mut tick_df = build_tick_ohlcv_df(&timestamps, &mids, &volumes)?;
    write_parquet_to_path(&mut tick_df, &tick_path)?;

    // Raw tick file
    match storage_format {
        TickStorageFormat::Parquet => {
            let raw_path = tick_raw_dir.join(format!("{}.parquet", year));
            let mut raw_df = build_raw_tick_df(&timestamps, &buf.bids, &buf.asks)?;
            write_parquet_to_path(&mut raw_df, &raw_path)?;
        }
        TickStorageFormat::Binary => {
            let bin_path = tick_raw_dir.join(format!("{}.bin", year));
            write_binary_ticks(&bin_path, &timestamps, &buf.bids, &buf.asks)?;
        }
    }

    info!(
        "Flushed year {} to disk ({} ticks)",
        year,
        buf.len()
    );

    Ok((min_ts, max_ts))
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Download Dukascopy tick data directly to partitioned Parquet/Binary files.
///
/// Streams hour-by-hour, accumulates per-year, flushes each year to disk.
/// Returns `(total_rows, start_date, end_date)`.
pub async fn download_symbol_direct(
    duka_symbol: &str,
    point_value: f64,
    start: NaiveDate,
    end: NaiveDate,
    tick_dir: &Path,
    tick_raw_dir: &Path,
    storage_format: TickStorageFormat,
    tz_offset_hours: f64,
    options: Arc<DownloadOptions>,
    cancel_flag: &AtomicBool,
    progress: impl Fn(u8, &str),
) -> Result<(usize, String, String), AppError> {
    std::fs::create_dir_all(tick_dir)
        .map_err(|e| AppError::FileWrite(format!("create tick_dir: {}", e)))?;
    std::fs::create_dir_all(tick_raw_dir)
        .map_err(|e| AppError::FileWrite(format!("create tick_raw_dir: {}", e)))?;

    let client = build_client()?;
    let tz_offset_us = (tz_offset_hours * 3_600_000_000.0) as i64;

    let total = total_hours(start, end).max(1);
    let mut done: u64 = 0;

    // Year buffers
    let mut year_bufs: std::collections::HashMap<i32, YearBuffer> =
        std::collections::HashMap::new();

    let mut total_rows: usize = 0;
    let mut global_min_ts = i64::MAX;
    let mut global_max_ts = i64::MIN;

    for (year, month, day, hour, hour_us) in HourIter::new(start, end) {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::DownloadCancelled);
        }

        done += 1;
        let pct = ((done * 100) / total).min(99) as u8;
        if done % 24 == 0 {
            progress(
                pct,
                &format!(
                    "Downloading {}-{:02}-{:02}...",
                    year, month, day
                ),
            );
        }

        let data = match fetch_bi5_with_retry(&client, duka_symbol, year, month, day, hour, &options).await {
            Ok(Some(d)) => d,
            Ok(None) => continue,
            Err(e) => {
                warn!("Error fetching {}-{:02}-{:02} {:02}h: {}", year, month, day, hour, e);
                continue;
            }
        };

        let ticks = parse_bi5(&data, hour_us, point_value, options.ignore_flats);
        if ticks.is_empty() {
            continue;
        }

        let buf = year_bufs.entry(year).or_insert_with(YearBuffer::new);
        for t in &ticks {
            buf.push(t);
        }
        total_rows += ticks.len();

        // Flush previous years when we cross a year boundary
        if month == 1 && day == 1 && hour == 0 {
            let old_years: Vec<i32> = year_bufs.keys().copied().filter(|&y| y < year).collect();
            for y in old_years {
                if let Some(buf) = year_bufs.remove(&y) {
                    let (min_ts, max_ts) = flush_year(y, &buf, tick_dir, tick_raw_dir, storage_format, tz_offset_us)?;
                    if min_ts < global_min_ts { global_min_ts = min_ts; }
                    if max_ts > global_max_ts { global_max_ts = max_ts; }
                }
            }
        }
    }

    // Flush remaining years
    let mut years: Vec<i32> = year_bufs.keys().copied().collect();
    years.sort();
    for y in years {
        if let Some(buf) = year_bufs.remove(&y) {
            let (min_ts, max_ts) = flush_year(y, &buf, tick_dir, tick_raw_dir, storage_format, tz_offset_us)?;
            if min_ts < global_min_ts { global_min_ts = min_ts; }
            if max_ts > global_max_ts { global_max_ts = max_ts; }
        }
    }

    if total_rows == 0 {
        return Err(AppError::DownloadError(
            "No tick data downloaded for the specified range".to_string(),
        ));
    }

    let start_date = format_us(global_min_ts);
    let end_date = format_us(global_max_ts);

    progress(100, "Download complete");
    info!(
        "download_symbol_direct: {} ticks, {} → {}",
        total_rows, start_date, end_date
    );

    Ok((total_rows, start_date, end_date))
}

/// Download Dukascopy tick data and write to a CSV file.
pub async fn download_symbol(
    duka_symbol: &str,
    point_value: f64,
    start: NaiveDate,
    end: NaiveDate,
    output_csv: &Path,
    options: Arc<DownloadOptions>,
    cancel_flag: &AtomicBool,
    progress: impl Fn(u8, &str),
) -> Result<(), AppError> {
    let client = build_client()?;

    let total = total_hours(start, end).max(1);
    let mut done: u64 = 0;

    if let Some(parent) = output_csv.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::FileWrite(format!("create csv dir: {}", e)))?;
    }

    let file = std::fs::File::create(output_csv)
        .map_err(|e| AppError::FileWrite(format!("create csv file: {}", e)))?;
    let mut writer = std::io::BufWriter::new(file);

    // Write header
    writeln!(writer, "DateTime,Bid,Ask,Volume")
        .map_err(|e| AppError::FileWrite(e.to_string()))?;

    let mut total_rows: usize = 0;

    for (year, month, day, hour, hour_us) in HourIter::new(start, end) {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::DownloadCancelled);
        }

        done += 1;
        let pct = ((done * 100) / total).min(99) as u8;
        if done % 24 == 0 {
            progress(
                pct,
                &format!("Downloading {}-{:02}-{:02}...", year, month, day),
            );
        }

        let data = match fetch_bi5_with_retry(&client, duka_symbol, year, month, day, hour, &options).await {
            Ok(Some(d)) => d,
            Ok(None) => continue,
            Err(e) => {
                warn!("Error fetching {}-{:02}-{:02} {:02}h: {}", year, month, day, hour, e);
                continue;
            }
        };

        let ticks = parse_bi5(&data, hour_us, point_value, options.ignore_flats);

        for t in &ticks {
            let dt = format_us_datetime(t.timestamp_us);
            writeln!(
                writer,
                "{},{},{},{}",
                dt,
                t.bid,
                t.ask,
                t.bid_vol + t.ask_vol
            )
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
        }

        total_rows += ticks.len();
    }

    writer.flush().map_err(|e| AppError::FileWrite(e.to_string()))?;

    progress(100, "Download complete");
    info!("download_symbol: {} ticks written to {}", total_rows, output_csv.display());

    Ok(())
}

/// Download Dukascopy tick data and aggregate to M1 OHLCV bars in memory.
///
/// Returns a `DataFrame` with columns: `datetime`, `open`, `high`, `low`, `close`, `volume`.
pub async fn download_symbol_m1_candles(
    duka_symbol: &str,
    point_value: f64,
    start: NaiveDate,
    end: NaiveDate,
    tz_offset_hours: f64,
    options: Arc<DownloadOptions>,
    cancel_flag: &AtomicBool,
    progress: impl Fn(u8, &str),
) -> Result<DataFrame, AppError> {
    let client = build_client()?;
    let tz_offset_us = (tz_offset_hours * 3_600_000_000.0) as i64;

    let total = total_hours(start, end).max(1);
    let mut done: u64 = 0;

    let mut all_timestamps: Vec<i64> = Vec::new();
    let mut all_bids: Vec<f64> = Vec::new();
    let mut all_asks: Vec<f64> = Vec::new();
    let mut all_volumes: Vec<f64> = Vec::new();

    for (year, month, day, hour, hour_us) in HourIter::new(start, end) {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::DownloadCancelled);
        }

        done += 1;
        let pct = ((done * 100) / total).min(99) as u8;
        if done % 24 == 0 {
            progress(
                pct,
                &format!("Downloading {}-{:02}-{:02}...", year, month, day),
            );
        }

        let data = match fetch_bi5_with_retry(&client, duka_symbol, year, month, day, hour, &options).await {
            Ok(Some(d)) => d,
            Ok(None) => continue,
            Err(e) => {
                warn!("Error {}-{:02}-{:02} {:02}h: {}", year, month, day, hour, e);
                continue;
            }
        };

        let ticks = parse_bi5(&data, hour_us, point_value, options.ignore_flats);

        for t in ticks {
            all_timestamps.push(t.timestamp_us - tz_offset_us);
            all_bids.push(t.bid);
            all_asks.push(t.ask);
            all_volumes.push((t.bid_vol + t.ask_vol) as f64);
        }
    }

    if all_timestamps.is_empty() {
        return Err(AppError::DownloadError(
            "No tick data downloaded for the specified range".to_string(),
        ));
    }

    progress(100, "Aggregating to M1...");

    // Build tick DataFrame (mid price)
    let mids: Vec<f64> = all_bids
        .iter()
        .zip(all_asks.iter())
        .map(|(b, a)| (b + a) / 2.0)
        .collect();

    let dt_series = Series::new("datetime".into(), &all_timestamps)
        .cast(&DataType::Datetime(TimeUnit::Microseconds, None))
        .map_err(|e| AppError::ParquetConversion(format!("dt cast: {}", e)))?;

    let tick_df = DataFrame::new(vec![
        dt_series.into_column(),
        Series::new("open".into(), &mids).into_column(),
        Series::new("high".into(), &mids).into_column(),
        Series::new("low".into(), &mids).into_column(),
        Series::new("close".into(), &mids).into_column(),
        Series::new("volume".into(), &all_volumes).into_column(),
    ])
    .map_err(|e| AppError::ParquetConversion(format!("tick df: {}", e)))?;

    // Aggregate to M1
    let m1_df = aggregate_ticks_to_m1(tick_df)?;
    info!(
        "download_symbol_m1_candles: {} M1 bars from {} ticks",
        m1_df.height(),
        all_timestamps.len()
    );

    Ok(m1_df)
}

// ─────────────────────────────────────────────────────────────────────────────
// Internal helpers
// ─────────────────────────────────────────────────────────────────────────────

async fn fetch_bi5_with_retry(
    client: &reqwest::Client,
    symbol: &str,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    opts: &DownloadOptions,
) -> Result<Option<Vec<u8>>, AppError> {
    let result = fetch_bi5(client, symbol, year, month, day, hour, opts).await;
    if result.is_err() && opts.retry_on_empty {
        // Wait briefly then retry
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        fetch_bi5(client, symbol, year, month, day, hour, opts).await
    } else {
        result
    }
}

fn aggregate_ticks_to_m1(tick_df: DataFrame) -> Result<DataFrame, AppError> {
    let lf = tick_df
        .lazy()
        .sort(["datetime"], SortMultipleOptions::default());

    lf.group_by_dynamic(
        col("datetime"),
        [],
        DynamicGroupOptions {
            every: Duration::parse("1m"),
            period: Duration::parse("1m"),
            offset: Duration::parse("0ns"),
            label: Label::Left,
            include_boundaries: false,
            closed_window: ClosedWindow::Left,
            start_by: StartBy::WindowBound,
            ..Default::default()
        },
    )
    .agg([
        col("open").first().alias("open"),
        col("high").max().alias("high"),
        col("low").min().alias("low"),
        col("close").last().alias("close"),
        col("volume").sum().alias("volume"),
    ])
    .collect()
    .map_err(|e| AppError::TimeframeConversion(format!("aggregate M1: {}", e)))
}

fn build_tick_ohlcv_df(
    datetimes: &[i64],
    mids: &[f64],
    volumes: &[f64],
) -> Result<DataFrame, AppError> {
    let dt_series = Series::new("datetime".into(), datetimes)
        .cast(&DataType::Datetime(TimeUnit::Microseconds, None))
        .map_err(|e| AppError::ParquetConversion(format!("dt cast: {}", e)))?;

    DataFrame::new(vec![
        dt_series.into_column(),
        Series::new("open".into(), mids).into_column(),
        Series::new("high".into(), mids).into_column(),
        Series::new("low".into(), mids).into_column(),
        Series::new("close".into(), mids).into_column(),
        Series::new("volume".into(), volumes).into_column(),
    ])
    .map_err(|e| AppError::ParquetConversion(format!("tick ohlcv df: {}", e)))
}

fn build_raw_tick_df(
    datetimes: &[i64],
    bids: &[f64],
    asks: &[f64],
) -> Result<DataFrame, AppError> {
    let dt_series = Series::new("datetime".into(), datetimes)
        .cast(&DataType::Datetime(TimeUnit::Microseconds, None))
        .map_err(|e| AppError::ParquetConversion(format!("dt cast: {}", e)))?;

    DataFrame::new(vec![
        dt_series.into_column(),
        Series::new("bid".into(), bids).into_column(),
        Series::new("ask".into(), asks).into_column(),
    ])
    .map_err(|e| AppError::ParquetConversion(format!("raw tick df: {}", e)))
}

fn write_parquet_to_path(df: &mut DataFrame, path: &Path) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::FileWrite(format!("create dir: {}", e)))?;
    }
    let file = std::fs::File::create(path)
        .map_err(|e| AppError::FileWrite(format!("create {}: {}", path.display(), e)))?;
    ParquetWriter::new(file)
        .finish(df)
        .map_err(|e| AppError::ParquetConversion(format!("write parquet {}: {}", path.display(), e)))?;
    Ok(())
}

fn write_binary_ticks(
    path: &Path,
    datetimes: &[i64],
    bids: &[f64],
    asks: &[f64],
) -> Result<(), AppError> {
    use byteorder::{LittleEndian, WriteBytesExt};
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::FileWrite(format!("create dir: {}", e)))?;
    }
    let file = std::fs::File::create(path)
        .map_err(|e| AppError::FileWrite(format!("create binary: {}", e)))?;
    let mut writer = std::io::BufWriter::new(file);
    for ((&dt, &bid), &ask) in datetimes.iter().zip(bids.iter()).zip(asks.iter()) {
        writer.write_i64::<LittleEndian>(dt)
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
        writer.write_f64::<LittleEndian>(bid)
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
        writer.write_f64::<LittleEndian>(ask)
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
    }
    writer.flush().map_err(|e| AppError::FileWrite(e.to_string()))?;
    Ok(())
}

fn format_us(us: i64) -> String {
    let ms = us / 1000;
    let millis = (us.abs() % 1_000_000) / 1000;
    match chrono::DateTime::from_timestamp_millis(ms) {
        Some(dt) => dt
            .naive_utc()
            .format(&format!("%Y-%m-%d %H:%M:%S.{:03}", millis))
            .to_string(),
        None => format!("{}", us),
    }
}

fn format_us_datetime(us: i64) -> String {
    let secs = us / 1_000_000;
    let micros = (us.abs() % 1_000_000) as u32;
    let millis = micros / 1000;
    match chrono::DateTime::from_timestamp(secs, micros * 1000) {
        Some(dt) => dt
            .naive_utc()
            .format(&format!("%Y-%m-%d %H:%M:%S.{:03}", millis))
            .to_string(),
        None => format!("{}", us),
    }
}
