use std::collections::HashMap;
use std::io::Write as IoWrite;
use std::path::Path;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use polars::prelude::*;
use tracing::{info, warn};

use crate::errors::AppError;
use crate::models::config::{DataFormat, TickStorageFormat};

use super::validator::ValidationResult;

// ─────────────────────────────────────────────────────────────────────────────
// CSV → DataFrame
// ─────────────────────────────────────────────────────────────────────────────

/// Load a CSV file into a Polars DataFrame, applying timezone offset.
///
/// Supports both Bar (OHLCV) and Tick formats. The resulting DataFrame always
/// has a `datetime` column typed as `Datetime(Microseconds, None)`.
pub fn load_csv_to_dataframe(
    path: &Path,
    validation: &ValidationResult,
    tz_offset_hours: f64,
) -> Result<DataFrame, AppError> {
    match validation.format {
        DataFormat::Bar => load_bar_csv(path, validation, tz_offset_hours),
        DataFormat::Tick => load_tick_csv(path, validation, tz_offset_hours),
    }
}

fn load_bar_csv(
    path: &Path,
    validation: &ValidationResult,
    tz_offset_hours: f64,
) -> Result<DataFrame, AppError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| AppError::FileRead(e.to_string()))?;

    let mut lines = content.lines().filter(|l| !l.trim().is_empty());
    let header_line = if validation.has_header {
        lines.next().unwrap_or("").to_string()
    } else {
        String::new()
    };

    let sep = validation.delimiter as char;
    let headers: Vec<String> = if validation.has_header {
        header_line
            .split(sep)
            .map(|s| s.trim().trim_matches('"').to_lowercase())
            .collect()
    } else {
        // Assign default names based on column count
        match validation.column_count {
            // MT4/MT5: Date, Time, O, H, L, C, TickVol, Vol, Spread
            9 => vec!["date", "time", "open", "high", "low", "close", "tick_volume", "volume", "spread"]
                .into_iter().map(String::from).collect(),
            // Date, Time, O, H, L, C, TickVol, Vol
            8 => vec!["date", "time", "open", "high", "low", "close", "tick_volume", "volume"]
                .into_iter().map(String::from).collect(),
            // Date, Time, O, H, L, C, Vol
            7 => vec!["date", "time", "open", "high", "low", "close", "volume"]
                .into_iter().map(String::from).collect(),
            // DateTime, O, H, L, C, Vol
            _ => vec!["datetime", "open", "high", "low", "close", "volume"]
                .into_iter().map(String::from).collect(),
        }
    };

    let has_separate_date_time = headers.contains(&"date".to_string())
        && headers.contains(&"time".to_string());

    // Determine column indices
    let dt_idx = if has_separate_date_time {
        None
    } else {
        headers.iter().position(|h| h == "datetime")
    };
    let date_idx = headers.iter().position(|h| h == "date");
    let time_idx = headers.iter().position(|h| h == "time");
    let open_idx = headers
        .iter()
        .position(|h| h == "open")
        .ok_or_else(|| AppError::InvalidCsvFormat("Missing 'Open' column".to_string()))?;
    let high_idx = headers
        .iter()
        .position(|h| h == "high")
        .ok_or_else(|| AppError::InvalidCsvFormat("Missing 'High' column".to_string()))?;
    let low_idx = headers
        .iter()
        .position(|h| h == "low")
        .ok_or_else(|| AppError::InvalidCsvFormat("Missing 'Low' column".to_string()))?;
    let close_idx = headers
        .iter()
        .position(|h| h == "close")
        .ok_or_else(|| AppError::InvalidCsvFormat("Missing 'Close' column".to_string()))?;
    let vol_idx = headers.iter().position(|h| h == "volume");

    let tz_offset_us = (tz_offset_hours * 3_600_000_000.0) as i64;

    let mut datetimes: Vec<i64> = Vec::new();
    let mut opens: Vec<f64> = Vec::new();
    let mut highs: Vec<f64> = Vec::new();
    let mut lows: Vec<f64> = Vec::new();
    let mut closes: Vec<f64> = Vec::new();
    let mut volumes: Vec<f64> = Vec::new();

    for (row_num, line) in lines.enumerate() {
        let cols: Vec<&str> = line.split(sep).collect();
        if cols.len() < 5 {
            warn!("Skipping malformed row {}: {:?}", row_num + 1, line);
            continue;
        }

        let dt_us = if has_separate_date_time {
            let date_str = date_idx.map(|i| cols.get(i).copied().unwrap_or("")).unwrap_or("");
            let time_str = time_idx.map(|i| cols.get(i).copied().unwrap_or("")).unwrap_or("");
            parse_date_time(date_str.trim(), time_str.trim(), row_num)?
        } else {
            let dt_str = dt_idx
                .and_then(|i| cols.get(i).copied())
                .unwrap_or("")
                .trim();
            parse_datetime(dt_str, row_num)?
        };

        let open = parse_f64(cols.get(open_idx).copied().unwrap_or(""), row_num, "Open")?;
        let high = parse_f64(cols.get(high_idx).copied().unwrap_or(""), row_num, "High")?;
        let low = parse_f64(cols.get(low_idx).copied().unwrap_or(""), row_num, "Low")?;
        let close = parse_f64(cols.get(close_idx).copied().unwrap_or(""), row_num, "Close")?;
        let volume = vol_idx
            .and_then(|i| cols.get(i).copied())
            .and_then(|s| s.trim().parse::<f64>().ok())
            .unwrap_or(0.0);

        datetimes.push(dt_us + tz_offset_us);
        opens.push(open);
        highs.push(high);
        lows.push(low);
        closes.push(close);
        volumes.push(volume);
    }

    build_ohlcv_dataframe(datetimes, opens, highs, lows, closes, volumes)
}

