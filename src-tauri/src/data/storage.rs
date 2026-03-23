use rusqlite::{Connection, params};
use tracing::info;

use crate::errors::AppError;
use crate::models::config::InstrumentConfig;
use crate::models::config::Timeframe;
use crate::models::strategy::Strategy;
use crate::models::symbol::Symbol;

// ─────────────────────────────────────────────────────────────────────────────
// Database Initialization
// ─────────────────────────────────────────────────────────────────────────────

/// Initialize the SQLite database at the given path.
///
/// Creates all required tables if they do not yet exist.
pub fn initialize_database(db_path: &str) -> Result<Connection, AppError> {
    let conn = Connection::open(db_path)?;

    // Enable WAL mode for better concurrent read performance
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS symbols (
            id                  TEXT PRIMARY KEY,
            name                TEXT NOT NULL,
            base_timeframe      TEXT NOT NULL,
            upload_date         TEXT NOT NULL,
            total_rows          INTEGER NOT NULL,
            start_date          TEXT NOT NULL,
            end_date            TEXT NOT NULL,
            timeframe_paths     TEXT NOT NULL,
            instrument_config   TEXT NOT NULL,
            status              TEXT NOT NULL DEFAULT 'complete',
            download_params     TEXT
        );

        CREATE TABLE IF NOT EXISTS strategies (
            id              TEXT PRIMARY KEY,
            name            TEXT NOT NULL,
            created_at      TEXT NOT NULL,
            updated_at      TEXT NOT NULL,
            strategy_json   TEXT NOT NULL
        );
        ",
    )?;

    info!("Database initialized at {}", db_path);
    Ok(conn)
}

// ─────────────────────────────────────────────────────────────────────────────
// Symbol CRUD
// ─────────────────────────────────────────────────────────────────────────────

/// Insert a fully-processed symbol into the database.
pub fn insert_symbol(db: &Connection, symbol: &Symbol) -> Result<(), AppError> {
    let timeframe_paths_json = serde_json::to_string(&symbol.timeframe_paths)?;
    let instrument_config_json = serde_json::to_string(&symbol.instrument_config)?;
    let download_params_json = symbol
        .download_params
        .as_ref()
        .map(|v| serde_json::to_string(v))
        .transpose()?;

    db.execute(
        "INSERT INTO symbols
            (id, name, base_timeframe, upload_date, total_rows, start_date, end_date,
             timeframe_paths, instrument_config, status, download_params)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            symbol.id,
            symbol.name,
            symbol.base_timeframe.as_str(),
            symbol.upload_date,
            symbol.total_rows as i64,
            symbol.start_date,
            symbol.end_date,
            timeframe_paths_json,
            instrument_config_json,
            symbol.status,
            download_params_json,
        ],
    )?;

    info!("Inserted symbol: {} ({})", symbol.name, symbol.id);
    Ok(())
}

/// Retrieve all symbols from the database.
pub fn get_all_symbols(db: &Connection) -> Result<Vec<Symbol>, AppError> {
    let mut stmt = db.prepare(
        "SELECT id, name, base_timeframe, upload_date, total_rows, start_date, end_date,
                timeframe_paths, instrument_config, status, download_params
         FROM symbols
         ORDER BY upload_date DESC",
    )?;

    let symbols = stmt.query_map([], row_to_symbol)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(symbols)
}

/// Retrieve a single symbol by ID.
pub fn get_symbol_by_id(db: &Connection, id: &str) -> Result<Symbol, AppError> {
    let mut stmt = db.prepare(
        "SELECT id, name, base_timeframe, upload_date, total_rows, start_date, end_date,
                timeframe_paths, instrument_config, status, download_params
         FROM symbols WHERE id = ?1",
    )?;

    stmt.query_row(params![id], row_to_symbol)
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                AppError::NotFound(format!("Symbol not found: {}", id))
            }
            other => AppError::Database(other.to_string()),
        })
}

/// Delete a symbol by ID and return the deleted symbol.
pub fn delete_symbol_by_id(db: &Connection, id: &str) -> Result<Symbol, AppError> {
    let symbol = get_symbol_by_id(db, id)?;
    db.execute("DELETE FROM symbols WHERE id = ?1", params![id])?;
    info!("Deleted symbol: {} ({})", symbol.name, id);
    Ok(symbol)
}

/// Update the timezone offset and date range for a symbol (after tz transform).
pub fn update_symbol_tz(
    db: &Connection,
    id: &str,
    config: &InstrumentConfig,
    start: &str,
    end: &str,
) -> Result<(), AppError> {
    let config_json = serde_json::to_string(config)?;
    db.execute(
        "UPDATE symbols SET instrument_config = ?1, start_date = ?2, end_date = ?3
         WHERE id = ?4",
        params![config_json, start, end, id],
    )?;
    Ok(())
}

