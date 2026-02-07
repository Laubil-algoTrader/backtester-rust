pub mod commands;
pub mod data;
pub mod engine;
pub mod errors;
pub mod models;
pub mod utils;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use rusqlite::Connection;
use tokio::sync::Mutex;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// Shared application state, accessible from all Tauri commands.
pub struct AppState {
    pub db: Mutex<Connection>,
    pub data_dir: PathBuf,
    /// Cancellation flag for long-running operations (backtest, optimization).
    pub cancel_flag: Arc<AtomicBool>,
}

/// Resolve the application data directory and ensure it exists.
fn get_data_dir() -> PathBuf {
    let dir = resolve_data_path().unwrap_or_else(|| PathBuf::from("./data"));
    fs::create_dir_all(&dir).ok();
    fs::create_dir_all(dir.join("symbols")).ok();
    fs::create_dir_all(dir.join("strategies")).ok();
    dir
}

/// Platform-aware data directory.
fn resolve_data_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let base = exe.parent()?;

    if cfg!(debug_assertions) {
        // In dev: walk up from src-tauri/target/debug to project root
        let mut dir = base.to_path_buf();
        for _ in 0..3 {
            dir = dir.parent()?.to_path_buf();
        }
        Some(dir.join("data"))
    } else {
        Some(base.join("data"))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Starting Backtester application");

    // Initialize database
    let data_dir = get_data_dir();
    let db_path = data_dir.join("backtester.db");
    let db_path_str = db_path.to_string_lossy().to_string();

    let conn = data::storage::initialize_database(&db_path_str)
        .expect("Failed to initialize database");
    info!("Database ready at {}", db_path_str);

    let app_state = AppState {
        db: Mutex::new(conn),
        data_dir,
        cancel_flag: Arc::new(AtomicBool::new(false)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::upload_csv,
            commands::get_symbols,
            commands::delete_symbol,
            commands::preview_data,
            commands::run_backtest,
            commands::cancel_backtest,
            commands::save_strategy,
            commands::load_strategies,
            commands::delete_strategy,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
