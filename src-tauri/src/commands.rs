use std::path::PathBuf;
use std::sync::atomic::Ordering;

use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tracing::info;

use crate::data::{converter, loader, storage, validator};
use crate::engine::executor;
use crate::errors::AppError;
use crate::models::config::{DataFormat, InstrumentConfig, Timeframe};
use crate::models::result::BacktestResults;
use crate::models::strategy::{BacktestConfig, Strategy};
use crate::models::symbol::Symbol;
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

    // 3. Load CSV into normalized DataFrame
    emit_progress(&app, 15, "Loading CSV data...");
    let df = loader::load_csv_to_dataframe(&path, &validation)?;
    let total_rows = df.height();
    info!("Loaded {} rows from CSV", total_rows);

    // 4. Get date range
    let (start_date, end_date) = loader::get_date_range(&df)?;

    // 5. Create symbol directory and generate all timeframes
    emit_progress(&app, 30, "Converting to Parquet...");
    let data_dir = state.data_dir.clone();
    let symbol_dir = data_dir.join("symbols").join(&symbol_name);
    std::fs::create_dir_all(&symbol_dir)?;

    emit_progress(&app, 40, "Generating timeframes...");
    let timeframe_paths =
        converter::generate_all_timeframes(&df, base_timeframe, &symbol_dir)?;

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

/// Run a backtest with the given strategy and configuration.
#[tauri::command]
pub async fn run_backtest(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    strategy: Strategy,
    config: BacktestConfig,
) -> Result<BacktestResults, AppError> {
    info!("Running backtest: strategy={}, symbol={}", strategy.name, config.symbol_id);

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

    // Load parquet and convert to candles
    let df = loader::load_parquet(&PathBuf::from(parquet_path))?;
    let all_candles = executor::candles_from_dataframe(&df)?;

    // Filter by date range
    let candles = executor::filter_candles_by_date(&all_candles, &config.start_date, &config.end_date);
    if candles.is_empty() {
        return Err(AppError::NoDataInRange);
    }

    info!("Backtest data: {} candles after date filter", candles.len());

    // Run the backtest (blocking computation in async context)
    let cancel_flag = state.cancel_flag.clone();
    let instrument = symbol.instrument_config.clone();

    let result = tokio::task::spawn_blocking(move || {
        executor::run_backtest(
            &candles,
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

// ── Helpers ──

/// Emit conversion progress to the frontend.
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