/// Insert a symbol with status "downloading" (before download completes).
pub fn insert_pending_symbol(db: &Connection, symbol: &Symbol) -> Result<(), AppError> {
    insert_symbol(db, symbol)
}

/// Mark a previously-pending symbol as complete and update all its fields.
pub fn complete_symbol(db: &Connection, symbol: &Symbol) -> Result<(), AppError> {
    let timeframe_paths_json = serde_json::to_string(&symbol.timeframe_paths)?;
    let instrument_config_json = serde_json::to_string(&symbol.instrument_config)?;

    db.execute(
        "UPDATE symbols SET
            name = ?1,
            base_timeframe = ?2,
            total_rows = ?3,
            start_date = ?4,
            end_date = ?5,
            timeframe_paths = ?6,
            instrument_config = ?7,
            status = 'complete',
            download_params = NULL
         WHERE id = ?8",
        params![
            symbol.name,
            symbol.base_timeframe.as_str(),
            symbol.total_rows as i64,
            symbol.start_date,
            symbol.end_date,
            timeframe_paths_json,
            instrument_config_json,
            symbol.id,
        ],
    )?;

    info!("Completed symbol: {} ({})", symbol.name, symbol.id);
    Ok(())
}

/// Delete a symbol that is still in "downloading" status (clean failure path).
pub fn delete_pending_symbol(db: &Connection, id: &str) -> Result<(), AppError> {
    db.execute(
        "DELETE FROM symbols WHERE id = ?1 AND status = 'downloading'",
        params![id],
    )?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Strategy CRUD
// ─────────────────────────────────────────────────────────────────────────────

/// Check if a strategy with the given ID already exists.
pub fn strategy_exists(db: &Connection, id: &str) -> Result<bool, AppError> {
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM strategies WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// Insert a new strategy. Returns the strategy ID.
pub fn insert_strategy(db: &Connection, strategy: &Strategy) -> Result<String, AppError> {
    let json = serde_json::to_string(strategy)?;
    db.execute(
        "INSERT INTO strategies (id, name, created_at, updated_at, strategy_json)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            strategy.id,
            strategy.name,
            strategy.created_at,
            strategy.updated_at,
            json,
        ],
    )?;
    info!("Inserted strategy: {} ({})", strategy.name, strategy.id);
    Ok(strategy.id.clone())
}

/// Update an existing strategy's fields.
pub fn update_strategy(db: &Connection, strategy: &Strategy) -> Result<(), AppError> {
    let json = serde_json::to_string(strategy)?;
    db.execute(
        "UPDATE strategies SET name = ?1, updated_at = ?2, strategy_json = ?3
         WHERE id = ?4",
        params![strategy.name, strategy.updated_at, json, strategy.id],
    )?;
    info!("Updated strategy: {} ({})", strategy.name, strategy.id);
    Ok(())
}

/// Retrieve all strategies.
pub fn get_all_strategies(db: &Connection) -> Result<Vec<Strategy>, AppError> {
    let mut stmt = db.prepare(
        "SELECT strategy_json FROM strategies ORDER BY updated_at DESC",
    )?;

    let strategies = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|json| serde_json::from_str::<Strategy>(&json))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Serialization(format!("deserialize strategy: {}", e)))?;

    Ok(strategies)
}

/// Delete a strategy by ID.
pub fn delete_strategy_by_id(db: &Connection, id: &str) -> Result<(), AppError> {
    db.execute("DELETE FROM strategies WHERE id = ?1", params![id])?;
    info!("Deleted strategy: {}", id);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Row deserialization helper
// ─────────────────────────────────────────────────────────────────────────────

fn row_to_symbol(row: &rusqlite::Row<'_>) -> Result<Symbol, rusqlite::Error> {
    let id: String = row.get(0)?;
    let name: String = row.get(1)?;
    let base_timeframe_str: String = row.get(2)?;
    let upload_date: String = row.get(3)?;
    let total_rows: i64 = row.get(4)?;
    let start_date: String = row.get(5)?;
    let end_date: String = row.get(6)?;
    let timeframe_paths_json: String = row.get(7)?;
    let instrument_config_json: String = row.get(8)?;
    let status: String = row.get(9)?;
    let download_params_json: Option<String> = row.get(10)?;

    let base_timeframe: Timeframe = base_timeframe_str
        .parse()
        .unwrap_or(Timeframe::M1);

    let timeframe_paths = serde_json::from_str(&timeframe_paths_json)
        .unwrap_or_default();

    let instrument_config: InstrumentConfig = serde_json::from_str(&instrument_config_json)
        .unwrap_or_default();

    let download_params = download_params_json
        .and_then(|json| serde_json::from_str(&json).ok());

    Ok(Symbol {
        id,
        name,
        base_timeframe,
        upload_date,
        total_rows: total_rows as usize,
        start_date,
        end_date,
        timeframe_paths,
        instrument_config,
        status,
        download_params,
    })
}
