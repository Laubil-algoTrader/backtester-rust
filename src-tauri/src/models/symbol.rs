use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::config::{InstrumentConfig, Timeframe};

/// A symbol with its metadata and paths to Parquet files per timeframe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub base_timeframe: Timeframe,
    pub upload_date: String,
    pub total_rows: usize,
    pub start_date: String,
    pub end_date: String,
    pub timeframe_paths: HashMap<String, String>,
    pub instrument_config: InstrumentConfig,
    /// "complete" | "downloading" — "downloading" means the download was started but
    /// not finished (e.g. the app was closed mid-download).
    #[serde(default = "default_status")]
    pub status: String,
    /// Parameters needed to resume an interrupted download. Only set when
    /// `status == "downloading"`.
    #[serde(default)]
    pub download_params: Option<serde_json::Value>,
}

fn default_status() -> String {
    "complete".to_string()
}
