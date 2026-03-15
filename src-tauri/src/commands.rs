use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tracing::info;

use crate::data::{converter, loader, storage, validator};
use crate::engine::{builder, executor, monte_carlo, optimizer, walk_forward};
use crate::engine::executor::SubBarData;
use crate::errors::AppError;
use crate::models::builder::BuilderConfig;
use crate::models::config::{DataFormat, InstrumentConfig, TickPipeline, TickStorageFormat, Timeframe};
use crate::models::project::Project;
use crate::models::result::{BacktestMetrics, BacktestResults, MonteCarloConfig, MonteCarloResult, OosResult, OptimizationConfig, OptimizationMethod, OptimizationResult, WalkForwardConfig, WalkForwardResult};
use crate::models::strategy::{BacktestConfig, BacktestPrecision, Strategy};
use crate::models::symbol::Symbol;
use crate::models::trade::TradeResult;
use crate::utils::{codegen, export};
use crate::AppState;

// ── Data Commands ──

/// Upload a CSV file, validate, convert to storage format, generate timeframes, store in DB.
#[tauri::command]
pub async fn upload_csv(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    file_path: String,
    symbol_name: String,
    instrument_config: InstrumentConfig,
    tick_storage_format: Option<TickStorageFormat>,
) -> Result<Symbol, AppError> {
    let tick_storage_format = tick_storage_format.unwrap_or_default();
    // 0. Sanitize symbol name (prevent path traversal)
    sanitize_symbol_name(&symbol_name)?;

    let path = PathBuf::from(&file_path);

    // 1. Validate CSV
    emit_progress(&app, 5, "Validating CSV...");
    let validation = validator::validate_csv(&path)?;
    info!(
        "Validated CSV: format={:?}, sample={}",
        validation.format, validation.row_count_sample
    );

    // 2. Determine base timeframe from format
    let base_timeframe = match validation.format {
        DataFormat::Tick => Timeframe::Tick,
        DataFormat::Bar => Timeframe::M1, // default, user can change later
    };

    // 3. Create symbol directory
    let data_dir = state.data_dir.clone();
    let symbol_dir = data_dir.join("symbols").join(&symbol_name);
    std::fs::create_dir_all(&symbol_dir)?;

    // 4. Load and process data
    let (total_rows, start_date, end_date, timeframe_paths) =
        if validation.format == DataFormat::Tick {
            // ── Tick data: streaming CSV → yearly Parquet (low memory) ──
            let tick_dir = symbol_dir.join("tick");
            let tick_raw_dir = symbol_dir.join("tick_raw");

            let (total_rows, start_date, end_date) = loader::stream_tick_csv_to_parquet(
                &path,
                &validation,
                &tick_dir,
                &tick_raw_dir,
                tick_storage_format,
                instrument_config.tz_offset_hours,
                |pct, msg| emit_progress(&app, pct, msg),
            )?;

            emit_progress(&app, 85, "Generating timeframes...");
            let mut timeframe_paths = converter::generate_timeframes_from_partitions(
                &tick_dir,
                &symbol_dir,
            )?;
            timeframe_paths.insert("tick".into(), tick_dir.to_string_lossy().into());
            timeframe_paths.insert("tick_raw".into(), tick_raw_dir.to_string_lossy().into());

            (total_rows, start_date, end_date, timeframe_paths)
        } else {
            // ── Bar data: standard flow (single CSV read) ──
            emit_progress(&app, 15, "Loading CSV data...");
            let df = loader::load_csv_to_dataframe(&path, &validation, instrument_config.tz_offset_hours)?;
            let total_rows = df.height();
            info!("Loaded {} rows from CSV", total_rows);

            let (start_date, end_date) = loader::get_date_range(&df)?;

            emit_progress(&app, 40, "Generating timeframes...");
            let timeframe_paths =
                converter::generate_all_timeframes(&df, base_timeframe, &symbol_dir)?;

            (total_rows, start_date, end_date, timeframe_paths)
        };

    // 6. Create symbol and store in DB
    emit_progress(&app, 90, "Saving to database...");
    let symbol_id = uuid::Uuid::new_v4().to_string();
    let upload_date = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let symbol = Symbol {
        id: symbol_id,
        name: symbol_name,
        base_timeframe,
        upload_date,
        total_rows,
        start_date,
        end_date,
        timeframe_paths,
        instrument_config,
    };

    let db = state.db.lock().await;
    storage::insert_symbol(&db, &symbol)?;

    emit_progress(&app, 100, "Done!");
    info!("Symbol uploaded: {} ({} rows)", symbol.name, symbol.total_rows);

    Ok(symbol)
}

/// Get all imported symbols.
#[tauri::command]
pub async fn get_symbols(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Symbol>, AppError> {
    let db = state.db.lock().await;
    storage::get_all_symbols(&db)
}

/// Delete a symbol and its Parquet files.
#[tauri::command]
pub async fn delete_symbol(
    state: tauri::State<'_, AppState>,
    symbol_id: String,
) -> Result<(), AppError> {
    let db = state.db.lock().await;
    let symbol = storage::delete_symbol_by_id(&db, &symbol_id)?;

    // Clean up Parquet files and tick data directories.
    // Tick data is stored as directories (one per year), so use remove_dir_all when needed.
    for (_tf, path) in &symbol.timeframe_paths {
        let p = std::path::Path::new(path);
        let result = if p.is_dir() {
            std::fs::remove_dir_all(p)
        } else {
            std::fs::remove_file(p)
        };
        if let Err(e) = result {
            tracing::warn!("Failed to remove {}: {}", path, e);
        }
    }

    // Try to remove the symbol directory if empty
    let symbol_dir = state.data_dir.join("symbols").join(&symbol.name);
    std::fs::remove_dir(&symbol_dir).ok();

    info!("Symbol deleted: {}", symbol.name);
    Ok(())
}

/// Transform all stored timestamps of a symbol to a new timezone offset.
///
/// Computes `delta = new_tz_offset_hours - old_tz_offset_hours` and shifts every
/// Parquet / binary file in the symbol's directories by that delta.
/// Updates `tz_offset_hours` in the DB instrument config and adjusts `start_date` / `end_date`.
#[tauri::command]
pub async fn transform_symbol_timezone(
    state: tauri::State<'_, AppState>,
    symbol_id: String,
    new_tz_offset_hours: f64,
) -> Result<Symbol, AppError> {
    // 1. Load symbol info (brief DB lock)
    let symbol = {
        let db = state.db.lock().await;
        storage::get_symbol_by_id(&db, &symbol_id)?
    };

    let old_offset = symbol.instrument_config.tz_offset_hours;
    let delta_hours = new_tz_offset_hours - old_offset;

    if delta_hours.abs() < 1e-9 {
        return Ok(symbol);
    }

    let delta_ms = (delta_hours * 3_600_000.0) as i64;

    // 2. Shift all Parquet / binary files (CPU + IO, no DB lock held)
    for (tf_name, tf_path) in &symbol.timeframe_paths {
        let path = std::path::Path::new(tf_path);
        if tf_name == "tick_raw" {
            loader::shift_tick_raw_dir(path, delta_ms)?;
        } else {
            loader::shift_parquet_dir_or_file(path, delta_ms)?;
        }
    }

    // 3. Shift the stored start/end dates by the same delta
    let new_start = shift_date_string(&symbol.start_date, delta_ms);
    let new_end = shift_date_string(&symbol.end_date, delta_ms);

    // 4. Persist updated config + dates
    let mut new_config = symbol.instrument_config.clone();
    new_config.tz_offset_hours = new_tz_offset_hours;

    let db = state.db.lock().await;
    storage::update_symbol_tz(&db, &symbol_id, &new_config, &new_start, &new_end)?;
    storage::get_symbol_by_id(&db, &symbol_id)
}

/// Shift a stored date string (e.g. "2024-01-15 00:00:00.000") by `delta_ms` milliseconds.
/// Returns the original string unchanged if parsing fails.
fn shift_date_string(date_str: &str, delta_ms: i64) -> String {
    use chrono::NaiveDateTime;
    let trimmed = date_str.trim();
    NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S"))
        .map(|ndt| {
            let ts_ms = ndt.and_utc().timestamp_millis() + delta_ms;
            chrono::DateTime::from_timestamp_millis(ts_ms)
                .map(|d| d.naive_utc().format("%Y-%m-%d %H:%M:%S%.3f").to_string())
                .unwrap_or_else(|| date_str.to_string())
        })
        .unwrap_or_else(|_| date_str.to_string())
}

/// Preview first N rows of a symbol's data for a given timeframe.
#[tauri::command]
pub async fn preview_data(
    state: tauri::State<'_, AppState>,
    symbol_id: String,
    timeframe: String,
    limit: usize,
) -> Result<Vec<Value>, AppError> {
    let db = state.db.lock().await;
    let symbol = storage::get_symbol_by_id(&db, &symbol_id)?;

    let parquet_path = symbol
        .timeframe_paths
        .get(&timeframe)
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Timeframe {} not available for {}",
                timeframe, symbol.name
            ))
        })?;

    // Lazy scan with row limit pushed down — avoids loading the full Parquet into memory.
    let df = loader::scan_parquet_lazy(&PathBuf::from(parquet_path))?
        .limit(limit as u32)
        .collect()
        .map_err(|e| AppError::Internal(format!("preview collect: {}", e)))?;

    // Convert to Vec<serde_json::Value> for the frontend
    dataframe_to_json(&df)
}

