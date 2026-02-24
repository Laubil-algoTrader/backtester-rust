use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tracing::info;

use crate::data::{converter, loader, storage, validator};
use crate::engine::{executor, optimizer};
use crate::engine::executor::SubBarData;
use crate::errors::AppError;
use crate::models::config::{DataFormat, InstrumentConfig, Timeframe};
use crate::models::result::{BacktestMetrics, BacktestResults, OosResult, OptimizationConfig, OptimizationMethod, OptimizationResult};
use crate::models::strategy::{BacktestConfig, BacktestPrecision, Strategy};
use crate::models::symbol::Symbol;
use crate::models::trade::TradeResult;
use crate::utils::{codegen, export};
use crate::AppState;

// ── Data Commands ──

/// Upload a CSV file, validate, convert to Parquet, generate timeframes, store in DB.
#[tauri::command]
pub async fn upload_csv(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    file_path: String,
    symbol_name: String,
    instrument_config: InstrumentConfig,
) -> Result<Symbol, AppError> {
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
            let df = loader::load_csv_to_dataframe(&path, &validation)?;
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

    // Clean up Parquet files
    for (_tf, path) in &symbol.timeframe_paths {
        if let Err(e) = std::fs::remove_file(path) {
            tracing::warn!("Failed to remove file {}: {}", path, e);
        }
    }

    // Try to remove the symbol directory if empty
    let symbol_dir = state.data_dir.join("symbols").join(&symbol.name);
    std::fs::remove_dir(&symbol_dir).ok();

    info!("Symbol deleted: {}", symbol.name);
    Ok(())
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

    let df = loader::load_parquet(&PathBuf::from(parquet_path))?;

    // Take first `limit` rows
    let preview = df.head(Some(limit));

    // Convert to Vec<serde_json::Value> for the frontend
    dataframe_to_json(&preview)
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
            // Partitioned loader: scans only relevant year files, column projection + date filter
            let filtered_df = loader::scan_tick_partitioned(
                tick_raw_path,
                &["datetime", "bid", "ask"],
                &config.start_date,
                &config.end_date,
            )?;
            let ticks = executor::tick_columns_from_dataframe(&filtered_df)?;
            info!("Loaded {} raw ticks as TickColumns with real spread", ticks.len());
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

    // Reset cancel flag
    state.cancel_flag.store(false, Ordering::Relaxed);

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

    let cancel_flag = state.cancel_flag.clone();
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
                // Reconstruct the strategy with this result's params
                let param_values: Vec<f64> = ranges.iter()
                    .map(|r| *opt_result.params.get(&r.display_name).unwrap_or(&0.0))
                    .collect();
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
                        Err(_) => {
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
    state.cancel_flag.store(true, Ordering::Relaxed);
    Ok(())
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
) -> Result<Symbol, AppError> {
    use crate::data::dukascopy;

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

    // Create temp directory for CSV
    let data_dir = state.data_dir.clone();
    let temp_dir = data_dir.join("temp");
    std::fs::create_dir_all(&temp_dir)?;
    let csv_path = temp_dir.join(format!("{}_dukascopy.csv", &symbol_name));

    let sym_name_for_cleanup = symbol_name.clone();
    let state_inner = state.inner().download_cancel_flags.clone();

    // Ensure we clean up the cancel flag when done (success or error)
    let result = async {
        // Phase 1: Download from Dukascopy (0-70%)
        let app_clone = app.clone();
        let sym_clone = symbol_name.clone();
        let _tick_count = dukascopy::download_symbol(
            &duka_symbol,
            point_value,
            start,
            end,
            &csv_path,
            &cancel_flag,
            move |pct, msg| {
                emit_download_progress(&app_clone, &sym_clone, pct, msg);
            },
        )
        .await?;

        // Check cancellation before proceeding
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(&csv_path);
            return Err(AppError::DownloadCancelled);
        }

        // Phase 2: Convert CSV → Parquet using existing pipeline (70-95%)
        emit_download_progress(&app, &symbol_name, 72, "Validating downloaded data...");
        let validation = validator::validate_csv(&csv_path)?;
        info!(
            "Validated downloaded CSV: format={:?}, rows={}",
            validation.format, validation.row_count_sample
        );

        let symbol_dir = data_dir.join("symbols").join(&symbol_name);
        std::fs::create_dir_all(&symbol_dir)?;

        let (total_rows, data_start, data_end, timeframe_paths, final_base_tf) = if is_tick_mode {
            // ── Tick mode: store raw ticks + generate all timeframes ──
            let tick_dir = symbol_dir.join("tick");
            let tick_raw_dir = symbol_dir.join("tick_raw");

            let app_clone = app.clone();
            let sym_clone = symbol_name.clone();
            let (total_rows, data_start, data_end) = loader::stream_tick_csv_to_parquet(
                &csv_path,
                &validation,
                &tick_dir,
                &tick_raw_dir,
                |pct, msg| {
                    let mapped = 72 + (pct as f64 * 0.20) as u8;
                    emit_download_progress(&app_clone, &sym_clone, mapped, msg);
                },
            )?;

            emit_download_progress(&app, &symbol_name, 93, "Generating timeframes...");
            let mut timeframe_paths = converter::generate_timeframes_from_partitions(
                &tick_dir,
                &symbol_dir,
            )?;
            timeframe_paths.insert("tick".into(), tick_dir.to_string_lossy().into());
            timeframe_paths.insert("tick_raw".into(), tick_raw_dir.to_string_lossy().into());

            (total_rows, data_start, data_end, timeframe_paths, Timeframe::Tick)
        } else {
            // ── M1 mode: aggregate ticks → M1 OHLCV, skip raw tick storage ──
            emit_download_progress(&app, &symbol_name, 75, "Aggregating ticks to M1 bars...");
            let df = loader::load_csv_to_dataframe(&csv_path, &validation)?;
            let total_rows = df.height();
            info!("Aggregated to {} M1 bars", total_rows);

            let (data_start, data_end) = loader::get_date_range(&df)?;

            emit_download_progress(&app, &symbol_name, 85, "Generating timeframes...");
            let timeframe_paths =
                converter::generate_all_timeframes(&df, Timeframe::M1, &symbol_dir)?;

            (total_rows, data_start, data_end, timeframe_paths, Timeframe::M1)
        };

        // Phase 4: Save to database (98-100%)
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

        // Clean up temp CSV
        let _ = std::fs::remove_file(&csv_path);

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
fn dataframe_to_json(df: &polars::prelude::DataFrame) -> Result<Vec<Value>, AppError> {
    let mut rows = Vec::with_capacity(df.height());

    for i in 0..df.height() {
        let mut row = serde_json::Map::new();
        for col in df.get_columns() {
            let val = col.get(i).map_err(|e| AppError::Internal(e.to_string()))?;
            row.insert(col.name().to_string(), anyvalue_to_json(&val));
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
