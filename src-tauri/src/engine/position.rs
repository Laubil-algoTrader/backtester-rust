use chrono::NaiveDate;

use crate::engine::orders::BidAskOhlc;
use crate::models::candle::Candle;
use crate::models::config::{InstrumentConfig, SwapMode};
use crate::models::strategy::{
    OrderType, PositionSizing, PositionSizingType, StopLoss, StopLossType, TakeProfit,
    TakeProfitType, TradeDirection, TrailingStop, TrailingStopType,
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
    /// True once the trailing stop has actually moved the stop loss level.
    /// Used to distinguish `CloseReason::TrailingStop` from `CloseReason::StopLoss`.
    pub trailing_stop_activated: bool,
    /// Date on which swap was last charged ("YYYY-MM-DD"). Empty before first charge.
    pub last_swap_date: String,
    /// Cumulative swap charged so far (negative = cost to the trader).
    pub accumulated_swap: f64,
    /// True once the stop loss has been moved to breakeven (avoids double-trigger).
    pub sl_moved_to_be: bool,
}

/// A pending limit or stop entry order waiting to be filled.
#[derive(Debug, Clone)]
pub struct PendingOrder {
    pub direction: TradeDirection,
    pub order_type: OrderType,
    /// Price level at which the order fills.
    pub target_price: f64,
    pub lots: f64,
    pub created_bar: usize,
    /// ATR values at signal time — used to compute SL/TP/TS when the order fills.
    pub atr_for_sl: Option<f64>,
    pub atr_for_tp: Option<f64>,
    pub atr_for_ts: Option<f64>,
}