/// Placeholder greet command (for testing communication).
#[tauri::command]
pub async fn greet(name: String) -> Result<String, AppError> {
    Ok(format!("Hello, {}! Backtester is ready.", name))
}

// ── Backtest Commands ──

/// Load sub-bar data based on the precision mode and symbol configuration.
/// Uses partitioned yearly Parquet files (skips irrelevant years entirely).
/// Falls back to single-file lazy scan for backward compatibility with old imports.
fn load_sub_bar_data(
    symbol: &Symbol,
    strategy: &Strategy,
    config: &BacktestConfig,
) -> Result<SubBarData, AppError> {
    let t0 = std::time::Instant::now();
    let date_filter = loader::build_date_filter(&config.start_date, &config.end_date);

    let result = match config.precision {
        BacktestPrecision::SelectedTfOnly => Ok(SubBarData::None),

        BacktestPrecision::M1TickSimulation => {
            let m1_path = symbol
                .timeframe_paths
                .get("m1")
                .ok_or_else(|| AppError::NotFound("M1 data not available for tick simulation".into()))?;
            let mut lf = loader::scan_parquet_lazy(&PathBuf::from(m1_path))?;
            if let Some(f) = &date_filter {
                lf = lf.filter(f.clone());
            }
            let filtered_df = lf.collect()
                .map_err(|e| AppError::Internal(format!("M1 lazy collect: {}", e)))?;
            let candles = executor::candles_from_dataframe(&filtered_df)?;
            info!("Loaded {} M1 sub-bars for tick simulation", candles.len());
            Ok(SubBarData::Candles(candles))
        }

        BacktestPrecision::RealTickCustomSpread => {
            let tick_path = symbol
                .timeframe_paths
                .get("tick")
                .ok_or_else(|| AppError::NotFound("Tick data not available for custom spread mode".into()))?;
            // Partitioned loader: scans only relevant year files, column projection + date filter
            let filtered_df = loader::scan_tick_partitioned(
                tick_path,
                &["datetime", "close"],
                &config.start_date,
                &config.end_date,
            )?;
            let half_spread = strategy.trading_costs.spread_pips * symbol.instrument_config.pip_size / 2.0;
            let ticks = executor::tick_columns_from_ohlcv_with_spread(&filtered_df, half_spread)?;
            info!("Loaded {} ticks as TickColumns with custom spread ({:.1} pips)", ticks.len(), strategy.trading_costs.spread_pips);
            Ok(SubBarData::Ticks(ticks))
        }

        BacktestPrecision::RealTickRealSpread => {
            let tick_raw_path = symbol
                .timeframe_paths
                .get("tick_raw")
                .ok_or_else(|| AppError::NotFound("Raw tick data (bid/ask) not available. Re-import tick data to enable real spread mode.".into()))?;

            // Auto-detect storage format by checking file extension of first file found
            let is_binary = std::path::Path::new(tick_raw_path.as_str()).is_dir() && {
                std::fs::read_dir(tick_raw_path.as_str())
                    .ok()
                    .and_then(|mut rd| rd.next())
                    .and_then(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().ends_with(".bin"))
                    .unwrap_or(false)
            };

            let ticks = if is_binary {
                executor::tick_columns_from_binary_dir(
                    tick_raw_path,
                    &config.start_date,
                    &config.end_date,
                )?
            } else {
                // Partitioned Parquet loader: column projection + date filter via Polars
                let filtered_df = loader::scan_tick_partitioned(
                    tick_raw_path,
                    &["datetime", "bid", "ask"],
                    &config.start_date,
                    &config.end_date,
                )?;
                executor::tick_columns_from_dataframe(&filtered_df)?
            };

            info!("Loaded {} raw ticks as TickColumns with real spread ({})",
                ticks.len(), if is_binary { "binary" } else { "parquet" });
            Ok(SubBarData::Ticks(ticks))
        }
    };

    let elapsed = t0.elapsed();
    info!("Sub-bar data loaded in {:.2}s", elapsed.as_secs_f64());
    result
}