fn load_tick_csv(
    path: &Path,
    validation: &ValidationResult,
    tz_offset_hours: f64,
) -> Result<DataFrame, AppError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| AppError::FileRead(e.to_string()))?;

    let mut lines = content.lines().filter(|l| !l.trim().is_empty());
    let header_line = if validation.has_header {
        lines.next().unwrap_or("").to_string()
    } else {
        String::new()
    };

    let sep = validation.delimiter as char;
    let headers: Vec<String> = if validation.has_header {
        header_line
            .split(sep)
            .map(|s| s.trim().trim_matches('"').to_lowercase())
            .collect()
    } else {
        vec!["datetime", "bid", "ask", "volume"]
            .into_iter()
            .map(String::from)
            .collect()
    };

    let dt_idx = headers.iter().position(|h| h == "datetime").unwrap_or(0);
    let bid_idx = headers
        .iter()
        .position(|h| h == "bid")
        .ok_or_else(|| AppError::InvalidCsvFormat("Missing 'Bid' column".to_string()))?;
    let ask_idx = headers
        .iter()
        .position(|h| h == "ask")
        .ok_or_else(|| AppError::InvalidCsvFormat("Missing 'Ask' column".to_string()))?;
    let vol_idx = headers.iter().position(|h| h == "volume");

    let tz_offset_us = (tz_offset_hours * 3_600_000_000.0) as i64;

    let mut datetimes: Vec<i64> = Vec::new();
    let mut bids: Vec<f64> = Vec::new();
    let mut asks: Vec<f64> = Vec::new();
    let mut volumes: Vec<f64> = Vec::new();

    for (row_num, line) in lines.enumerate() {
        let cols: Vec<&str> = line.split(sep).collect();
        if cols.len() < 3 {
            warn!("Skipping malformed tick row {}", row_num + 1);
            continue;
        }

        let dt_us = parse_datetime(cols.get(dt_idx).copied().unwrap_or("").trim(), row_num)?;
        let bid = parse_f64(cols.get(bid_idx).copied().unwrap_or(""), row_num, "Bid")?;
        let ask = parse_f64(cols.get(ask_idx).copied().unwrap_or(""), row_num, "Ask")?;
        let volume = vol_idx
            .and_then(|i| cols.get(i).copied())
            .and_then(|s| s.trim().parse::<f64>().ok())
            .unwrap_or(0.0);

        datetimes.push(dt_us + tz_offset_us);
        bids.push(bid);
        asks.push(ask);
        volumes.push(volume);
    }

    // For tick data loaded as full DataFrame, produce OHLCV where open=high=low=close=mid
    let mid: Vec<f64> = bids
        .iter()
        .zip(asks.iter())
        .map(|(b, a)| (b + a) / 2.0)
        .collect();

    build_ohlcv_dataframe(
        datetimes,
        mid.clone(),
        mid.clone(),
        mid.clone(),
        mid,
        volumes,
    )
}

