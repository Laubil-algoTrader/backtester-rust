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
            if entry_price == 0.0 || instrument.lot_size == 0.0 {
                return instrument.min_lot;
            }
            sizing.value / (entry_price * instrument.lot_size)
        }
        PositionSizingType::PercentEquity => {
            if entry_price == 0.0 || instrument.lot_size == 0.0 {
                return instrument.min_lot;
            }
            (equity * sizing.value / 100.0) / (entry_price * instrument.lot_size)
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
pub fn check_sl_tp_hit(
    position: &OpenPosition,
    candle: &Candle,
) -> Option<(f64, CloseReason)> {
    match position.direction {
        TradeDirection::Long | TradeDirection::Both => {
            // For long: SL hit if low <= SL, TP hit if high >= TP
            if let Some(sl) = position.stop_loss {
                if candle.low <= sl {
                    return Some((sl, CloseReason::StopLoss));
                }
            }
            if let Some(tp) = position.take_profit {
                if candle.high >= tp {
                    return Some((tp, CloseReason::TakeProfit));
                }
            }
        }
        TradeDirection::Short => {
            // For short: SL hit if high >= SL, TP hit if low <= TP
            if let Some(sl) = position.stop_loss {
                if candle.high >= sl {
                    return Some((sl, CloseReason::StopLoss));
                }
            }
            if let Some(tp) = position.take_profit {
                if candle.low <= tp {
                    return Some((tp, CloseReason::TakeProfit));
                }
            }
        }
    }
    None
}

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