/// Run a backtest with the given strategy and configuration.
#[tauri::command]
pub async fn run_backtest(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    strategy: Strategy,
    config: BacktestConfig,
) -> Result<BacktestResults, AppError> {
    info!("Running backtest: strategy={}, symbol={}, precision={:?}",
        strategy.name, config.symbol_id, config.precision);

    // Reset cancel flag
    state.cancel_flag.store(false, Ordering::Relaxed);

    // Load symbol to get instrument config and parquet path
    let db = state.db.lock().await;
    let symbol = storage::get_symbol_by_id(&db, &config.symbol_id)?;
    drop(db); // Release lock before long operation

    let timeframe_key = config.timeframe.as_str().to_string();
    let parquet_path = symbol
        .timeframe_paths
        .get(&timeframe_key)
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Timeframe {} not available for {}",
                timeframe_key, symbol.name
            ))
        })?;

    // Lazy-load parquet with date filter pushdown → only filtered rows materialized
    let date_filter = loader::build_date_filter(&config.start_date, &config.end_date);
    let mut lf = loader::scan_parquet_lazy(&PathBuf::from(parquet_path))?;
    if let Some(f) = &date_filter {
        lf = lf.filter(f.clone());
    }
    let df = lf.collect()
        .map_err(|e| AppError::Internal(format!("candle lazy collect: {}", e)))?;
    let candles = executor::candles_from_dataframe(&df)?;
    if candles.is_empty() {
        return Err(AppError::NoDataInRange);
    }

    info!("Backtest data: {} candles after date filter", candles.len());

    // Load sub-bar data for precision mode
    let sub_bars = load_sub_bar_data(&symbol, &strategy, &config)?;

    // Run the backtest (blocking computation in async context)
    let cancel_flag = state.cancel_flag.clone();
    let instrument = symbol.instrument_config.clone();

    let result = tokio::task::spawn_blocking(move || {
        executor::run_backtest(
            &candles,
            &sub_bars,
            &strategy,
            &config,
            &instrument,
            &cancel_flag,
            |pct, current, total| {
                let _ = app.emit(
                    "backtest-progress",
                    serde_json::json!({
                        "percent": pct,
                        "current_bar": current,
                        "total_bars": total,
                    }),
                );
            },
        )
    })
    .await
    .map_err(|e| AppError::BacktestExecution(format!("Task join error: {}", e)))??;

    info!(
        "Backtest complete: {} trades, net profit: {:.2}",
        result.trades.len(),
        result.metrics.net_profit
    );

    Ok(result)
}

/// Cancel a running backtest.
#[tauri::command]
pub async fn cancel_backtest(
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    info!("Cancelling backtest");
    state.cancel_flag.store(true, Ordering::Relaxed);
    Ok(())
}

// ── Strategy Commands ──

/// Save a strategy (insert or update). Returns the strategy ID.
#[tauri::command]
pub async fn save_strategy(
    state: tauri::State<'_, AppState>,
    mut strategy: Strategy,
) -> Result<String, AppError> {
    let db = state.db.lock().await;
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    let exists = storage::strategy_exists(&db, &strategy.id)?;

    if exists {
        strategy.updated_at = now;
        storage::update_strategy(&db, &strategy)?;
        Ok(strategy.id)
    } else {
        if strategy.id.is_empty() {
            strategy.id = uuid::Uuid::new_v4().to_string();
        }
        strategy.created_at = now.clone();
        strategy.updated_at = now;
        storage::insert_strategy(&db, &strategy)
    }
}

/// Load all saved strategies.
#[tauri::command]
pub async fn load_strategies(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Strategy>, AppError> {
    let db = state.db.lock().await;
    storage::get_all_strategies(&db)
}

/// Delete a strategy by ID.
#[tauri::command]
pub async fn delete_strategy(
    state: tauri::State<'_, AppState>,
    strategy_id: String,
) -> Result<(), AppError> {
    let db = state.db.lock().await;
    storage::delete_strategy_by_id(&db, &strategy_id)
}

// ── Optimization Commands ──

/// Run optimization (Grid Search or Genetic Algorithm).
#[tauri::command]
pub async fn run_optimization(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    strategy: Strategy,
    optimization_config: OptimizationConfig,
) -> Result<Vec<OptimizationResult>, AppError> {
    info!(
        "Running {:?} optimization: {} parameter ranges, precision={:?}",
        optimization_config.method,
        optimization_config.parameter_ranges.len(),
        optimization_config.backtest_config.precision
    );

    // Reset the optimization-specific cancel flag (separate from backtest cancel).
    state.optimization_cancel_flag.store(false, Ordering::Relaxed);

    // Load symbol to get instrument config and parquet path
    let bt_config = &optimization_config.backtest_config;
    let db = state.db.lock().await;
    let symbol = storage::get_symbol_by_id(&db, &bt_config.symbol_id)?;
    drop(db);

    let timeframe_key = bt_config.timeframe.as_str().to_string();
    let parquet_path = symbol
        .timeframe_paths
        .get(&timeframe_key)
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Timeframe {} not available for {}",
                timeframe_key, symbol.name
            ))
        })?;

    // Lazy-load parquet with date filter pushdown → only filtered rows materialized
    let date_filter = loader::build_date_filter(&bt_config.start_date, &bt_config.end_date);
    let mut lf = loader::scan_parquet_lazy(&PathBuf::from(parquet_path))?;
    if let Some(f) = &date_filter {
        lf = lf.filter(f.clone());
    }
    let df = lf.collect()
        .map_err(|e| AppError::Internal(format!("candle lazy collect: {}", e)))?;
    let candles = executor::candles_from_dataframe(&df)?;
    if candles.is_empty() {
        return Err(AppError::NoDataInRange);
    }

    info!("Optimization data: {} candles after date filter", candles.len());

    // Load sub-bar data once (shared across all optimization runs)
    let sub_bars = load_sub_bar_data(&symbol, &strategy, bt_config)?;

    // Pre-load OOS data for each OOS period
    let oos_periods = optimization_config.oos_periods.clone();
    let mut oos_data: Vec<(String, Vec<crate::models::candle::Candle>, SubBarData)> = Vec::new();
    for period in &oos_periods {
        let oos_date_filter = loader::build_date_filter(&period.start_date, &period.end_date);
        let mut oos_lf = loader::scan_parquet_lazy(&PathBuf::from(parquet_path))?;
        if let Some(f) = &oos_date_filter {
            oos_lf = oos_lf.filter(f.clone());
        }
        let oos_df = oos_lf.collect()
            .map_err(|e| AppError::Internal(format!("OOS candle lazy collect: {}", e)))?;
        let oos_candles = executor::candles_from_dataframe(&oos_df)?;

        // Load sub-bar data for OOS period with adjusted date range
        let mut oos_bt_config = bt_config.clone();
        oos_bt_config.start_date = period.start_date.clone();
        oos_bt_config.end_date = period.end_date.clone();
        let oos_sub = load_sub_bar_data(&symbol, &strategy, &oos_bt_config)?;

        info!("OOS '{}': {} candles loaded", period.label, oos_candles.len());
        oos_data.push((period.label.clone(), oos_candles, oos_sub));
    }

    let cancel_flag = state.optimization_cancel_flag.clone();
    let instrument = symbol.instrument_config.clone();

    let result = tokio::task::spawn_blocking(move || {
        let bt_config = &optimization_config.backtest_config;
        let ranges = &optimization_config.parameter_ranges;
        let objectives = &optimization_config.objectives;

        let opt_start = std::time::Instant::now();
        let progress_cb = |pct: u8, current: usize, total: usize, best: f64| {
            let eta = if pct > 2 && pct < 100 {
                let elapsed = opt_start.elapsed().as_secs_f64();
                let remaining = (elapsed / pct as f64) * (100.0 - pct as f64);
                remaining as u64
            } else {
                0
            };
            let _ = app.emit(
                "optimization-progress",
                serde_json::json!({
                    "percent": pct,
                    "current": current,
                    "total": total,
                    "best_so_far": best,
                    "eta_seconds": eta,
                }),
            );
        };

        let mut results = match optimization_config.method {
            OptimizationMethod::GridSearch => optimizer::run_grid_search(
                &candles,
                &sub_bars,
                &strategy,
                bt_config,
                &instrument,
                ranges,
                objectives,
                &cancel_flag,
                progress_cb,
            ),
            OptimizationMethod::GeneticAlgorithm => {
                let ga_config = optimization_config.ga_config.as_ref().ok_or_else(|| {
                    AppError::OptimizationError(
                        "Genetic Algorithm config required".into(),
                    )
                })?;
                optimizer::run_genetic_algorithm(
                    &candles,
                    &sub_bars,
                    &strategy,
                    bt_config,
                    &instrument,
                    ranges,
                    objectives,
                    ga_config,
                    &cancel_flag,
                    progress_cb,
                )
            }
        }?;

        // Run OOS evaluation for each top result
        if !oos_data.is_empty() && !results.is_empty() {
            info!("Running OOS evaluation: {} results × {} periods", results.len(), oos_data.len());
            let no_cancel = std::sync::atomic::AtomicBool::new(false);

            for opt_result in results.iter_mut() {
                // Respect cancellation between OOS result evaluations
                if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    info!("OOS evaluation cancelled by user");
                    break;
                }

                // Reconstruct the strategy with this result's params.
                // If a parameter name is missing, skip this result rather than
                // silently applying 0.0 which would corrupt the OOS backtest.
                let param_values_result: Result<Vec<f64>, _> = ranges.iter()
                    .map(|r| opt_result.params.get(&r.display_name)
                        .copied()
                        .ok_or_else(|| format!("OOS: parameter '{}' missing from result", r.display_name)))
                    .collect();
                let param_values = match param_values_result {
                    Ok(vals) => vals,
                    Err(msg) => {
                        tracing::warn!("{}", msg);
                        continue;
                    }
                };
                let modified_strategy = optimizer::apply_params(&strategy, ranges, &param_values);

                let mut oos_results = Vec::new();
                for (label, oos_candles, oos_sub) in &oos_data {
                    if oos_candles.is_empty() {
                        oos_results.push(OosResult {
                            label: label.clone(),
                            total_return_pct: 0.0,
                            sharpe_ratio: 0.0,
                            max_drawdown_pct: 0.0,
                            profit_factor: 0.0,
                            total_trades: 0,
                        });
                        continue;
                    }

                    match executor::run_backtest(
                        oos_candles,
                        oos_sub,
                        &modified_strategy,
                        bt_config,
                        &instrument,
                        &no_cancel,
                        |_, _, _| {},
                    ) {
                        Ok(bt) => {
                            oos_results.push(OosResult {
                                label: label.clone(),
                                total_return_pct: bt.metrics.total_return_pct,
                                sharpe_ratio: bt.metrics.sharpe_ratio,
                                max_drawdown_pct: bt.metrics.max_drawdown_pct,
                                profit_factor: bt.metrics.profit_factor,
                                total_trades: bt.metrics.total_trades,
                            });
                        }
                        Err(e) => {
                            tracing::warn!("OOS backtest failed for '{}': {}", label, e);
                            oos_results.push(OosResult {
                                label: label.clone(),
                                total_return_pct: 0.0,
                                sharpe_ratio: 0.0,
                                max_drawdown_pct: 0.0,
                                profit_factor: 0.0,
                                total_trades: 0,
                            });
                        }
                    }
                }
                opt_result.oos_results = oos_results;
            }
        }

        Ok::<Vec<OptimizationResult>, AppError>(results)
    })
    .await
    .map_err(|e| AppError::OptimizationError(format!("Task join error: {}", e)))??;

    info!("Optimization complete: {} results", result.len());
    Ok(result)
}