fn build_ohlcv_dataframe(
    datetimes: Vec<i64>,
    opens: Vec<f64>,
    highs: Vec<f64>,
    lows: Vec<f64>,
    closes: Vec<f64>,
    volumes: Vec<f64>,
) -> Result<DataFrame, AppError> {
    let dt_series = Series::new("datetime".into(), &datetimes)
        .cast(&DataType::Datetime(TimeUnit::Microseconds, None))
        .map_err(|e| AppError::ParquetConversion(format!("datetime cast: {}", e)))?;

    let df = DataFrame::new(vec![
        dt_series.into_column(),
        Series::new("open".into(), &opens).into_column(),
        Series::new("high".into(), &highs).into_column(),
        Series::new("low".into(), &lows).into_column(),
        Series::new("close".into(), &closes).into_column(),
        Series::new("volume".into(), &volumes).into_column(),
    ])
    .map_err(|e| AppError::ParquetConversion(format!("DataFrame::new: {}", e)))?;

    Ok(df)
}

// ─────────────────────────────────────────────────────────────────────────────
// Date Range
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the start and end datetime strings from a DataFrame's `datetime` column.
///
/// Returns `(start, end)` in `"YYYY-MM-DD HH:MM:SS.mmm"` format.
pub fn get_date_range(df: &DataFrame) -> Result<(String, String), AppError> {
    let col = df
        .column("datetime")
        .map_err(|_| AppError::InvalidCsvFormat("DataFrame has no 'datetime' column".into()))?;

    let ts = col
        .cast(&DataType::Int64)
        .map_err(|e| AppError::ParquetConversion(format!("cast datetime to i64: {}", e)))?;

    let ts_i64 = ts
        .i64()
        .map_err(|e| AppError::ParquetConversion(e.to_string()))?;

    let min_us = ts_i64
        .min()
        .ok_or_else(|| AppError::InvalidCsvFormat("Empty datetime column".into()))?;
    let max_us = ts_i64
        .max()
        .ok_or_else(|| AppError::InvalidCsvFormat("Empty datetime column".into()))?;

    Ok((format_us(min_us), format_us(max_us)))
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

// ─────────────────────────────────────────────────────────────────────────────
// Parquet Scanning
// ─────────────────────────────────────────────────────────────────────────────

/// Scan a Parquet file or directory of Parquet files lazily.
///
/// If `path` is a file: scan that single file.
/// If `path` is a directory: scan all `.parquet` files in it as a union.
pub fn scan_parquet_lazy(path: &Path) -> Result<LazyFrame, AppError> {
    if path.is_file() {
        LazyFrame::scan_parquet(path, ScanArgsParquet::default())
            .map_err(|e| AppError::ParquetConversion(format!("scan_parquet: {}", e)))
    } else if path.is_dir() {
        // Collect all .parquet files
        let mut files: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| AppError::FileRead(e.to_string()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("parquet"))
            .collect();
        files.sort();

        if files.is_empty() {
            return Err(AppError::FileNotFound(format!(
                "No parquet files in directory: {}",
                path.display()
            )));
        }

        // Build a union of lazy frames
        let frames: Vec<LazyFrame> = files
            .iter()
            .map(|f| {
                LazyFrame::scan_parquet(f, ScanArgsParquet::default())
                    .map_err(|e| AppError::ParquetConversion(format!("scan_parquet {}: {}", f.display(), e)))
            })
            .collect::<Result<Vec<_>, _>>()?;

        concat(frames, UnionArgs::default())
            .map_err(|e| AppError::ParquetConversion(format!("concat parquet dir: {}", e)))
    } else {
        Err(AppError::FileNotFound(format!(
            "Parquet path does not exist: {}",
            path.display()
        )))
    }
}

/// Build an optional date filter expression for Polars `LazyFrame::filter`.
///
/// Filters the `datetime` column (Datetime Microseconds UTC) to `[start_date, end_date]`.
/// Returns `None` if both strings are empty.
pub fn build_date_filter(start_date: &str, end_date: &str) -> Option<Expr> {
    let start_empty = start_date.trim().is_empty();
    let end_empty = end_date.trim().is_empty();

    if start_empty && end_empty {
        return None;
    }

    let parse_dt = |s: &str| -> Option<i64> {
        let s = s.trim();
        // Try with milliseconds
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
            .or_else(|_| {
                // Date only: treat as start of day
                chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| {
                    d.and_hms_opt(0, 0, 0).unwrap()
                })
            })
            .ok()
            .map(|ndt| ndt.and_utc().timestamp_micros())
    };

    let dt_col = col("datetime").cast(DataType::Int64);

    let mut expr: Option<Expr> = None;

    if !start_empty {
        if let Some(start_us) = parse_dt(start_date) {
            let e = dt_col.clone().gt_eq(lit(start_us));
            expr = Some(expr.map(|prev| prev.and(e.clone())).unwrap_or(e));
        }
    }

    if !end_empty {
        // Normalize date-only end to end of day
        let end_str = end_date.trim();
        let end_us = if end_str.len() == 10 {
            // "YYYY-MM-DD" → "YYYY-MM-DD 23:59:59.999999"
            let extended = format!("{} 23:59:59.999999", end_str);
            parse_dt(&extended)
        } else {
            parse_dt(end_str)
        };
        if let Some(end_us) = end_us {
            let e = dt_col.clone().lt_eq(lit(end_us));
            expr = Some(expr.map(|prev| prev.and(e.clone())).unwrap_or(e));
        }
    }

    expr
}

