use serde::{Deserialize, Serialize};

/// Instrument-specific configuration. Set per symbol at import time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstrumentConfig {
    /// Size of one pip (e.g. 0.0001 for EUR/USD, 0.01 for USD/JPY)
    pub pip_size: f64,
    /// Monetary value of 1 pip per 1 standard lot
    pub pip_value: f64,
    /// Size of 1 standard lot (e.g. 100_000 for Forex)
    pub lot_size: f64,
    /// Minimum lot size (e.g. 0.01)
    pub min_lot: f64,
    /// Minimum price movement
    pub tick_size: f64,
    /// Number of decimal places
    pub digits: usize,
}

/// Supported timeframes for OHLCV data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Timeframe {
    Tick,
    M1,
    M5,
    M15,
    M30,
    H1,
    H4,
    D1,
}

impl Timeframe {
    /// Returns the duration in minutes (0 for tick).
    pub fn minutes(&self) -> u32 {
        match self {
            Timeframe::Tick => 0,
            Timeframe::M1 => 1,
            Timeframe::M5 => 5,
            Timeframe::M15 => 15,
            Timeframe::M30 => 30,
            Timeframe::H1 => 60,
            Timeframe::H4 => 240,
            Timeframe::D1 => 1440,
        }
    }

    /// Returns the Polars duration string for `group_by_dynamic`.
    pub fn polars_duration(&self) -> &'static str {
        match self {
            Timeframe::Tick => "1s", // not really used for grouping
            Timeframe::M1 => "1m",
            Timeframe::M5 => "5m",
            Timeframe::M15 => "15m",
            Timeframe::M30 => "30m",
            Timeframe::H1 => "1h",
            Timeframe::H4 => "4h",
            Timeframe::D1 => "1d",
        }
    }

    /// Returns all timeframes that should be generated from this base timeframe.
    /// E.g. from M1 -> [M5, M15, M30, H1, H4, D1]
    pub fn higher_timeframes(&self) -> Vec<Timeframe> {
        let all = [
            Timeframe::M1,
            Timeframe::M5,
            Timeframe::M15,
            Timeframe::M30,
            Timeframe::H1,
            Timeframe::H4,
            Timeframe::D1,
        ];
        all.into_iter()
            .filter(|tf| tf.minutes() > self.minutes())
            .collect()
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Timeframe::Tick => "tick",
            Timeframe::M1 => "m1",
            Timeframe::M5 => "m5",
            Timeframe::M15 => "m15",
            Timeframe::M30 => "m30",
            Timeframe::H1 => "h1",
            Timeframe::H4 => "h4",
            Timeframe::D1 => "d1",
        }
    }
}

impl std::fmt::Display for Timeframe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Timeframe {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tick" => Ok(Timeframe::Tick),
            "m1" => Ok(Timeframe::M1),
            "m5" => Ok(Timeframe::M5),
            "m15" => Ok(Timeframe::M15),
            "m30" => Ok(Timeframe::M30),
            "h1" => Ok(Timeframe::H1),
            "h4" => Ok(Timeframe::H4),
            "d1" => Ok(Timeframe::D1),
            _ => Err(format!("Unknown timeframe: {}", s)),
        }
    }
}

/// Detected CSV data format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    /// Tick data: DateTime, Bid, Ask, Volume
    Tick,
    /// OHLCV bar data (any timeframe)
    Bar,
}
