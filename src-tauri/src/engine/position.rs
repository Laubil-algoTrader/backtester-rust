use crate::models::candle::Candle;
use crate::models::config::InstrumentConfig;
use crate::models::strategy::{
    PositionSizing, PositionSizingType, StopLoss, StopLossType, TakeProfit, TakeProfitType,
    TradeDirection, TrailingStop, TrailingStopType,
};
use crate::models::trade::CloseReason;

/// An open position being tracked during backtest execution.
#[derive(Debug, Clone)]
pub struct OpenPosition {
    pub direction: TradeDirection,
    pub entry_price: f64,
    pub entry_bar: usize,
    pub entry_time: String,
    pub lots: f64,
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub trailing_stop_distance: Option<f64>,
    /// Highest price since entry (for long positions / trailing stop).
    pub highest_since_entry: f64,
    /// Lowest price since entry (for short positions / trailing stop).
    pub lowest_since_entry: f64,
    /// Maximum adverse excursion (worst unrealized loss in pips).
    pub mae_pips: f64,
    /// Maximum favorable excursion (best unrealized profit in pips).
    pub mfe_pips: f64,
}

/// Calculate position size in lots.
pub fn calculate_lots(
    sizing: &PositionSizing,
    equity: f64,
    entry_price: f64,
    sl_price: Option<f64>,
    instrument: &InstrumentConfig,
) -> f64 {
    let raw = match sizing.sizing_type {
        PositionSizingType::FixedLots => sizing.value,
        PositionSizingType::FixedAmount => {
            // Fixed Amount = risk exactly $X per trade based on SL distance
            // lots = risk_amount / (sl_distance_pips * pip_value)
            if let Some(sl) = sl_price {
                let sl_distance_pips = (entry_price - sl).abs() / instrument.pip_size;
                if sl_distance_pips == 0.0 || instrument.pip_value == 0.0 {
                    return instrument.min_lot;
                }
                sizing.value / (sl_distance_pips * instrument.pip_value)
            } else {
                // No SL → can't calculate risk-based sizing, use min lot
                instrument.min_lot
            }
        }
        PositionSizingType::PercentEquity => {
            // Same as FixedAmount but amount = equity * percent / 100
            if let Some(sl) = sl_price {
                let sl_distance_pips = (entry_price - sl).abs() / instrument.pip_size;
                if sl_distance_pips == 0.0 || instrument.pip_value == 0.0 {
                    return instrument.min_lot;
                }
                let risk_amount = equity * sizing.value / 100.0;
                risk_amount / (sl_distance_pips * instrument.pip_value)
            } else {
                instrument.min_lot
            }
        }
        PositionSizingType::RiskBased => {
            // Risk-based: lots = (equity * risk%) / (SL distance in pips * pip_value)
            if let Some(sl) = sl_price {
                let sl_distance_pips = (entry_price - sl).abs() / instrument.pip_size;
                if sl_distance_pips == 0.0 || instrument.pip_value == 0.0 {
                    return instrument.min_lot;
                }
                let risk_amount = equity * sizing.value / 100.0;
                risk_amount / (sl_distance_pips * instrument.pip_value)
            } else {
                instrument.min_lot
            }
        }
    };

    // Clamp to min_lot and round to min_lot increments
    let lots = (raw / instrument.min_lot).floor() * instrument.min_lot;
    lots.max(instrument.min_lot)
}

/// Calculate stop loss price.
pub fn calculate_stop_loss(
    config: &StopLoss,
    entry_price: f64,
    direction: TradeDirection,
    atr_value: Option<f64>,
    instrument: &InstrumentConfig,
) -> f64 {
    let distance = match config.sl_type {
        StopLossType::Pips => config.value * instrument.pip_size,
        StopLossType::Percentage => entry_price * config.value / 100.0,
        StopLossType::ATR => {
            let atr = atr_value.unwrap_or(0.0);
            atr * config.value
        }
    };

    match direction {
        TradeDirection::Long | TradeDirection::Both => entry_price - distance,
        TradeDirection::Short => entry_price + distance,
    }
}

/// Calculate take profit price.
pub fn calculate_take_profit(
    config: &TakeProfit,
    entry_price: f64,
    sl_price: Option<f64>,
    direction: TradeDirection,
    atr_value: Option<f64>,
    instrument: &InstrumentConfig,
) -> f64 {
    let distance = match config.tp_type {
        TakeProfitType::Pips => config.value * instrument.pip_size,
        TakeProfitType::RiskReward => {
            // TP distance = SL distance * R:R ratio
            if let Some(sl) = sl_price {
                (entry_price - sl).abs() * config.value
            } else {
                config.value * instrument.pip_size * 10.0
            }
        }
        TakeProfitType::ATR => {
            let atr = atr_value.unwrap_or(0.0);
            atr * config.value
        }
    };

    match direction {
        TradeDirection::Long | TradeDirection::Both => entry_price + distance,
        TradeDirection::Short => entry_price - distance,
    }
}

