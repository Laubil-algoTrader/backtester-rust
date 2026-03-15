use serde::{Deserialize, Serialize};

/// How swap rates are denominated.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SwapMode {
    /// Swap expressed in pips per lot per day (standard Forex).
    #[default]
    InPips,
    /// Swap expressed in points per lot per day.
    InPoints,
    /// Swap expressed in account currency per lot per day.
    InMoney,
    /// Swap as annual percentage of position value (divided by 365 per day).
    AsPercent,
}

fn default_triple_swap_day() -> u8 {
    3 // Wednesday (ISO weekday: Mon=1 … Sun=7)
}

fn default_swap_annual_days() -> u32 {
    365
}

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

    // ── Swap (overnight financing cost) ──

    /// Daily swap rate for long positions. Interpretation depends on `swap_mode`.
    /// Positive = credit to trader, negative = charge to trader.
    #[serde(default)]
    pub swap_long: f64,
    /// Daily swap rate for short positions.
    #[serde(default)]
    pub swap_short: f64,
    /// How `swap_long` / `swap_short` are denominated.
    #[serde(default)]
    pub swap_mode: SwapMode,
    /// ISO weekday (Mon=1 … Sun=7) on which triple swap is charged.
    /// Default 3 = Wednesday (covers Saturday + Sunday in Forex).
    #[serde(default = "default_triple_swap_day")]
    pub triple_swap_day: u8,
    /// Calendar days per year for `SwapMode::AsPercent` annualization.
    /// Forex = 365, equities = 252, crypto = 365.
    #[serde(default = "default_swap_annual_days")]
    pub swap_annual_days: u32,

    // ── Stops level ──

    /// Minimum distance in pips between entry price and SL/TP.
    /// Mirrors MT5's SYMBOL_TRADE_STOPS_LEVEL. Set to 0 to disable (default).
    #[serde(default)]
    pub min_stop_distance_pips: f64,

    // ── Timezone ──

    /// Timezone offset in hours applied to all timestamps at import time.
    /// Positive = shift forward (e.g. +3 for UTC+3), negative = shift backward (e.g. -5 for UTC-5).
    /// Default 0 = no shift (treat raw data timestamps as-is).
    /// Fractional values are supported (e.g. 5.5 for UTC+5:30, -3.5 for UTC-3:30).
    #[serde(default)]
    pub tz_offset_hours: f64,
}

impl Default for InstrumentConfig {
    /// Defaults to a standard 5-digit Forex major pair (e.g. EUR/USD).
    fn default() -> Self {
        Self {
            pip_size: 0.0001,
            pip_value: 10.0,
            lot_size: 100_000.0,
            min_lot: 0.01,
            tick_size: 0.00001,
            digits: 5,
            swap_long: 0.0,
            swap_short: 0.0,
            swap_mode: SwapMode::InPips,
            triple_swap_day: 3,
            swap_annual_days: 365,
            min_stop_distance_pips: 0.0,
            tz_offset_hours: 0.0,
        }
    }
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

/// Storage format for raw tick data (bid/ask).
///
/// Applies only to the `tick_raw/` partition; `tick/` OHLCV files always use Parquet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TickStorageFormat {
    /// Compressed columnar Parquet (default). Compatible with Polars tooling.
    #[default]
    Parquet,
    /// Flat binary: `i64_le timestamp_µs + f64_le bid + f64_le ask` = 24 bytes/tick.
    /// Zero parsing overhead at load time — direct memcpy into TickColumns.
    Binary,
}

/// Download pipeline for tick-mode Dukascopy downloads.
///
/// `Direct` (default): bi5 → YearBuffer → Parquet/Binary — fast, no intermediate files.
/// `ViaCsv`: bi5 → intermediate CSV → `stream_tick_csv_to_parquet()` — identical
/// to the manual CSV import path; use when the Direct path produces discrepancies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TickPipeline {
    #[default]
    Direct,
    ViaCsv,
}

/// Detected CSV data format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    /// Tick data: DateTime, Bid, Ask, Volume
    Tick,
    /// OHLCV bar data (any timeframe)
    Bar,
}
