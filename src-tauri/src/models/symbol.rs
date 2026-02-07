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
}