/// Cancel a running optimization.
#[tauri::command]
pub async fn cancel_optimization(
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    info!("Cancelling optimization");
    state.optimization_cancel_flag.store(true, Ordering::Relaxed);
    Ok(())
}

// ── Walk-Forward Analysis ──

/// Run a Walk-Forward Analysis.
///
/// Divides the data into `config.num_windows` sequential windows. Each window is split
/// into an in-sample portion (optimized) and an out-of-sample portion (validated).
/// Emits `walk-forward-progress` events during execution.
#[tauri::command]
pub async fn run_walk_forward(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    strategy: Strategy,
    wf_config: WalkForwardConfig,
) -> Result<WalkForwardResult, AppError> {
    info!(
        "Running Walk-Forward Analysis: {} windows, IS={:.0}%, strategy={}",
        wf_config.num_windows,
        wf_config.in_sample_pct * 100.0,
        strategy.name
    );

    state.optimization_cancel_flag.store(false, Ordering::Relaxed);

    let bt_config = &wf_config.optimization_config.backtest_config;
    let db = state.db.lock().await;
    let symbol = storage::get_symbol_by_id(&db, &bt_config.symbol_id)?;
    drop(db);

    let timeframe_key = bt_config.timeframe.as_str().to_string();
    let parquet_path = symbol
        .timeframe_paths
        .get(&timeframe_key)
        .ok_or_else(|| AppError::NotFound(format!(
            "Timeframe {} not available for {}",
            timeframe_key, symbol.name
        )))?
        .clone();

    let date_filter = loader::build_date_filter(&bt_config.start_date, &bt_config.end_date);
    let mut lf = loader::scan_parquet_lazy(&PathBuf::from(&parquet_path))?;
    if let Some(f) = &date_filter {
        lf = lf.filter(f.clone());
    }
    let df = lf.collect()
        .map_err(|e| AppError::Internal(format!("candle lazy collect: {}", e)))?;
    let candles = executor::candles_from_dataframe(&df)?;
    if candles.is_empty() {
        return Err(AppError::NoDataInRange);
    }

    info!("Walk-forward data: {} candles", candles.len());

    let cancel_flag = state.optimization_cancel_flag.clone();
    let instrument = symbol.instrument_config.clone();

    let result = tokio::task::spawn_blocking(move || {
        walk_forward::run_walk_forward(
            &candles,
            &strategy,
            &wf_config,
            &instrument,
            &cancel_flag,
            |pct, current, total| {
                let _ = app.emit(
                    "walk-forward-progress",
                    serde_json::json!({
                        "percent": pct,
                        "current_window": current,
                        "total_windows": total,
                    }),
                );
            },
        )
    })
    .await
    .map_err(|e| AppError::Internal(format!("Task join error: {}", e)))??;

    info!(
        "Walk-forward complete: {} windows, efficiency_ratio={:.2}",
        result.windows.len(),
        result.efficiency_ratio
    );

    Ok(result)
}

// ── Monte Carlo Simulation ──

/// Run a Monte Carlo simulation on a list of historical trades.
///
/// Run a Monte Carlo simulation on historical trades.
///
/// Accepts a `MonteCarloConfig` that specifies which methods to apply
/// (resampling and/or skip trades), the number of simulations, and the skip probability.
/// Returns a confidence-level table plus sampled equity curves for visualization.
#[tauri::command]
pub async fn run_monte_carlo(
    state: tauri::State<'_, AppState>,
    trades: Vec<crate::models::trade::TradeResult>,
    initial_capital: f64,
    config: MonteCarloConfig,
) -> Result<MonteCarloResult, AppError> {
    info!(
        "Running Monte Carlo: {} trades, {} simulations, resample={}, skip={}",
        trades.len(),
        config.n_simulations,
        config.use_resampling,
        config.use_skip_trades,
    );

    state.optimization_cancel_flag.store(false, Ordering::Relaxed);
    let cancel_flag = state.optimization_cancel_flag.clone();

    let result = tokio::task::spawn_blocking(move || {
        monte_carlo::run_monte_carlo(&trades, initial_capital, &config, &cancel_flag)
    })
    .await
    .map_err(|e| AppError::Internal(format!("Task join error: {}", e)))?;

    info!("Monte Carlo complete: {} simulations", result.n_simulations);
    Ok(result)
}