/// Calculate trailing stop distance.
pub fn calculate_trailing_stop_distance(
    config: &TrailingStop,
    entry_price: f64,
    sl_price: Option<f64>,
    atr_value: Option<f64>,
    instrument: &InstrumentConfig,
) -> f64 {
    match config.ts_type {
        TrailingStopType::ATR => {
            let atr = atr_value.unwrap_or(0.0);
            atr * config.value
        }
        TrailingStopType::RiskReward => {
            if let Some(sl) = sl_price {
                (entry_price - sl).abs() * config.value
            } else {
                config.value * instrument.pip_size * 10.0
            }
        }
    }
}

/// Update the trailing stop for an open position. Returns the new stop loss price.
pub fn update_trailing_stop(position: &mut OpenPosition, candle: &Candle) {
    if let Some(distance) = position.trailing_stop_distance {
        match position.direction {
            TradeDirection::Long | TradeDirection::Both => {
                if candle.high > position.highest_since_entry {
                    position.highest_since_entry = candle.high;
                    let new_sl = position.highest_since_entry - distance;
                    if let Some(ref mut sl) = position.stop_loss {
                        if new_sl > *sl {
                            *sl = new_sl;
                        }
                    } else {
                        position.stop_loss = Some(new_sl);
                    }
                }
            }
            TradeDirection::Short => {
                if candle.low < position.lowest_since_entry {
                    position.lowest_since_entry = candle.low;
                    let new_sl = position.lowest_since_entry + distance;
                    if let Some(ref mut sl) = position.stop_loss {
                        if new_sl < *sl {
                            *sl = new_sl;
                        }
                    } else {
                        position.stop_loss = Some(new_sl);
                    }
                }
            }
        }
    }
}

/// Check if SL or TP was hit on the current candle.
/// Returns (exit_price, CloseReason) if triggered.
///
/// Handles three realistic scenarios:
/// 1. **Gap-through SL**: SL is a stop-market order. If price opens beyond
///    the SL level, fill is at the open (worse for the trader, realistic slippage).
/// 2. **TP fill**: TP is a limit order. Fills at the TP level (conservative).
/// 3. **Both hit on same candle**: The level closer to the open was hit first.
pub fn check_sl_tp_hit(
    position: &OpenPosition,
    candle: &Candle,
) -> Option<(f64, CloseReason)> {
    let sl_result = check_stop_loss(position, candle);
    let tp_result = check_take_profit(position, candle);

    match (sl_result, tp_result) {
        (None, None) => None,
        (Some(sl), None) => Some(sl),
        (None, Some(tp)) => Some(tp),
        (Some((sl_fill, _)), Some((tp_fill, _))) => {
            // Both triggered on same candle — closer level to open was hit first
            let dist_sl = (candle.open - sl_fill).abs();
            let dist_tp = (candle.open - tp_fill).abs();
            if dist_sl <= dist_tp {
                Some((sl_fill, CloseReason::StopLoss))
            } else {
                Some((tp_fill, CloseReason::TakeProfit))
            }
        }
    }
}

/// Check if stop loss was triggered. SL is a stop-market order:
/// gap-through fills at open (worse price for the trader).
fn check_stop_loss(pos: &OpenPosition, candle: &Candle) -> Option<(f64, CloseReason)> {
    let sl = pos.stop_loss?;
    match pos.direction {
        TradeDirection::Long | TradeDirection::Both => {
            if candle.low <= sl {
                // Gap-through: open already below SL → fill at open (worse)
                let fill = if candle.open <= sl { candle.open } else { sl };
                Some((fill, CloseReason::StopLoss))
            } else {
                None
            }
        }
        TradeDirection::Short => {
            if candle.high >= sl {
                // Gap-through: open already above SL → fill at open (worse)
                let fill = if candle.open >= sl { candle.open } else { sl };
                Some((fill, CloseReason::StopLoss))
            } else {
                None
            }
        }
    }
}

