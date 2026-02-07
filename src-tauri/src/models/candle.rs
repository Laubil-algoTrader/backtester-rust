use serde::{Deserialize, Serialize};

/// A single OHLCV candle/bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candle {
    pub datetime: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}