// ── Export Commands ──

/// Export trades to a CSV file.
#[tauri::command]
pub async fn export_trades_csv(
    trades: Vec<TradeResult>,
    file_path: String,
) -> Result<(), AppError> {
    info!("Exporting {} trades to CSV: {}", trades.len(), file_path);
    export::write_trades_csv(&trades, &PathBuf::from(&file_path))?;
    info!("Trades exported successfully");
    Ok(())
}

/// Export backtest metrics to a CSV report.
#[tauri::command]
pub async fn export_metrics_csv(
    metrics: BacktestMetrics,
    file_path: String,
) -> Result<(), AppError> {
    info!("Exporting metrics report to CSV: {}", file_path);
    export::write_metrics_csv(&metrics, &PathBuf::from(&file_path))?;
    info!("Metrics report exported successfully");
    Ok(())
}

/// Export a full backtest report as HTML.
#[tauri::command]
pub async fn export_report_html(
    results: BacktestResults,
    file_path: String,
) -> Result<(), AppError> {
    info!("Exporting HTML report to: {}", file_path);
    export::write_report_html(&results, &PathBuf::from(&file_path))?;
    info!("HTML report exported successfully");
    Ok(())
}

/// Export raw tick data for a symbol to a CSV file in MetaTrader 5 import format.
///
/// MT5 format: `Date,Bid,Ask,Last,Volume,Flags`
/// Date: `YYYY.MM.DD HH:MM:SS.mmm`
/// Only available for symbols whose base timeframe is Tick.
#[tauri::command]
pub async fn export_tick_data_mt5(
    state: tauri::State<'_, AppState>,
    symbol_id: String,
    file_path: String,
) -> Result<usize, AppError> {
    let db = state.db.lock().await;
    let symbol = storage::get_symbol_by_id(&db, &symbol_id)?;
    drop(db);

    let tick_raw_path = symbol
        .timeframe_paths
        .get("tick_raw")
        .ok_or_else(|| AppError::NotFound(
            format!("Symbol '{}' has no raw tick data (tick_raw timeframe)", symbol.name)
        ))?;

    info!("Exporting tick data to MT5 CSV: {} → {}", symbol.name, file_path);
    let rows = export::write_tick_mt5_csv(tick_raw_path, &PathBuf::from(&file_path))?;
    info!("MT5 export complete: {} rows written to {}", rows, file_path);

    Ok(rows)
}

// ── Code Generation Commands ──

/// Generate strategy code for MQL5 or PineScript.
#[tauri::command]
pub async fn generate_strategy_code(
    language: String,
    strategy: Strategy,
) -> Result<codegen::CodeGenerationResult, AppError> {
    info!("Generating {} code for strategy: {}", language, strategy.name);

    let result = match language.to_lowercase().as_str() {
        "mql5" => codegen::generate_mql5(&strategy)?,
        "pinescript" => codegen::generate_pinescript(&strategy)?,
        _ => return Err(AppError::InvalidConfig(format!(
            "Unsupported language: {}. Use 'mql5' or 'pinescript'",
            language
        ))),
    };

    let total_lines: usize = result.files.iter().map(|f| f.code.lines().count()).sum();
    info!("Code generation complete: {} files, {} total lines", result.files.len(), total_lines);
    Ok(result)
}

// ── Helpers ──

/// Emit conversion progress to the frontend.
// ── Download Commands ──

