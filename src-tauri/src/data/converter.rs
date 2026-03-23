use std::collections::HashMap;
use std::path::Path;

use polars::prelude::*;
use tracing::info;

use crate::errors::AppError;
use crate::models::config::Timeframe;

use super::loader::{scan_parquet_lazy, write_parquet};

/// Generate all higher timeframes from a base DataFrame.
///
/// Takes a DataFrame containing OHLCV data at `base_tf` resolution,
/// aggregates upward to each higher timeframe, and saves each to
/// `{symbol_dir}/{tf}.parquet`.
///
/// Returns a `HashMap<String, String>` mapping timeframe key (e.g. "m1") → absolute path.
pub fn generate_all_timeframes(
    df: &DataFrame,
    base_tf: Timeframe,
    symbol_dir: &Path,
) -> Result<HashMap<String, String>, AppError> {
    let mut result: HashMap<String, String> = HashMap::new();

    // Save base timeframe
    let base_key = base_tf.as_str().to_string();
    let base_path = symbol_dir.join(format!("{}.parquet", base_key));
    let mut base_df = df.clone();
    write_parquet(&mut base_df, &base_path)?;
    result.insert(base_key, base_path.to_string_lossy().to_string());

    // If base is M1, also generate all higher timeframes
    let higher = base_tf.higher_timeframes();
    for tf in &higher {
        let tf_key = tf.as_str().to_string();
        let tf_path = symbol_dir.join(format!("{}.parquet", tf_key));

        info!("Generating {} from base {}...", tf_key, base_tf.as_str());

        let mut tf_df = aggregate_to_timeframe(df, tf)?;
        write_parquet(&mut tf_df, &tf_path)?;

        let tf_path_str = tf_path.to_string_lossy().to_string();
        result.insert(tf_key.clone(), tf_path_str);
        info!("  → {} rows in {}", tf_df.height(), tf_key);
    }

    Ok(result)
}

/// Generate all timeframes from partitioned yearly Parquet tick files.
///
/// Reads all yearly Parquet files from `tick_dir`, combines them, aggregates to M1,
/// then generates all higher timeframes. Saves M1 + higher to `symbol_dir`.
///
/// Returns a `HashMap<String, String>` mapping timeframe key → absolute path.
pub fn generate_timeframes_from_partitions(
    tick_dir: &Path,
    symbol_dir: &Path,
) -> Result<HashMap<String, String>, AppError> {
    info!(
        "Generating timeframes from partitions in {}",
        tick_dir.display()
    );

    // Scan all yearly parquet files and combine
    let lf = scan_parquet_lazy(tick_dir)?;
    let df = lf
        .sort(["datetime"], SortMultipleOptions::default())
        .collect()
        .map_err(|e| AppError::ParquetConversion(format!("collect tick partitions: {}", e)))?;

    if df.height() == 0 {
        return Err(AppError::InvalidCsvFormat(
            "No tick data found in partitioned directory".to_string(),
        ));
    }

    info!("Loaded {} tick rows from partitions", df.height());

    // Aggregate to M1 first
    let m1_df = aggregate_to_timeframe(&df, &Timeframe::M1)?;
    info!("Aggregated to {} M1 bars", m1_df.height());

    // Generate all timeframes from M1
    generate_all_timeframes(&m1_df, Timeframe::M1, symbol_dir)
}

/// Aggregate a DataFrame to the target timeframe using Polars group_by_dynamic.
fn aggregate_to_timeframe(df: &DataFrame, tf: &Timeframe) -> Result<DataFrame, AppError> {
    let duration = tf.polars_duration();

    let lf = df.clone().lazy();

    // Ensure datetime is sorted before group_by_dynamic
    let lf = lf.sort(["datetime"], SortMultipleOptions::default());

    let agg_lf = lf
        .group_by_dynamic(
            col("datetime"),
            [],
            DynamicGroupOptions {
                every: Duration::parse(duration),
                period: Duration::parse(duration),
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
        ]);

    agg_lf
        .collect()
        .map_err(|e| AppError::TimeframeConversion(format!("aggregate {} failed: {}", duration, e)))
}