// ─────────────────────────────────────────────────────────────────────────────
// Streaming Tick CSV → Parquet / Binary
// ─────────────────────────────────────────────────────────────────────────────

/// Progress is reported every N rows.
const PROGRESS_INTERVAL: usize = 1_000_000;

/// Stream a tick CSV file to partitioned yearly Parquet/Binary files.
///
/// Uses `csv::Reader` + a 64 MB `BufReader` instead of `read_to_string`, keeping
/// memory usage bounded regardless of file size. A fast-path datetime parser
/// handles the common "YYYY-MM-DD HH:MM:SS.mmm" format without calling chrono
/// on every row.
///
/// Writes OHLCV tick data to `tick_dir/YYYY.parquet` and raw bid/ask to
/// `tick_raw_dir/YYYY.parquet` or `tick_raw_dir/YYYY.bin`.
///
/// Returns `(total_rows, start_date, end_date)`.
pub fn stream_tick_csv_to_parquet(
    path: &Path,
    validation: &ValidationResult,
    tick_dir: &Path,
    tick_raw_dir: &Path,
    storage_format: TickStorageFormat,
    tz_offset_hours: f64,
    progress: impl Fn(u8, &str),
) -> Result<(usize, String, String), AppError> {
    std::fs::create_dir_all(tick_dir)
        .map_err(|e| AppError::FileWrite(format!("create tick_dir: {}", e)))?;
    std::fs::create_dir_all(tick_raw_dir)
        .map_err(|e| AppError::FileWrite(format!("create tick_raw_dir: {}", e)))?;

    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(1) as f64;

    // ── Streaming CSV reader with large I/O buffer ────────────────────────────
    let file = std::fs::File::open(path)
        .map_err(|e| AppError::FileRead(format!("open tick csv: {}", e)))?;
    let buf = std::io::BufReader::with_capacity(64 * 1024 * 1024, file); // 64 MB read buffer

    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(validation.delimiter)
        .has_headers(validation.has_header)
        .flexible(true) // tolerate rows with extra/missing columns
        .from_reader(buf);

    // ── Column indices ─────────────────────────────────────────────────────────
    let default_headers;
    let headers: Vec<String> = if validation.has_header {
        rdr.headers()
            .map_err(|e| AppError::InvalidCsvFormat(e.to_string()))?
            .iter()
            .map(|s| s.trim().trim_matches('"').to_lowercase())
            .collect()
    } else {
        default_headers = vec!["datetime", "bid", "ask", "volume"];
        default_headers.iter().map(|s| s.to_string()).collect()
    };

    let dt_idx = headers.iter().position(|h| h == "datetime").unwrap_or(0);
    let bid_idx = headers
        .iter()
        .position(|h| h == "bid")
        .ok_or_else(|| AppError::InvalidCsvFormat("Missing 'Bid' column in tick CSV".into()))?;
    let ask_idx = headers
        .iter()
        .position(|h| h == "ask")
        .ok_or_else(|| AppError::InvalidCsvFormat("Missing 'Ask' column in tick CSV".into()))?;
    let vol_idx = headers.iter().position(|h| h == "volume");

    let tz_offset_us = (tz_offset_hours * 3_600_000_000.0) as i64;

    let mut year_buckets: HashMap<i32, YearBucket> = HashMap::new();
    let mut total_rows: usize = 0;
    let mut bytes_read: u64 = 0;

    progress(5, "Reading tick data...");

    // Reusable record buffer — avoids one heap allocation per row.
    let mut record = csv::StringRecord::new();

    loop {
        match rdr.read_record(&mut record) {
            Ok(false) => break, // EOF
            Ok(true) => {}
            Err(e) => {
                warn!("CSV read error at row {}: {}", total_rows + 1, e);
                continue;
            }
        }

        let dt_str = record.get(dt_idx).unwrap_or("").trim();
        if dt_str.is_empty() {
            continue;
        }

        // Fast-path datetime parser — falls back to chrono only for unusual formats.
        let dt_us = match parse_datetime_fast(dt_str.as_bytes())
            .or_else(|| parse_datetime(dt_str, total_rows).ok())
        {
            Some(v) => v + tz_offset_us,
            None => {
                warn!("Row {}: cannot parse datetime '{}'", total_rows + 1, dt_str);
                continue;
            }
        };

        let bid = match record.get(bid_idx).unwrap_or("").trim().parse::<f64>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ask = match record.get(ask_idx).unwrap_or("").trim().parse::<f64>() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let volume = vol_idx
            .and_then(|i| record.get(i))
            .and_then(|s| s.trim().parse::<f64>().ok())
            .unwrap_or(0.0);

        // Extract year directly from the timestamp string — avoids DateTime construction.
        let year = year_from_str(dt_str.as_bytes());
        year_buckets.entry(year).or_insert_with(YearBucket::new).push(dt_us, bid, ask, volume);

        total_rows += 1;
        bytes_read += record.as_slice().len() as u64 + 1;

        if total_rows % PROGRESS_INTERVAL == 0 {
            let pct = (5.0 + (bytes_read as f64 / file_size * 75.0).min(75.0)) as u8;
            progress(pct, "Processing tick data...");

            // Flush years that are older than the current year (works for sorted data).
            let old_years: Vec<i32> = year_buckets.keys().copied()
                .filter(|&y| y < year)
                .collect();
            for y in old_years {
                if let Some(bucket) = year_buckets.remove(&y) {
                    flush_year_bucket(y, bucket, tick_dir, tick_raw_dir, storage_format)?;
                }
            }
        }
    }

    progress(80, "Writing parquet files...");

    // Flush all remaining years
    let mut all_years: Vec<i32> = year_buckets.keys().copied().collect();
    all_years.sort();

    for year in &all_years {
        if let Some(bucket) = year_buckets.remove(year) {
            flush_year_bucket(*year, bucket, tick_dir, tick_raw_dir, storage_format)?;
        }
    }

    if total_rows == 0 {
        return Err(AppError::InvalidCsvFormat(
            "No valid tick rows found in CSV".to_string(),
        ));
    }

    progress(90, "Computing date range...");

    // Determine overall start/end from the written files
    let (start_date, end_date) = compute_date_range_from_tick_dir(tick_dir)?;

    info!(
        "stream_tick_csv_to_parquet: {} rows, {} → {}",
        total_rows, start_date, end_date
    );

    Ok((total_rows, start_date, end_date))
}