/// Download historical tick data from Dukascopy servers and import it.
/// `base_timeframe` can be "tick" (raw ticks) or "m1" (aggregate to 1-minute OHLCV bars).
#[tauri::command]
pub async fn download_dukascopy(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    symbol_name: String,
    duka_symbol: String,
    point_value: f64,
    start_date: String,
    end_date: String,
    base_timeframe: String,
    instrument_config: InstrumentConfig,
    tick_storage_format: Option<TickStorageFormat>,
    tick_pipeline: Option<TickPipeline>,
    keep_csv: Option<bool>,
) -> Result<Symbol, AppError> {
    use crate::data::dukascopy;

    let tick_storage_format = tick_storage_format.unwrap_or_default();

    // Sanitize symbol name (prevent path traversal)
    sanitize_symbol_name(&symbol_name)?;

    let is_tick_mode = base_timeframe == "tick";

    // Parse dates
    let start = chrono::NaiveDate::parse_from_str(&start_date, "%Y-%m-%d")
        .map_err(|e| AppError::InvalidConfig(format!("Invalid start date: {}", e)))?;
    let end = chrono::NaiveDate::parse_from_str(&end_date, "%Y-%m-%d")
        .map_err(|e| AppError::InvalidConfig(format!("Invalid end date: {}", e)))?;

    if start >= end {
        return Err(AppError::InvalidConfig(
            "Start date must be before end date".to_string(),
        ));
    }

    // Create per-download cancel flag
    let cancel_flag = std::sync::Arc::new(AtomicBool::new(false));
    {
        let mut flags = state.download_cancel_flags.lock().await;
        flags.insert(symbol_name.clone(), cancel_flag.clone());
    }

    let data_dir = state.data_dir.clone();
    let sym_name_for_cleanup = symbol_name.clone();
    let state_inner = state.inner().download_cancel_flags.clone();

    // Ensure we clean up the cancel flag when done (success or error)
    let result = async {
        // Check cancellation
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::DownloadCancelled);
        }

        // Phase 2: Convert directly to storage format (no intermediate CSV)
        let symbol_dir = data_dir.join("symbols").join(&symbol_name);
        std::fs::create_dir_all(&symbol_dir)?;

        let (total_rows, data_start, data_end, timeframe_paths, final_base_tf) = if is_tick_mode {
            let tick_dir = symbol_dir.join("tick");
            let tick_raw_dir = symbol_dir.join("tick_raw");

            let pipeline = tick_pipeline.unwrap_or_default();

            let (total_rows, data_start, data_end) = match pipeline {
                TickPipeline::Direct => {
                    // ── Direct path: bi5 → YearBuffer → Parquet/Binary ──
                    let app_clone = app.clone();
                    let sym_clone = symbol_name.clone();
                    dukascopy::download_symbol_direct(
                        &duka_symbol,
                        point_value,
                        start,
                        end,
                        &tick_dir,
                        &tick_raw_dir,
                        tick_storage_format,
                        instrument_config.tz_offset_hours,
                        &cancel_flag,
                        move |pct, msg| {
                            let mapped = (pct as f64 * 0.92) as u8;
                            emit_download_progress(&app_clone, &sym_clone, mapped, msg);
                        },
                    ).await?
                }

                TickPipeline::ViaCsv => {
                    // ── Via CSV path: bi5 → raw_ticks.csv → stream_tick_csv_to_parquet ──
                    let csv_path = symbol_dir.join("raw_ticks.csv");

                    // Phase 1: download bi5 → CSV (0–60%)
                    {
                        let app_clone = app.clone();
                        let sym_clone = symbol_name.clone();
                        dukascopy::download_symbol(
                            &duka_symbol,
                            point_value,
                            start,
                            end,
                            &csv_path,
                            &cancel_flag,
                            move |pct, msg| {
                                let mapped = (pct as f64 * 0.60) as u8;
                                emit_download_progress(&app_clone, &sym_clone, mapped, msg);
                            },
                        ).await?;
                    }

                    emit_download_progress(&app, &symbol_name, 62, "Converting CSV to storage format...");

                    // Phase 2: CSV → Parquet/Binary — same code path as manual import (62–92%)
                    let validation = validator::ValidationResult {
                        format: DataFormat::Tick,
                        has_header: true,
                        delimiter: b',',
                        row_count_sample: 0,
                        column_count: 4,
                    };

                    let app_clone = app.clone();
                    let sym_clone = symbol_name.clone();
                    let (total_rows, data_start, data_end) = loader::stream_tick_csv_to_parquet(
                        &csv_path,
                        &validation,
                        &tick_dir,
                        &tick_raw_dir,
                        tick_storage_format,
                        instrument_config.tz_offset_hours,
                        move |pct, msg| {
                            let mapped = 62 + (pct as f64 * 0.30) as u8;
                            emit_download_progress(&app_clone, &sym_clone, mapped, msg);
                        },
                    )?;

                    // Phase 3: optionally remove the intermediate CSV
                    if !keep_csv.unwrap_or(false) {
                        if let Err(e) = std::fs::remove_file(&csv_path) {
                            tracing::warn!("Could not remove intermediate CSV {}: {}", csv_path.display(), e);
                        }
                    } else {
                        info!("Keeping intermediate CSV at {}", csv_path.display());
                    }

                    (total_rows, data_start, data_end)
                }
            };

            emit_download_progress(&app, &symbol_name, 93, "Generating timeframes...");
            let mut timeframe_paths = converter::generate_timeframes_from_partitions(
                &tick_dir,
                &symbol_dir,
            )?;
            timeframe_paths.insert("tick".into(), tick_dir.to_string_lossy().into());
            timeframe_paths.insert("tick_raw".into(), tick_raw_dir.to_string_lossy().into());

            (total_rows, data_start, data_end, timeframe_paths, Timeframe::Tick)
        } else {
            // ── M1 mode: aggregate ticks to M1 directly in memory (no CSV) ──
            let app_clone = app.clone();
            let sym_clone = symbol_name.clone();
            let df = dukascopy::download_symbol_m1_direct(
                &duka_symbol,
                point_value,
                start,
                end,
                instrument_config.tz_offset_hours,
                &cancel_flag,
                |pct, msg| {
                    let mapped = (pct as f64 * 0.85) as u8;
                    emit_download_progress(&app_clone, &sym_clone, mapped, msg);
                },
            ).await?;
            let total_rows = df.height();
            info!("Aggregated to {} M1 bars", total_rows);

            let (data_start, data_end) = loader::get_date_range(&df)?;

            emit_download_progress(&app, &symbol_name, 88, "Generating timeframes...");
            let timeframe_paths =
                converter::generate_all_timeframes(&df, Timeframe::M1, &symbol_dir)?;

            (total_rows, data_start, data_end, timeframe_paths, Timeframe::M1)
        };

        // Phase 3: Save to database (98-100%)
        emit_download_progress(&app, &symbol_name, 98, "Saving to database...");
        let symbol_id = uuid::Uuid::new_v4().to_string();
        let upload_date = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let symbol = Symbol {
            id: symbol_id,
            name: symbol_name.clone(),
            base_timeframe: final_base_tf,
            upload_date,
            total_rows,
            start_date: data_start,
            end_date: data_end,
            timeframe_paths,
            instrument_config,
        };

        let db = state.db.lock().await;
        storage::insert_symbol(&db, &symbol)?;

        emit_download_progress(&app, &symbol_name, 100, "Done!");
        info!(
            "Dukascopy download complete: {} ({} rows, mode={})",
            symbol.name, symbol.total_rows, base_timeframe
        );

        Ok(symbol)
    }.await;

    // Clean up cancel flag
    {
        let mut flags = state_inner.lock().await;
        flags.remove(&sym_name_for_cleanup);
    }

    result
}

/// Cancel an ongoing download by symbol name.
#[tauri::command]
pub async fn cancel_download(
    state: tauri::State<'_, AppState>,
    symbol_name: String,
) -> Result<(), AppError> {
    let flags = state.download_cancel_flags.lock().await;
    if let Some(flag) = flags.get(&symbol_name) {
        flag.store(true, Ordering::Relaxed);
        info!("Download cancellation requested for: {}", symbol_name);
    } else {
        info!("No active download found for: {}", symbol_name);
    }
    Ok(())
}

// ── License Commands ──

/// Validate a license key and optionally save credentials.
#[tauri::command]
pub async fn validate_license(
    state: tauri::State<'_, AppState>,
    username: String,
    license_key: String,
    remember: bool,
) -> Result<crate::license::LicenseResponse, AppError> {
    let response = crate::license::validate_license(&username, &license_key).await;

    if response.valid {
        // Update the tier in app state
        let mut tier = state.license_tier.lock().await;
        *tier = response.tier;

        // Persist credentials if "remember me" is checked
        if remember {
            crate::license::save_credentials(&state.data_dir, &username, &license_key)?;
        } else {
            // If not remembering, clear any previously saved credentials
            crate::license::clear_credentials(&state.data_dir)?;
        }
    }

    Ok(response)
}

/// Load saved credentials from disk (for auto-login).
#[tauri::command]
pub async fn load_saved_license(
    state: tauri::State<'_, AppState>,
) -> Result<Option<crate::license::SavedCredentials>, AppError> {
    Ok(crate::license::load_credentials(&state.data_dir))
}

/// Clear saved license and reset tier to Free.
#[tauri::command]
pub async fn clear_license(
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    let mut tier = state.license_tier.lock().await;
    *tier = crate::license::LicenseTier::Free;
    crate::license::clear_credentials(&state.data_dir)?;
    Ok(())
}

/// Start background license monitor that re-validates every hour.
/// Emits "license-tier-changed" event if the tier changes.
#[tauri::command]
pub async fn start_license_monitor(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    let data_dir = state.data_dir.clone();
    let license_tier = state.license_tier.clone();

    tokio::spawn(async move {
        let interval = std::time::Duration::from_secs(3600); // 1 hour
        loop {
            tokio::time::sleep(interval).await;

            // Load saved credentials to re-validate
            let creds = match crate::license::load_credentials(&data_dir) {
                Some(c) => c,
                None => continue,
            };

            let response =
                crate::license::validate_license(&creds.username, &creds.license_key).await;
            let new_tier = if response.valid {
                response.tier
            } else {
                crate::license::LicenseTier::Free
            };

            let mut current = license_tier.lock().await;
            if *current != new_tier {
                info!(
                    "License tier changed: {:?} -> {:?} (user: {})",
                    *current, new_tier, creds.username
                );
                *current = new_tier;
                let tier_str = match new_tier {
                    crate::license::LicenseTier::Pro => "pro",
                    crate::license::LicenseTier::Free => "free",
                };
                let _ = app.emit(
                    "license-tier-changed",
                    serde_json::json!({ "tier": tier_str }),
                );
            }
        }
    });

    Ok(())
}

