use serde::{Deserialize, Serialize};

/// A single OHLCV candle/bar.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Candle {
    /// Microseconds since epoch — used for fast sub-bar range lookups.
    /// Populated during DataFrame conversion, skipped in JSON serialization.
    #[serde(skip, default)]
    pub timestamp: i64,
    pub datetime: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// Raw tick data with bid/ask prices for real-spread precision mode.
/// NOTE: Kept for compatibility. For high-performance tick backtesting,
/// use `TickColumns` (struct-of-arrays layout) instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickData {
    pub datetime: String,
    pub bid: f64,
    pub ask: f64,
    pub volume: f64,
}

/// Columnar tick data for maximum cache efficiency in tick-level backtesting.
///
/// Uses struct-of-arrays (SoA) layout instead of array-of-structs to:
/// - Eliminate per-tick String allocations for datetimes (~1GB savings for 20M ticks)
/// - Enable i64 timestamp comparisons instead of string comparisons
/// - Maximize CPU cache locality when iterating bid/ask arrays
pub struct TickColumns {
    /// Timestamps in microseconds since epoch (i64 for fast comparison).
    pub timestamps: Vec<i64>,
    /// Bid prices (contiguous f64 array for cache-friendly iteration).
    pub bids: Vec<f64>,
    /// Ask prices (contiguous f64 array for cache-friendly iteration).
    pub asks: Vec<f64>,
}

impl TickColumns {
    pub fn len(&self) -> usize {
        self.timestamps.len()
    }
}
