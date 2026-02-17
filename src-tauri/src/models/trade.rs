use serde::{Deserialize, Serialize};

use super::strategy::TradeDirection;

/// Reason a trade was closed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CloseReason {
    Signal,
    StopLoss,
    TakeProfit,
    TrailingStop,
    EndOfData,
    TimeClose,
}

/// A completed trade with all its details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeResult {
    pub id: String,
    pub direction: TradeDirection,
    pub entry_time: String,
    pub entry_price: f64,
    pub exit_time: String,
    pub exit_price: f64,
    pub lots: f64,
    pub pnl: f64,
    pub pnl_pips: f64,
    pub commission: f64,
    pub close_reason: CloseReason,
    pub duration_bars: usize,
    pub duration_time: String,
    pub mae: f64,
    pub mfe: f64,
}