// ── Helpers ──

/// Validates a symbol name to prevent path traversal attacks.
///
/// Allowed characters: letters, digits, underscore, hyphen, and a single dot
/// (not two consecutive dots). Names starting or ending with a dot are rejected.
/// Returns an error if the name is invalid.
fn sanitize_symbol_name(name: &str) -> Result<(), AppError> {
    if name.is_empty() || name.len() > 64 {
        return Err(AppError::InvalidConfig(
            "Symbol name must be between 1 and 64 characters".to_string(),
        ));
    }
    // Reject path separators and parent-directory components
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(AppError::InvalidConfig(format!(
            "Symbol name contains illegal characters: '{}'",
            name
        )));
    }
    // Reject null bytes and control characters
    if name.chars().any(|c| c == '\0' || c.is_control()) {
        return Err(AppError::InvalidConfig(
            "Symbol name contains control characters".to_string(),
        ));
    }
    // Allow only: A-Z a-z 0-9 _ - . (and not leading/trailing dot)
    let valid = name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.');
    if !valid || name.starts_with('.') || name.ends_with('.') {
        return Err(AppError::InvalidConfig(format!(
            "Symbol name '{}' contains illegal characters. Use only letters, digits, '_', '-', or '.'",
            name
        )));
    }
    Ok(())
}

fn emit_download_progress(app: &AppHandle, symbol_name: &str, percent: u8, message: &str) {
    let _ = app.emit(
        "download-progress",
        serde_json::json!({ "symbol_name": symbol_name, "percent": percent, "message": message }),
    );
}

fn emit_progress(app: &AppHandle, percent: u8, message: &str) {
    let _ = app.emit(
        "conversion-progress",
        serde_json::json!({ "percent": percent, "message": message }),
    );
}

/// Convert a DataFrame to a Vec of JSON objects for the frontend.
///
/// Uses columnar access: each column is scanned once sequentially, which is
/// O(rows × cols) with cache-friendly access instead of O(n²) random `.get(i)` calls.
fn dataframe_to_json(df: &polars::prelude::DataFrame) -> Result<Vec<Value>, AppError> {
    use polars::prelude::DataType;

    let n_rows = df.height();
    let n_cols = df.width();

    if n_rows == 0 {
        return Ok(Vec::new());
    }

    // Scan each column once and extract all its values (columnar → cache-friendly).
    let mut col_values: Vec<Vec<Value>> = Vec::with_capacity(n_cols);
    for col in df.get_columns() {
        let values: Vec<Value> = match col.dtype() {
            DataType::Float64 => col
                .f64()
                .map_err(|e| AppError::Internal(e.to_string()))?
                .iter()
                .map(|v| {
                    v.and_then(|f| serde_json::Number::from_f64(f).map(Value::Number))
                        .unwrap_or(Value::Null)
                })
                .collect(),
            DataType::Float32 => col
                .f32()
                .map_err(|e| AppError::Internal(e.to_string()))?
                .iter()
                .map(|v| {
                    v.and_then(|f| serde_json::Number::from_f64(f as f64).map(Value::Number))
                        .unwrap_or(Value::Null)
                })
                .collect(),
            DataType::Int64 => col
                .i64()
                .map_err(|e| AppError::Internal(e.to_string()))?
                .iter()
                .map(|v| v.map(Value::from).unwrap_or(Value::Null))
                .collect(),
            DataType::Int32 => col
                .i32()
                .map_err(|e| AppError::Internal(e.to_string()))?
                .iter()
                .map(|v| v.map(Value::from).unwrap_or(Value::Null))
                .collect(),
            DataType::UInt64 => col
                .u64()
                .map_err(|e| AppError::Internal(e.to_string()))?
                .iter()
                .map(|v| v.map(Value::from).unwrap_or(Value::Null))
                .collect(),
            DataType::UInt32 => col
                .u32()
                .map_err(|e| AppError::Internal(e.to_string()))?
                .iter()
                .map(|v| v.map(Value::from).unwrap_or(Value::Null))
                .collect(),
            DataType::Boolean => col
                .bool()
                .map_err(|e| AppError::Internal(e.to_string()))?
                .iter()
                .map(|v| v.map(Value::Bool).unwrap_or(Value::Null))
                .collect(),
            DataType::String => col
                .str()
                .map_err(|e| AppError::Internal(e.to_string()))?
                .iter()
                .map(|v| v.map(|s| Value::String(s.to_string())).unwrap_or(Value::Null))
                .collect(),
            DataType::Datetime(_, _) => {
                // Cast to string for human-readable ISO-8601 representation.
                col.cast(&DataType::String)
                    .map_err(|e| AppError::Internal(e.to_string()))?
                    .str()
                    .map_err(|e| AppError::Internal(e.to_string()))?
                    .iter()
                    .map(|v| v.map(|s| Value::String(s.to_string())).unwrap_or(Value::Null))
                    .collect()
            }
            _ => {
                // Fallback for remaining types: use AnyValue (slower but correct).
                (0..n_rows)
                    .map(|i| col.get(i).map(|av| anyvalue_to_json(&av)).unwrap_or(Value::Null))
                    .collect()
            }
        };
        col_values.push(values);
    }

    // Assemble rows by transposing the per-column vecs.
    let col_names: Vec<String> = df.get_column_names().iter().map(|s| s.to_string()).collect();
    let mut rows = Vec::with_capacity(n_rows);
    for i in 0..n_rows {
        let mut row = serde_json::Map::with_capacity(n_cols);
        for (j, name) in col_names.iter().enumerate() {
            row.insert(name.clone(), col_values[j][i].clone());
        }
        rows.push(Value::Object(row));
    }

    Ok(rows)
}