struct YearBucket {
    datetimes: Vec<i64>,
    bids: Vec<f64>,
    asks: Vec<f64>,
    volumes: Vec<f64>,
}

impl YearBucket {
    fn new() -> Self {
        Self {
            datetimes: Vec::new(),
            bids: Vec::new(),
            asks: Vec::new(),
            volumes: Vec::new(),
        }
    }

    fn push(&mut self, dt_us: i64, bid: f64, ask: f64, volume: f64) {
        self.datetimes.push(dt_us);
        self.bids.push(bid);
        self.asks.push(ask);
        self.volumes.push(volume);
    }
}

fn flush_year_bucket(
    year: i32,
    bucket: YearBucket,
    tick_dir: &Path,
    tick_raw_dir: &Path,
    storage_format: TickStorageFormat,
) -> Result<(), AppError> {
    if bucket.datetimes.is_empty() {
        return Ok(());
    }

    // Build OHLCV tick DataFrame (mid price)
    let mids: Vec<f64> = bucket
        .bids
        .iter()
        .zip(bucket.asks.iter())
        .map(|(b, a)| (b + a) / 2.0)
        .collect();

    let mut tick_df = build_tick_ohlcv_df(&bucket.datetimes, &mids, &bucket.volumes)?;

    // Write OHLCV tick parquet
    let tick_path = tick_dir.join(format!("{}.parquet", year));
    write_parquet(&mut tick_df, &tick_path)?;

    // Write raw bid/ask
    match storage_format {
        TickStorageFormat::Parquet => {
            let mut raw_df = build_raw_tick_df(&bucket.datetimes, &bucket.bids, &bucket.asks)?;
            let raw_path = tick_raw_dir.join(format!("{}.parquet", year));
            write_parquet(&mut raw_df, &raw_path)?;
        }
        TickStorageFormat::Binary => {
            let bin_path = tick_raw_dir.join(format!("{}.bin", year));
            write_binary_ticks(&bin_path, &bucket.datetimes, &bucket.bids, &bucket.asks)?;
        }
    }

    Ok(())
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

pub fn write_parquet(df: &mut DataFrame, path: &Path) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::FileWrite(format!("create dir {}: {}", parent.display(), e)))?;
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::FileWrite(format!("create dir: {}", e)))?;
    }
    let file = std::fs::File::create(path)
        .map_err(|e| AppError::FileWrite(format!("create binary file: {}", e)))?;
    let mut writer = std::io::BufWriter::new(file);
    for ((&dt, &bid), &ask) in datetimes.iter().zip(bids.iter()).zip(asks.iter()) {
        writer
            .write_i64::<LittleEndian>(dt)
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
        writer
            .write_f64::<LittleEndian>(bid)
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
        writer
            .write_f64::<LittleEndian>(ask)
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
    }
    writer.flush().map_err(|e| AppError::FileWrite(e.to_string()))?;
    Ok(())
}

