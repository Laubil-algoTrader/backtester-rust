pub mod commands;
pub mod data;
pub mod engine;
pub mod errors;
pub mod license;
pub mod models;
pub mod utils;

use std::collections::HashMap;
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
    /// Cancellation flag for backtest operations.
    pub cancel_flag: Arc<AtomicBool>,
    /// Cancellation flag for optimization / walk-forward / Monte Carlo operations.
    /// Separate from `cancel_flag` so that cancelling a backtest does not
    /// accidentally abort a concurrently running optimization (or vice versa).
    pub optimization_cancel_flag: Arc<AtomicBool>,
    /// Per-download cancellation flags, keyed by symbol name.
    pub download_cancel_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    /// Current license tier (Arc for sharing with background monitor).
    pub license_tier: Arc<Mutex<license::LicenseTier>>,
    /// Cancellation flag for builder operations.
    pub builder_cancel_flag: Arc<AtomicBool>,
    /// Pause flag for builder operations.
    pub builder_pause_flag: Arc<AtomicBool>,
    /// Cancellation flag for SR (Symbolic Regression) builder operations.
    pub sr_cancel_flag: Arc<AtomicBool>,
}

/// Resolve the application data directory and ensure it exists.
fn get_data_dir() -> PathBuf {
    let dir = resolve_data_path().unwrap_or_else(|| PathBuf::from("./data"));
    fs::create_dir_all(&dir).ok();
    fs::create_dir_all(dir.join("symbols")).ok();
    fs::create_dir_all(dir.join("strategies")).ok();
    fs::create_dir_all(dir.join("projects")).ok();
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
        optimization_cancel_flag: Arc::new(AtomicBool::new(false)),
        download_cancel_flags: Arc::new(Mutex::new(HashMap::new())),
        license_tier: Arc::new(Mutex::new(license::LicenseTier::Free)),
        builder_cancel_flag: Arc::new(AtomicBool::new(false)),
        builder_pause_flag: Arc::new(AtomicBool::new(false)),
        sr_cancel_flag: Arc::new(AtomicBool::new(false)),
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
            commands::run_optimization,
            commands::cancel_optimization,
            commands::export_trades_csv,
            commands::export_metrics_csv,
            commands::export_report_html,
            commands::export_tick_data_mt5,
            commands::generate_strategy_code,
            commands::download_dukascopy,
            commands::cancel_download,
            commands::validate_license,
            commands::load_saved_license,
            commands::clear_license,
            commands::start_license_monitor,
            commands::run_walk_forward,
            commands::run_monte_carlo,
            commands::transform_symbol_timezone,
            commands::start_builder,
            commands::stop_builder,
            commands::pause_builder,
            commands::run_sr_builder,
            commands::cancel_sr_builder,
            commands::run_sr_backtest,
            commands::generate_sr_code,
            commands::save_project,
            commands::load_projects,
            commands::delete_project,
            commands::open_project_from_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