/// Convert a Polars AnyValue to a serde_json Value.
fn anyvalue_to_json(val: &polars::prelude::AnyValue) -> Value {
    use polars::prelude::AnyValue;
    match val {
        AnyValue::Null => Value::Null,
        AnyValue::Boolean(b) => Value::Bool(*b),
        AnyValue::Int8(n) => Value::Number((*n).into()),
        AnyValue::Int16(n) => Value::Number((*n).into()),
        AnyValue::Int32(n) => Value::Number((*n).into()),
        AnyValue::Int64(n) => Value::Number((*n).into()),
        AnyValue::UInt8(n) => Value::Number((*n).into()),
        AnyValue::UInt16(n) => Value::Number((*n).into()),
        AnyValue::UInt32(n) => Value::Number((*n).into()),
        AnyValue::UInt64(n) => Value::Number((*n).into()),
        AnyValue::Float32(f) => {
            serde_json::Number::from_f64(*f as f64)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        AnyValue::Float64(f) => {
            serde_json::Number::from_f64(*f)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        AnyValue::String(s) => Value::String(s.to_string()),
        _ => Value::String(format!("{}", val)),
    }
}

// ── Builder Commands ──

/// Start the builder (GP strategy evolution).
#[tauri::command]
pub async fn start_builder(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    builder_config: BuilderConfig,
    symbol_id: String,
    timeframe: Timeframe,
    start_date: String,
    end_date: String,
    initial_capital: f64,
) -> Result<(), AppError> {
    use std::sync::atomic::Ordering;

    // Reset flags
    state.builder_cancel_flag.store(false, Ordering::SeqCst);
    state.builder_pause_flag.store(false, Ordering::SeqCst);

    // Load symbol
    let db = state.db.lock().await;
    let symbol = storage::get_symbol_by_id(&db, &symbol_id)?;
    drop(db);

    // Load candles (with optional date filter pushed down to Parquet)
    let tf_key = timeframe.as_str().to_string();
    let tf_path = symbol.timeframe_paths.get(&tf_key)
        .ok_or_else(|| AppError::Internal(format!("No data for timeframe {tf_key}")))?;
    let path = std::path::Path::new(tf_path);
    let date_filter = loader::build_date_filter(&start_date, &end_date);
    let mut lf = loader::scan_parquet_lazy(path)?;
    if let Some(f) = &date_filter {
        lf = lf.filter(f.clone());
    }
    let df = lf.collect()
        .map_err(|e| AppError::Internal(format!("parquet collect: {}", e)))?;
    let candles = executor::candles_from_dataframe(&df)?;

    let instrument = symbol.instrument_config.clone();
    let cancel_flag = state.builder_cancel_flag.clone();
    let pause_flag = state.builder_pause_flag.clone();
    let app_handle = app.clone();

    let precision = builder_config.data_config.precision;
    let backtest_config = BacktestConfig {
        symbol_id: symbol_id.clone(),
        timeframe: timeframe.clone(),
        start_date,
        end_date,
        initial_capital,
        leverage: 1.0,
        precision,
        // Abort evaluation early if a strategy hasn't traded in the first 30% of bars.
        // Eliminates ~60-80% of wasted compute on zero-trade random strategies.
        early_stop_no_trades_pct: Some(0.30),
    };

    // Channel + drain thread: builder sends progress events through a channel,
    // a dedicated drain thread forwards them as Tauri events.
    // When the builder finishes (tx dropped), the drain thread emits builder-finished.
    let (tx, rx) = std::sync::mpsc::sync_channel::<builder::BuilderProgressEvent>(1024);
    let drain_handle = app_handle.clone();
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            match event {
                builder::BuilderProgressEvent::Stats(stats) => {
                    let _ = drain_handle.emit("builder-stats", &stats);
                }
                builder::BuilderProgressEvent::Log(msg) => {
                    let _ = drain_handle.emit("builder-log", &msg);
                }
                builder::BuilderProgressEvent::StrategyFound(strat) => {
                    let _ = drain_handle.emit("builder-strategy-found", &strat);
                }
                builder::BuilderProgressEvent::IslandStats(stats) => {
                    let _ = drain_handle.emit("builder-island-stats", &stats);
                }
            }
        }
        // tx dropped → builder finished → emit finished event
        let _ = drain_handle.emit("builder-finished", ());
    });

    tokio::task::spawn_blocking(move || {
        // Keep a clone so we can send the final log BEFORE the channel closes
        let tx_final = tx.clone();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            builder::run_builder(
                &candles,
                &instrument,
                &backtest_config,
                &builder_config,
                &cancel_flag,
                &pause_flag,
                tx,
            )
        }));

        // Send completion/error log through the channel BEFORE dropping tx_final,
        // so the drain thread forwards it before emitting builder-finished.
        match result {
            Ok(Ok(strategies)) => {
                let _ = tx_final.send(builder::BuilderProgressEvent::Log(
                    format!("Builder completed: {} strategies in databank", strategies.len()),
                ));
            }
            Ok(Err(ref e)) => {
                if !matches!(e, AppError::BuilderCancelled) {
                    let _ = tx_final.send(builder::BuilderProgressEvent::Log(
                        format!("Builder error: {}", e),
                    ));
                }
            }
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "Unknown panic".to_string()
                };
                let _ = tx_final.send(builder::BuilderProgressEvent::Log(
                    format!("Builder PANIC: {}", msg),
                ));
            }
        }
        drop(tx_final);
        // tx_final dropped here → drain thread gets Err on recv() → emits builder-finished
    });

    Ok(())
}

/// Stop the builder.
#[tauri::command]
pub async fn stop_builder(
    state: tauri::State<'_, AppState>,
) -> Result<(), AppError> {
    state.builder_cancel_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    state.builder_pause_flag.store(false, std::sync::atomic::Ordering::SeqCst);
    info!("Builder stop requested");
    Ok(())
}

/// Pause or resume the builder.
#[tauri::command]
pub async fn pause_builder(
    state: tauri::State<'_, AppState>,
    paused: bool,
) -> Result<(), AppError> {
    state.builder_pause_flag.store(paused, std::sync::atomic::Ordering::SeqCst);
    info!("Builder pause = {paused}");
    Ok(())
}

// ── Project Commands ──────────────────────────────────────────────────────────

/// Save (create or update) a project to disk as `{data_dir}/projects/{id}.json`.
#[tauri::command]
pub async fn save_project(
    state: tauri::State<'_, AppState>,
    project: Project,
) -> Result<(), AppError> {
    let projects_dir = state.data_dir.join("projects");
    std::fs::create_dir_all(&projects_dir)?;
    let file_path = projects_dir.join(format!("{}.json", project.id));
    let json = serde_json::to_string_pretty(&project)?;
    std::fs::write(&file_path, json)?;
    Ok(())
}

/// Load all projects from `{data_dir}/projects/*.json`.
/// Skips files that fail to parse and logs a warning.
#[tauri::command]
pub async fn load_projects(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Project>, AppError> {
    let projects_dir = state.data_dir.join("projects");
    if !projects_dir.exists() {
        return Ok(vec![]);
    }
    let mut projects = Vec::new();
    for entry in std::fs::read_dir(&projects_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let json = std::fs::read_to_string(&path)?;
            match serde_json::from_str::<Project>(&json) {
                Ok(p) => projects.push(p),
                Err(e) => tracing::warn!("Skipping invalid project {:?}: {}", path, e),
            }
        }
    }
    Ok(projects)
}

/// Delete `{data_dir}/projects/{id}.json`. No-op if the file does not exist.
#[tauri::command]
pub async fn delete_project(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let file_path = state.data_dir.join("projects").join(format!("{}.json", id));
    if file_path.exists() {
        std::fs::remove_file(&file_path)?;
    }
    Ok(())
}

/// Open a project JSON from an arbitrary path via native file picker.
/// Returns `None` if the user cancelled the dialog.
#[tauri::command]
pub async fn open_project_from_path(
    app: AppHandle,
) -> Result<Option<Project>, AppError> {
    use tauri_plugin_dialog::DialogExt;
    let path = app
        .dialog()
        .file()
        .add_filter("Project files", &["json"])
        .blocking_pick_file();
    let Some(path) = path else { return Ok(None) };
    let file_path = path
        .into_path()
        .map_err(|e| AppError::FileRead(e.to_string()))?;
    let json = std::fs::read_to_string(&file_path)?;
    let project = serde_json::from_str::<Project>(&json)?;
    Ok(Some(project))
}