fn compute_date_range_from_tick_dir(tick_dir: &Path) -> Result<(String, String), AppError> {
    // Scan all parquet files to get min/max datetime
    let lf = scan_parquet_lazy(tick_dir)?;
    let df = lf
        .select([col("datetime")])
        .collect()
        .map_err(|e| AppError::ParquetConversion(format!("collect date range: {}", e)))?;
    get_date_range(&df)
}

// ─────────────────────────────────────────────────────────────────────────────
// Partitioned Tick Scanning
// ─────────────────────────────────────────────────────────────────────────────

/// Scan partitioned yearly Parquet tick files with column projection and date filter.
///
/// Only reads year files that overlap with the requested date range.
pub fn scan_tick_partitioned(
    path: &str,
    columns: &[&str],
    start_date: &str,
    end_date: &str,
) -> Result<DataFrame, AppError> {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err(AppError::FileNotFound(format!(
            "Tick directory does not exist: {}",
            path
        )));
    }

    // Determine year range from dates
    let start_year = parse_year(start_date).unwrap_or(i32::MIN);
    let end_year = parse_year(end_date).unwrap_or(i32::MAX);

    // Find relevant parquet files
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| AppError::FileRead(e.to_string()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(|e| e.to_str()) == Some("parquet")
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<i32>().ok())
                    .map(|y| y >= start_year && y <= end_year)
                    .unwrap_or(true)
        })
        .collect();
    files.sort();

    if files.is_empty() {
        return Err(AppError::NoDataInRange);
    }

    let frames: Vec<LazyFrame> = files
        .iter()
        .map(|f| {
            LazyFrame::scan_parquet(f, ScanArgsParquet::default())
                .map_err(|e| AppError::ParquetConversion(format!("scan {}: {}", f.display(), e)))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut lf = concat(frames, UnionArgs::default())
        .map_err(|e| AppError::ParquetConversion(format!("concat: {}", e)))?;

    // Column projection
    if !columns.is_empty() {
        let exprs: Vec<Expr> = columns.iter().map(|c| col(*c)).collect();
        lf = lf.select(exprs);
    }

    // Date filter
    if let Some(filter) = build_date_filter(start_date, end_date) {
        lf = lf.filter(filter);
    }

    lf.collect()
        .map_err(|e| AppError::ParquetConversion(format!("collect tick partitioned: {}", e)))
}

fn parse_year(date_str: &str) -> Option<i32> {
    let s = date_str.trim();
    if s.is_empty() {
        return None;
    }
    s.get(0..4)?.parse::<i32>().ok()
}

// ─────────────────────────────────────────────────────────────────────────────
// Timezone Shift
// ─────────────────────────────────────────────────────────────────────────────

/// Shift all timestamps in a directory of binary tick files or Parquet files.
///
/// For `.bin` files: reads 24-byte records, shifts the i64 microsecond timestamp,
/// rewrites the file. For `.parquet` files: delegates to `shift_parquet_dir_or_file`.
pub fn shift_tick_raw_dir(path: &Path, delta_ms: i64) -> Result<(), AppError> {
    if !path.is_dir() {
        return shift_parquet_dir_or_file(path, delta_ms);
    }

    let entries: Vec<_> = std::fs::read_dir(path)
        .map_err(|e| AppError::FileRead(e.to_string()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();

    for entry in &entries {
        let ext = entry
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext == "bin" {
            shift_binary_file(entry, delta_ms)?;
        } else if ext == "parquet" {
            shift_parquet_file(entry, delta_ms)?;
        }
    }

    Ok(())
}

/// Shift all timestamps in a Parquet file or directory of Parquet files.
pub fn shift_parquet_dir_or_file(path: &Path, delta_ms: i64) -> Result<(), AppError> {
    if path.is_file() {
        shift_parquet_file(path, delta_ms)
    } else if path.is_dir() {
        let entries: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| AppError::FileRead(e.to_string()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("parquet"))
            .collect();
        for entry in &entries {
            shift_parquet_file(entry, delta_ms)?;
        }
        Ok(())
    } else {
        // Path doesn't exist — silently OK (no data to shift)
        Ok(())
    }
}

fn shift_parquet_file(path: &Path, delta_ms: i64) -> Result<(), AppError> {
    let delta_us = delta_ms * 1000_i64;

    let lf = LazyFrame::scan_parquet(path, ScanArgsParquet::default())
        .map_err(|e| AppError::ParquetConversion(format!("scan {}: {}", path.display(), e)))?;

    let mut df = lf
        .with_column(
            (col("datetime").cast(DataType::Int64) + lit(delta_us))
                .cast(DataType::Datetime(TimeUnit::Microseconds, None))
                .alias("datetime"),
        )
        .collect()
        .map_err(|e| AppError::ParquetConversion(format!("collect shift: {}", e)))?;

    write_parquet(&mut df, path)
}

fn shift_binary_file(path: &Path, delta_ms: i64) -> Result<(), AppError> {
    let delta_us = delta_ms * 1000_i64;
    let data = std::fs::read(path).map_err(|e| AppError::FileRead(e.to_string()))?;

    if data.len() % 24 != 0 {
        warn!(
            "Binary tick file {} has unexpected size {} (not multiple of 24)",
            path.display(),
            data.len()
        );
    }

    let mut out = Vec::with_capacity(data.len());
    let mut cursor = std::io::Cursor::new(&data);

    while (cursor.position() as usize) + 24 <= data.len() {
        let ts = cursor
            .read_i64::<LittleEndian>()
            .map_err(|e| AppError::FileRead(e.to_string()))?;
        let bid = cursor
            .read_f64::<LittleEndian>()
            .map_err(|e| AppError::FileRead(e.to_string()))?;
        let ask = cursor
            .read_f64::<LittleEndian>()
            .map_err(|e| AppError::FileRead(e.to_string()))?;

        out.write_i64::<LittleEndian>(ts + delta_us)
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
        out.write_f64::<LittleEndian>(bid)
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
        out.write_f64::<LittleEndian>(ask)
            .map_err(|e| AppError::FileWrite(e.to_string()))?;
    }

    std::fs::write(path, &out).map_err(|e| AppError::FileWrite(e.to_string()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the year directly from a datetime string (e.g. "2024-01-15 ...").
/// Reads the first 4 ASCII digit bytes — no heap allocation, no timestamp math.
#[inline]
fn year_from_str(s: &[u8]) -> i32 {
    if s.len() >= 4 {
        let d = |i: usize| (s[i].wrapping_sub(b'0')) as i32;
        let y = d(0) * 1000 + d(1) * 100 + d(2) * 10 + d(3);
        if y >= 1970 && y <= 2100 {
            return y;
        }
    }
    1970
}

/// Fast-path parser for the two most common tick datetime formats:
///   "YYYY-MM-DD HH:MM:SS.ffffff"  (ISO with fractional seconds)
///   "YYYY-MM-DD HH:MM:SS"         (ISO without fractional seconds)
///
/// Returns microseconds since Unix epoch, or `None` for any other format
/// (the caller falls back to the chrono-based `parse_datetime`).
fn parse_datetime_fast(s: &[u8]) -> Option<i64> {
    #[inline]
    fn d2(a: u8, b: u8) -> Option<i64> {
        let hi = a.wrapping_sub(b'0');
        let lo = b.wrapping_sub(b'0');
        if hi > 9 || lo > 9 { return None; }
        Some(hi as i64 * 10 + lo as i64)
    }
    #[inline]
    fn d4(b: &[u8]) -> Option<i64> {
        let a = b[0].wrapping_sub(b'0') as i64;
        let bb = b[1].wrapping_sub(b'0') as i64;
        let c = b[2].wrapping_sub(b'0') as i64;
        let d = b[3].wrapping_sub(b'0') as i64;
        if a > 9 || bb > 9 || c > 9 || d > 9 { return None; }
        Some(a * 1000 + bb * 100 + c * 10 + d)
    }
    #[inline]
    fn parse_frac(s: &[u8], dot_pos: usize) -> i64 {
        if s.len() <= dot_pos || s[dot_pos] != b'.' { return 0; }
        let frac_bytes = &s[dot_pos + 1..s.len().min(dot_pos + 7)]; // max 6 digits
        let mut val = 0i64;
        let mut digits = 0usize;
        for &b in frac_bytes {
            let d = b.wrapping_sub(b'0');
            if d > 9 { break; }
            val = val * 10 + d as i64;
            digits += 1;
        }
        while digits < 6 { val *= 10; digits += 1; }
        val
    }

    // ── "yyyyMMdd HH:mm:ss[.SSS]" — SQX/MT5 compact format, no date separators ──
    // e.g. "20240115 12:30:45.123"
    if s.len() >= 17
        && s[4].is_ascii_digit() && s[5].is_ascii_digit()
        && s[6].is_ascii_digit() && s[7].is_ascii_digit()
        && s[8] == b' ' && s[11] == b':' && s[14] == b':'
    {
        let year  = d4(&s[0..4])?;
        let month = d2(s[4], s[5])?;
        let day   = d2(s[6], s[7])?;
        let hour  = d2(s[9], s[10])?;
        let min   = d2(s[12], s[13])?;
        let sec   = d2(s[15], s[16])?;
        if month < 1 || month > 12 || day < 1 || day > 31
            || hour > 23 || min > 59 || sec > 60 { return None; }
        let frac_us = parse_frac(s, 17);
        let days = days_since_epoch(year, month, day)?;
        return Some((days * 86400 + hour * 3600 + min * 60 + sec) * 1_000_000 + frac_us);
    }

    // ── "YYYY-MM-DD HH:MM:SS[.ffffff]" or "YYYY.MM.DD HH:MM:SS[.fff]" ──
    if s.len() < 19 { return None; }
    let date_sep = s[4];
    if (date_sep != b'-' && date_sep != b'.') || s[7] != date_sep
        || s[10] != b' ' || s[13] != b':' || s[16] != b':'
    {
        return None;
    }

    let year  = d4(&s[0..4])?;
    let month = d2(s[5], s[6])?;
    let day   = d2(s[8], s[9])?;
    let hour  = d2(s[11], s[12])?;
    let min   = d2(s[14], s[15])?;
    let sec   = d2(s[17], s[18])?;

    if month < 1 || month > 12 || day < 1 || day > 31
        || hour > 23 || min > 59 || sec > 60 { return None; }

    let frac_us = parse_frac(s, 19);
    let days = days_since_epoch(year, month, day)?;
    Some((days * 86400 + hour * 3600 + min * 60 + sec) * 1_000_000 + frac_us)
}

/// Compute days since 1970-01-01 for a given (year, month, day).
fn days_since_epoch(y: i64, m: i64, d: i64) -> Option<i64> {
    if y < 1970 { return None; }
    // Shift months so March = 1 (simplifies leap-year handling)
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y / 400;
    let yoe = y - era * 400;                           // year of era [0, 399]
    let doy = (153 * m + 2) / 5 + d - 1;             // day of year [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // day of era
    let days = era * 146097 + doe - 719468;           // days since 1970-01-01
    Some(days)
}


fn parse_datetime(s: &str, row: usize) -> Result<i64, AppError> {
    let s = s.trim().trim_matches('"');
    // Try various formats
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(ndt.and_utc().timestamp_micros());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(ndt.and_utc().timestamp_micros());
    }
    // MT4/MT5 format: YYYY.MM.DD HH:MM:SS.mmm (con y sin fracciones)
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y.%m.%d %H:%M:%S%.f") {
        return Ok(ndt.and_utc().timestamp_micros());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y.%m.%d %H:%M:%S") {
        return Ok(ndt.and_utc().timestamp_micros());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y.%m.%d %H:%M") {
        return Ok(ndt.and_utc().timestamp_micros());
    }
    // SQX compact format: yyyyMMdd HH:mm:ss[.SSS] (no separators in date)
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y%m%d %H:%M:%S%.f") {
        return Ok(ndt.and_utc().timestamp_micros());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y%m%d %H:%M:%S") {
        return Ok(ndt.and_utc().timestamp_micros());
    }
    // Date only
    if let Ok(nd) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        if let Some(ndt) = nd.and_hms_opt(0, 0, 0) {
            return Ok(ndt.and_utc().timestamp_micros());
        }
    }
    Err(AppError::CsvParseError {
        row: row + 1,
        message: format!("Cannot parse datetime: '{}'", s),
    })
}

fn parse_date_time(date_str: &str, time_str: &str, row: usize) -> Result<i64, AppError> {
    let combined = format!("{} {}", date_str.trim(), time_str.trim());
    parse_datetime(&combined, row)
}

fn parse_f64(s: &str, row: usize, field: &str) -> Result<f64, AppError> {
    let trimmed = s.trim().trim_matches('"');
    trimmed.parse::<f64>().map_err(|_| AppError::CsvParseError {
        row: row + 1,
        message: format!("Cannot parse {} as f64: '{}'", field, trimmed),
    })
}