/// Check if take profit was triggered. TP is a limit order:
/// always fills at the TP level (conservative, standard behavior).
fn check_take_profit(pos: &OpenPosition, candle: &Candle) -> Option<(f64, CloseReason)> {
    let tp = pos.take_profit?;
    match pos.direction {
        TradeDirection::Long | TradeDirection::Both => {
            if candle.high >= tp {
                Some((tp, CloseReason::TakeProfit))
            } else {
                None
            }
        }
        TradeDirection::Short => {
            if candle.low <= tp {
                Some((tp, CloseReason::TakeProfit))
            } else {
                None
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════
// Tick-level functions (for RealTick precision modes)
// NOTE: These are kept for reference/testing but the executor now uses
// an optimized inlined loop in process_subbars_tick_columnar() instead.
// ══════════════════════════════════════════════════════════════

/// Check SL/TP hit at tick level (single bid/ask price point, no OHLC range).
/// For Long positions: exit at bid. For Short: exit at ask.
#[allow(dead_code)]
pub fn check_tick_sl_tp(
    pos: &OpenPosition,
    bid: f64,
    ask: f64,
) -> Option<(f64, CloseReason)> {
    let sl_result = check_tick_stop_loss(pos, bid, ask);
    let tp_result = check_tick_take_profit(pos, bid, ask);

    match (sl_result, tp_result) {
        (None, None) => None,
        (Some(sl), None) => Some(sl),
        (None, Some(tp)) => Some(tp),
        (Some(sl), Some(_tp)) => {
            // Both hit on same tick — SL (stop order) takes priority
            Some(sl)
        }
    }
}

/// Check stop loss at tick level. SL is a stop-market order.
#[allow(dead_code)]
fn check_tick_stop_loss(pos: &OpenPosition, bid: f64, ask: f64) -> Option<(f64, CloseReason)> {
    let sl = pos.stop_loss?;
    match pos.direction {
        TradeDirection::Long | TradeDirection::Both => {
            // Long exits at bid
            if bid <= sl {
                Some((bid, CloseReason::StopLoss))
            } else {
                None
            }
        }
        TradeDirection::Short => {
            // Short exits at ask
            if ask >= sl {
                Some((ask, CloseReason::StopLoss))
            } else {
                None
            }
        }
    }
}

/// Check take profit at tick level. TP is a limit order — fills at TP level.
#[allow(dead_code)]
fn check_tick_take_profit(pos: &OpenPosition, bid: f64, ask: f64) -> Option<(f64, CloseReason)> {
    let tp = pos.take_profit?;
    match pos.direction {
        TradeDirection::Long | TradeDirection::Both => {
            // Long exits at bid; TP limit fill at TP level
            if bid >= tp {
                Some((tp, CloseReason::TakeProfit))
            } else {
                None
            }
        }
        TradeDirection::Short => {
            // Short exits at ask; TP limit fill at TP level
            if ask <= tp {
                Some((tp, CloseReason::TakeProfit))
            } else {
                None
            }
        }
    }
}

/// Update trailing stop based on tick bid/ask prices.
#[allow(dead_code)]
pub fn update_trailing_stop_tick(pos: &mut OpenPosition, bid: f64, ask: f64) {
    if let Some(distance) = pos.trailing_stop_distance {
        match pos.direction {
            TradeDirection::Long | TradeDirection::Both => {
                // Track highest bid (selling price)
                if bid > pos.highest_since_entry {
                    pos.highest_since_entry = bid;
                    let new_sl = bid - distance;
                    if let Some(ref mut sl) = pos.stop_loss {
                        if new_sl > *sl {
                            *sl = new_sl;
                        }
                    } else {
                        pos.stop_loss = Some(new_sl);
                    }
                }
            }
            TradeDirection::Short => {
                // Track lowest ask (buying price)
                if ask < pos.lowest_since_entry {
                    pos.lowest_since_entry = ask;
                    let new_sl = ask + distance;
                    if let Some(ref mut sl) = pos.stop_loss {
                        if new_sl < *sl {
                            *sl = new_sl;
                        }
                    } else {
                        pos.stop_loss = Some(new_sl);
                    }
                }
            }
        }
    }
}

/// Update MAE/MFE based on tick bid/ask prices.
#[allow(dead_code)]
pub fn update_mae_mfe_tick(
    pos: &mut OpenPosition,
    bid: f64,
    ask: f64,
    instrument: &InstrumentConfig,
) {
    match pos.direction {
        TradeDirection::Long | TradeDirection::Both => {
            let adverse = (pos.entry_price - bid) / instrument.pip_size;
            let favorable = (bid - pos.entry_price) / instrument.pip_size;
            if adverse > pos.mae_pips {
                pos.mae_pips = adverse;
            }
            if favorable > pos.mfe_pips {
                pos.mfe_pips = favorable;
            }
        }
        TradeDirection::Short => {
            let adverse = (ask - pos.entry_price) / instrument.pip_size;
            let favorable = (pos.entry_price - ask) / instrument.pip_size;
            if adverse > pos.mae_pips {
                pos.mae_pips = adverse;
            }
            if favorable > pos.mfe_pips {
                pos.mfe_pips = favorable;
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════
// Candle-level MAE/MFE
// ══════════════════════════════════════════════════════════════

/// Update MAE/MFE tracking for an open position.
pub fn update_mae_mfe(
    position: &mut OpenPosition,
    candle: &Candle,
    instrument: &InstrumentConfig,
) {
    match position.direction {
        TradeDirection::Long | TradeDirection::Both => {
            let adverse = (position.entry_price - candle.low) / instrument.pip_size;
            let favorable = (candle.high - position.entry_price) / instrument.pip_size;
            if adverse > position.mae_pips {
                position.mae_pips = adverse;
            }
            if favorable > position.mfe_pips {
                position.mfe_pips = favorable;
            }
        }
        TradeDirection::Short => {
            let adverse = (candle.high - position.entry_price) / instrument.pip_size;
            let favorable = (position.entry_price - candle.low) / instrument.pip_size;
            if adverse > position.mae_pips {
                position.mae_pips = adverse;
            }
            if favorable > position.mfe_pips {
                position.mfe_pips = favorable;
            }
        }
    }
}