/// Calculate position size in lots.
///
/// `consecutive_losses` is used only by `AntiMartingale` mode — pass 0 for all other modes.
pub fn calculate_lots(
    sizing: &PositionSizing,
    equity: f64,
    entry_price: f64,
    sl_price: Option<f64>,
    instrument: &InstrumentConfig,
    consecutive_losses: u32,
) -> f64 {
    let raw = match sizing.sizing_type {
        PositionSizingType::FixedLots => sizing.value,
        PositionSizingType::FixedAmount => {
            if let Some(sl) = sl_price {
                let sl_distance_pips = (entry_price - sl).abs() / instrument.pip_size;
                if sl_distance_pips == 0.0 || instrument.pip_value == 0.0 {
                    return instrument.min_lot;
                }
                sizing.value / (sl_distance_pips * instrument.pip_value)
            } else {
                instrument.min_lot
            }
        }
        PositionSizingType::PercentEquity => {
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
        PositionSizingType::AntiMartingale => {
            // Base size uses risk-based sizing, then apply decay: decrease_factor^n_losses.
            // decrease_factor is in (0, 1]: 0.9 = −10% per loss, 1.0 = no decay.
            if let Some(sl) = sl_price {
                let sl_distance_pips = (entry_price - sl).abs() / instrument.pip_size;
                if sl_distance_pips == 0.0 || instrument.pip_value == 0.0 {
                    return instrument.min_lot;
                }
                let risk_amount = equity * sizing.value / 100.0;
                let base_lots = risk_amount / (sl_distance_pips * instrument.pip_value);
                let decay = sizing.decrease_factor.max(0.0).powi(consecutive_losses as i32);
                base_lots * decay
            } else {
                instrument.min_lot
            }
        }
    };

    // Clamp to min_lot and round to min_lot increments
    if raw <= 0.0 || raw.is_nan() {
        tracing::warn!(
            "Position sizing produced invalid raw lots ({:.6}); clamping to min_lot={}. \
             Check SL distance and pip_value configuration.",
            raw, instrument.min_lot
        );
        return instrument.min_lot;
    }
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

    let sl_price = match direction {
        TradeDirection::Long | TradeDirection::Both => entry_price - distance,
        TradeDirection::Short => entry_price + distance,
    };

    // Sanity check: SL must be on the correct side of entry price
    match direction {
        TradeDirection::Long | TradeDirection::Both => {
            if sl_price >= entry_price {
                tracing::warn!(
                    "Stop loss ({:.5}) is at or above entry price ({:.5}) for Long. \
                     Check SL value/ATR configuration. The SL will likely never trigger.",
                    sl_price, entry_price
                );
            }
        }
        TradeDirection::Short => {
            if sl_price <= entry_price {
                tracing::warn!(
                    "Stop loss ({:.5}) is at or below entry price ({:.5}) for Short. \
                     Check SL value/ATR configuration. The SL will likely never trigger.",
                    sl_price, entry_price
                );
            }
        }
    }

    sl_price
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

/// Update the trailing stop for an open position using bid/ask OHLC.
///
/// Long: tracks highest bid price — trailing stop follows bid side.
/// Short: tracks lowest ask price — trailing stop follows ask side.
/// Sets `trailing_stop_activated = true` the first time the stop level is moved.
pub fn update_trailing_stop(position: &mut OpenPosition, ba: &BidAskOhlc) {
    if let Some(distance) = position.trailing_stop_distance {
        match position.direction {
            TradeDirection::Long | TradeDirection::Both => {
                // Track highest bid
                if ba.bid_high > position.highest_since_entry {
                    position.highest_since_entry = ba.bid_high;
                    let new_sl = position.highest_since_entry - distance;
                    let moved = if let Some(ref mut sl) = position.stop_loss {
                        if new_sl > *sl { *sl = new_sl; true } else { false }
                    } else {
                        position.stop_loss = Some(new_sl);
                        true
                    };
                    if moved { position.trailing_stop_activated = true; }
                }
            }
            TradeDirection::Short => {
                // Track lowest ask
                if ba.ask_low < position.lowest_since_entry {
                    position.lowest_since_entry = ba.ask_low;
                    let new_sl = position.lowest_since_entry + distance;
                    let moved = if let Some(ref mut sl) = position.stop_loss {
                        if new_sl < *sl { *sl = new_sl; true } else { false }
                    } else {
                        position.stop_loss = Some(new_sl);
                        true
                    };
                    if moved { position.trailing_stop_activated = true; }
                }
            }
        }
    }
}

/// Check if SL or TP was hit on the current bar using bid/ask OHLC.
///
/// Bid/Ask convention (MT5-matching):
/// - Long exits at **bid**: SL checked against bid_low, TP against bid_high.
/// - Short exits at **ask**: SL checked against ask_high, TP against ask_low.
///
/// Handles three realistic scenarios:
/// 1. **Gap-through SL**: If price opens beyond SL, fill is at the open (bid/ask open).
/// 2. **TP fill**: Fills at the TP level (limit order — always fills at target).
/// 3. **Both hit same bar**: The level closer to the relevant open was hit first.
pub fn check_sl_tp_hit(
    position: &OpenPosition,
    ba: &BidAskOhlc,
) -> Option<(f64, CloseReason)> {
    let sl_result = check_stop_loss(position, ba);
    let tp_result = check_take_profit(position, ba);

    match (sl_result, tp_result) {
        (None, None) => None,
        (Some(sl), None) => Some(sl),
        (None, Some(tp)) => Some(tp),
        (Some((sl_fill, sl_reason)), Some((tp_fill, _))) => {
            // Both triggered on same bar — closer level to the relevant open wins
            let ref_open = match position.direction {
                TradeDirection::Long | TradeDirection::Both => ba.bid_open,
                TradeDirection::Short => ba.ask_open,
            };
            let dist_sl = (ref_open - sl_fill).abs();
            let dist_tp = (ref_open - tp_fill).abs();
            if dist_sl <= dist_tp {
                Some((sl_fill, sl_reason))
            } else {
                Some((tp_fill, CloseReason::TakeProfit))
            }
        }
    }
}

/// Check if stop loss was triggered. SL is a stop-market order.
/// Long: checks bid_low; Short: checks ask_high.
/// Gap-through fills at the relevant open price (worse price for the trader).
/// Returns `CloseReason::TrailingStop` when the trailing stop moved the SL level.
fn check_stop_loss(pos: &OpenPosition, ba: &BidAskOhlc) -> Option<(f64, CloseReason)> {
    let sl = pos.stop_loss?;
    let reason = if pos.trailing_stop_activated {
        CloseReason::TrailingStop
    } else {
        CloseReason::StopLoss
    };
    match pos.direction {
        TradeDirection::Long | TradeDirection::Both => {
            if ba.bid_low <= sl {
                // Gap-through: bid opened already below SL → fill at bid_open (worse)
                let fill = if ba.bid_open <= sl { ba.bid_open } else { sl };
                Some((fill, reason))
            } else {
                None
            }
        }
        TradeDirection::Short => {
            if ba.ask_high >= sl {
                // Gap-through: ask opened already above SL → fill at ask_open (worse)
                let fill = if ba.ask_open >= sl { ba.ask_open } else { sl };
                Some((fill, reason))
            } else {
                None
            }
        }
    }
}

/// Check if take profit was triggered. TP is a limit order — always fills at TP level.
/// Long: checks bid_high; Short: checks ask_low.
fn check_take_profit(pos: &OpenPosition, ba: &BidAskOhlc) -> Option<(f64, CloseReason)> {
    let tp = pos.take_profit?;
    match pos.direction {
        TradeDirection::Long | TradeDirection::Both => {
            if ba.bid_high >= tp {
                Some((tp, CloseReason::TakeProfit))
            } else {
                None
            }
        }
        TradeDirection::Short => {
            if ba.ask_low <= tp {
                Some((tp, CloseReason::TakeProfit))
            } else {
                None
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════
// Swap / overnight financing
// ══════════════════════════════════════════════════════════════

/// Calculate daily swap charge for an open position.
///
/// `multiplier` = 1.0 for normal days, 3.0 on `triple_swap_day` (covers weekend).
/// Returns a signed amount: negative = cost to the trader, positive = credit.
pub fn calculate_swap_charge(
    direction: TradeDirection,
    lots: f64,
    entry_price: f64,
    instrument: &InstrumentConfig,
    multiplier: f64,
) -> f64 {
    let rate = match direction {
        TradeDirection::Long | TradeDirection::Both => instrument.swap_long,
        TradeDirection::Short => instrument.swap_short,
    };
    let daily_swap = match instrument.swap_mode {
        SwapMode::InPips => rate * instrument.pip_value * lots,
        SwapMode::InPoints => {
            // 1 point = tick_size; convert to pip-equivalent value
            if instrument.tick_size > 0.0 {
                rate * (instrument.tick_size / instrument.pip_size) * instrument.pip_value * lots
            } else {
                rate * instrument.pip_value * lots
            }
        }
        SwapMode::InMoney => rate * lots,
        SwapMode::AsPercent => {
            // Annual rate (%) divided by configured annual days (default 365; use 252 for equities)
            let pos_value = entry_price * lots * instrument.lot_size;
            let annual_days = instrument.swap_annual_days.max(1) as f64;
            pos_value * rate / 100.0 / annual_days
        }
    };
    daily_swap * multiplier
}

/// Determine if swap should be charged on this bar.
///
/// Returns `(should_charge, multiplier)` where:
/// - `should_charge` = true when the bar's date has advanced past `last_swap_date`
/// - `multiplier` = 3.0 on `triple_swap_day`, 1.0 otherwise
pub fn should_charge_swap(
    position: &OpenPosition,
    candle_datetime: &str,
    instrument: &InstrumentConfig,
) -> (bool, f64) {
    if candle_datetime.len() < 10 {
        return (false, 1.0);
    }
    let bar_date = &candle_datetime[..10];
    if position.last_swap_date.is_empty() || bar_date > position.last_swap_date.as_str() {
        // Determine weekday to check for triple swap (Mon=1 … Sun=7)
        let multiplier = NaiveDate::parse_from_str(bar_date, "%Y-%m-%d")
            .map(|d| {
                use chrono::Datelike;
                let weekday = d.weekday().number_from_monday() as u8;
                if weekday == instrument.triple_swap_day { 3.0 } else { 1.0 }
            })
            .unwrap_or(1.0);
        (true, multiplier)
    } else {
        (false, 1.0)
    }
}

// ══════════════════════════════════════════════════════════════
// Stops-level enforcement
// ══════════════════════════════════════════════════════════════

/// Clamp a stop loss price to respect the instrument's minimum stop distance.
///
/// `min_stop_distance_pips` mirrors MT5's `SYMBOL_TRADE_STOPS_LEVEL`.
/// For Long positions: SL must be ≥ `min_stop_distance_pips` below entry.
/// For Short positions: SL must be ≥ `min_stop_distance_pips` above entry.
pub fn enforce_stops_level_sl(
    sl_price: f64,
    entry_price: f64,
    direction: TradeDirection,
    instrument: &InstrumentConfig,
) -> f64 {
    if instrument.min_stop_distance_pips <= 0.0 {
        return sl_price;
    }
    let min_dist = instrument.min_stop_distance_pips * instrument.pip_size;
    match direction {
        TradeDirection::Long | TradeDirection::Both => sl_price.min(entry_price - min_dist),
        TradeDirection::Short => sl_price.max(entry_price + min_dist),
    }
}

/// Clamp a take profit price to respect the instrument's minimum stop distance.
pub fn enforce_stops_level_tp(
    tp_price: f64,
    entry_price: f64,
    direction: TradeDirection,
    instrument: &InstrumentConfig,
) -> f64 {
    if instrument.min_stop_distance_pips <= 0.0 {
        return tp_price;
    }
    let min_dist = instrument.min_stop_distance_pips * instrument.pip_size;
    match direction {
        TradeDirection::Long | TradeDirection::Both => tp_price.max(entry_price + min_dist),
        TradeDirection::Short => tp_price.min(entry_price - min_dist),
    }
}

// ══════════════════════════════════════════════════════════════
// MAE/MFE with bid/ask split
// ══════════════════════════════════════════════════════════════

/// Update MAE/MFE using bid/ask OHLC.
/// Long: MAE from bid_low, MFE from bid_high.
/// Short: MAE from ask_high, MFE from ask_low.
pub fn update_mae_mfe_ba(
    position: &mut OpenPosition,
    ba: &BidAskOhlc,
    instrument: &InstrumentConfig,
) {
    match position.direction {
        TradeDirection::Long | TradeDirection::Both => {
            let adverse = (position.entry_price - ba.bid_low) / instrument.pip_size;
            let favorable = (ba.bid_high - position.entry_price) / instrument.pip_size;
            if adverse > position.mae_pips { position.mae_pips = adverse; }
            if favorable > position.mfe_pips { position.mfe_pips = favorable; }
        }
        TradeDirection::Short => {
            let adverse = (ba.ask_high - position.entry_price) / instrument.pip_size;
            let favorable = (position.entry_price - ba.ask_low) / instrument.pip_size;
            if adverse > position.mae_pips { position.mae_pips = adverse; }
            if favorable > position.mfe_pips { position.mfe_pips = favorable; }
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
