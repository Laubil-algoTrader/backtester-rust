/// SR builder: indicator pre-computation, NSGA-II main loop, CMA-ES refinement.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use rand::SeedableRng;
use rayon::prelude::*;
use tracing::info;

use crate::engine::executor::{
    SubBarData, find_subbar_range, process_subbars_candle, process_subbars_tick_columnar,
};
use crate::engine::indicators::{
    compute_indicator_with_slices, CandleSlices, IndicatorOutput,
};
use crate::engine::metrics::calculate_metrics;
use crate::engine::orders::BidAskOhlc;
use crate::engine::position::{
    calculate_lots, calculate_stop_loss, calculate_take_profit, calculate_trailing_stop_distance,
    check_sl_tp_hit, update_trailing_stop, OpenPosition,
};
use crate::errors::AppError;
use crate::models::candle::Candle;
use crate::models::config::{InstrumentConfig, SwapMode, Timeframe};
use crate::models::result::{BacktestMetrics, BacktestResults, DrawdownPoint, EquityPoint};
use crate::models::sr_result::{
    PoolLeaf, SrConfig, SrFrontItem, SrObjectives, SrProgressEvent, SrStrategy,
};
use crate::models::strategy::{
    BacktestConfig, CloseTradesAt, IndicatorConfig, IndicatorParams, IndicatorType, TradeDirection,
    TradingHours,
};
use crate::models::trade::{CloseReason, TradeResult};

use super::cmaes::optimize_constants;
use super::nsga2::{
    make_offspring, nsga2_select, random_population,
    replace_weakest, top_k_by_fitness,
    SrIndividual,
};
use super::tree::{self, tree_depth, SrCache};

// ── Pool Expansion ────────────────────────────────────────────────────────────

/// Expand pool entries with period ranges into individual concrete entries.
///
/// Each `PoolLeaf` with `period_min/max/step` set is expanded into N leaves —
/// one per period value in `[period_min, period_max]` stepping by `period_step`.
/// Entries without ranges are passed through unchanged.
fn expand_pool(pool: Vec<PoolLeaf>) -> Vec<PoolLeaf> {
    let mut expanded = Vec::new();
    for leaf in pool {
        match (leaf.period_min, leaf.period_max, leaf.period_step) {
            (Some(min), Some(max), Some(step)) if step > 0 && max >= min => {
                let mut p = min;
                while p <= max {
                    let mut new_leaf = leaf.clone();
                    new_leaf.config.params.period = Some(p);
                    new_leaf.period_min = None;
                    new_leaf.period_max = None;
                    new_leaf.period_step = None;
                    expanded.push(new_leaf);
                    p += step;
                }
            }
            _ => expanded.push(leaf),
        }
    }
    expanded
}

// ── Indicator Pre-computation ─────────────────────────────────────────────────

/// Build the shared indicator cache from the SR pool.
/// One entry per unique (indicator_type, params) combination, regardless of buffer_index.
pub fn build_sr_cache_pub(
    pool: &[PoolLeaf],
    candles: &[Candle],
) -> Result<SrCache, AppError> {
    build_sr_cache(pool, candles)
}

/// Pre-compute ATR series for SL/TP (public for external callers).
/// Uses period 14 for backward compatibility; prefer passing the desired period directly.
pub fn build_atr_series_pub(candles: &[Candle]) -> Vec<f64> {
    build_atr_series(candles, 14)
}

fn build_sr_cache(
    pool: &[PoolLeaf],
    candles: &[Candle],
) -> Result<SrCache, AppError> {
    let slices = CandleSlices::from_candles(candles);
    let mut map: HashMap<u64, Arc<IndicatorOutput>> = HashMap::new();

    for leaf in pool {
        let key = leaf.config.cache_key_hash();
        if map.contains_key(&key) {
            continue; // already computed
        }
        match compute_indicator_with_slices(&leaf.config, &slices, candles) {
            Ok(output) => {
                map.insert(key, Arc::new(output));
            }
            Err(e) => {
                tracing::warn!("SR cache: could not compute {:?}: {e}", leaf.config.cache_key());
            }
        }
    }

    Ok(Arc::new(map))
}

/// Pre-compute an ATR series for SL/TP calculation when ATR-based types are used.
/// Uses `atr_period` from the caller (default 14 when not configured).
/// Falls back to zeros on error and logs a warning — callers that use ATR-based
/// stops will effectively have zero stop distance when this happens.
fn build_atr_series(candles: &[Candle], atr_period: usize) -> Vec<f64> {
    let slices = CandleSlices::from_candles(candles);
    let config = IndicatorConfig {
        indicator_type: IndicatorType::ATR,
        params: IndicatorParams { period: Some(atr_period), ..Default::default() },
        output_field: None,
        cached_hash: 0,
    };
    compute_indicator_with_slices(&config, &slices, candles)
        .map(|o| o.primary)
        .unwrap_or_else(|e| {
            tracing::warn!("SR: ATR({atr_period}) computation failed: {e}. ATR-based SL/TP will use zero distance.");
            vec![0.0; candles.len()]
        })
}

// ── Time helpers ─────────────────────────────────────────────────────────────

/// Extract hour and minute from a datetime string "YYYY-MM-DD HH:MM:SS...".
#[inline(always)]
fn extract_hm(datetime: &str) -> (u8, u8) {
    let b = datetime.as_bytes();
    if b.len() >= 16 {
        let h = (b[11] - b'0') * 10 + (b[12] - b'0');
        let m = (b[14] - b'0') * 10 + (b[15] - b'0');
        (h, m)
    } else {
        (0, 0)
    }
}

/// Returns `true` if hour:minute is within the configured trading window.
/// Handles ranges that cross midnight (e.g. 22:00 → 06:00).
#[inline(always)]
fn within_trading_hours(hours: &TradingHours, h: u8, m: u8) -> bool {
    let current = h as u16 * 60 + m as u16;
    let start = hours.start_hour as u16 * 60 + hours.start_minute as u16;
    let end = hours.end_hour as u16 * 60 + hours.end_minute as u16;
    if start <= end { current >= start && current < end } else { current >= start || current < end }
}

/// Returns `true` if the bar's time has reached or passed the force-close time.
#[inline(always)]
fn should_force_close(close_at: &CloseTradesAt, datetime: &str) -> bool {
    let (h, m) = extract_hm(datetime);
    let current = h as u16 * 60 + m as u16;
    let target = close_at.hour as u16 * 60 + close_at.minute as u16;
    current >= target
}

/// Compute overnight swap for a closed position.
///
/// Returns the swap in account currency (negative = cost, positive = credit).
/// Uses `instrument.swap_long/short` and `swap_mode` so the SR backtest matches
/// the classic builder's swap accounting. Returns 0.0 when both rates are zero.
#[inline(always)]
fn compute_swap(
    direction: TradeDirection,
    lots: f64,
    duration_bars: usize,
    timeframe: Timeframe,
    entry_price: f64,
    instrument: &InstrumentConfig,
) -> f64 {
    let rate = match direction {
        TradeDirection::Long | TradeDirection::Both => instrument.swap_long,
        TradeDirection::Short => instrument.swap_short,
    };
    if rate == 0.0 { return 0.0; }
    let minutes = timeframe.minutes() as f64;
    if minutes == 0.0 { return 0.0; }
    let days_held = duration_bars as f64 * minutes / (24.0 * 60.0);
    match instrument.swap_mode {
        SwapMode::InPips => rate * instrument.pip_value * lots * days_held,
        // InPoints: 1 point ≈ 1 tick = tick_size; monetary value = tick_size/pip_size * pip_value
        SwapMode::InPoints => {
            let point_value = if instrument.pip_size > 0.0 {
                instrument.tick_size / instrument.pip_size * instrument.pip_value
            } else { 0.0 };
            rate * point_value * lots * days_held
        }
        SwapMode::InMoney => rate * lots * days_held,
        SwapMode::AsPercent => {
            let position_value = entry_price * instrument.lot_size * lots;
            rate / 100.0 / instrument.swap_annual_days.max(1) as f64 * position_value * days_held
        }
    }
}

// ── Lightweight SR Backtest ───────────────────────────────────────────────────

/// Evaluate an SR strategy on historical candles.
///
/// When `sub_bars` contains M1 candles or tick data, SL/TP resolution uses sub-bar
/// precision (iterating through intra-bar data). Otherwise falls back to bar-level OHLC.
/// Returns `BacktestMetrics` on success, or `None` if the strategy produces no trades.
pub fn sr_backtest(
    strategy: &SrStrategy,
    candles: &[Candle],
    cache: &SrCache,
    atr_series: &[f64],
    instrument: &InstrumentConfig,
    initial_capital: f64,
    timeframe: Timeframe,
    max_trades_per_day: Option<u32>,
    sub_bars: &SubBarData,
) -> Option<BacktestMetrics> {
    let n = candles.len();
    if n < 2 {
        return None;
    }

    let mut equity = initial_capital;
    let mut trades: Vec<TradeResult> = Vec::new();
    let mut equity_curve: Vec<EquityPoint> = Vec::with_capacity(n);
    let mut open: Option<OpenPosition> = None;
    let mut prev_exit_signal: f64 = 0.0;
    let spread_price = strategy.trading_costs.spread_pips * instrument.pip_size;
    let slippage_price = strategy.trading_costs.slippage_pips * instrument.pip_size;
    // Trailing stop activation: convert pips to price distance once
    let ts_activation_dist = strategy.trailing_stop.as_ref()
        .and_then(|ts| ts.activation_pips)
        .map(|pips| pips * instrument.pip_size);

    // Precompute commission helper
    let commission_per_lot = |lots: f64| -> f64 {
        use crate::models::strategy::CommissionType;
        match strategy.trading_costs.commission_type {
            CommissionType::FixedPerLot => strategy.trading_costs.commission_value * lots,
            CommissionType::Percentage => {
                strategy.trading_costs.commission_value / 100.0
                    * lots
                    * instrument.lot_size
            }
        }
    };

    // Trade frequency tracking
    let mut trades_today: u32 = 0;
    let mut last_trade_date = String::new();
    let mut consecutive_losses: u32 = 0;
    // Bar index of the most recently closed position (for cooldown tracking).
    let mut last_exit_bar: Option<usize> = None;
    // Sub-bar cursor for high-precision modes (advances monotonically through sub-bar data)
    let mut sub_cursor: usize = 0;

    for i in 1..n {
        let candle = &candles[i];
        let ba = BidAskOhlc::from_candle(candle, spread_price);
        // Find sub-bar range for this candle (no-op when SubBarData::None)
        let next_dt = if i + 1 < n { candles[i + 1].datetime.as_str() } else { "" };
        let next_ts = if i + 1 < n { candles[i + 1].timestamp } else { i64::MAX };
        let (sub_start, sub_end) = find_subbar_range(
            sub_bars, &mut sub_cursor, &candle.datetime, next_dt,
            candle.timestamp, next_ts,
        );
        // Evaluate signals using the PREVIOUS bar's completed data (i-1),
        // then act at the OPEN of the current bar (i). This matches MT5's
        // CopyBuffer(..., shift=1, ...) behaviour and avoids look-ahead bias.
        let exit_signal = tree::evaluate(&strategy.exit, i - 1, cache);

        // ── Phase 1: manage open position ───────────────────────────────────
        if let Some(ref mut pos) = open {
            let mut closed_price: Option<f64> = None;
            let mut close_reason = CloseReason::Signal;
            let mut _close_time: Option<String> = None;

            // Check SL/TP using sub-bar data when available, otherwise bar-level OHLC.
            match sub_bars {
                SubBarData::Candles(ref subs) if sub_start < sub_end => {
                    if let Some((fill, time, reason)) = process_subbars_candle(
                        pos, subs, sub_start, sub_end, instrument, spread_price,
                    ) {
                        closed_price = Some(fill);
                        close_reason = reason;
                        _close_time = Some(time);
                    }
                }
                SubBarData::Ticks(ref ticks) if sub_start < sub_end => {
                    if let Some((fill, time, reason)) = process_subbars_tick_columnar(
                        pos, ticks, sub_start, sub_end, instrument,
                    ) {
                        closed_price = Some(fill);
                        close_reason = reason;
                        _close_time = Some(time);
                    }
                }
                _ => {
                    // Bar-level OHLC check (SelectedTfOnly)
                    if let Some((fill, reason)) = check_sl_tp_hit(pos, &ba) {
                        closed_price = Some(fill);
                        close_reason = reason;
                    }
                }
            }

            // Check max_bars_open (time-based exit)
            if closed_price.is_none() {
                if let Some(max_bars) = strategy.max_bars_open {
                    if i.saturating_sub(pos.entry_bar) >= max_bars {
                        let fill = match pos.direction {
                            TradeDirection::Short => ba.ask_open,
                            _ => ba.bid_open,
                        };
                        closed_price = Some(fill);
                        close_reason = CloseReason::ExitAfterBars;
                    }
                }
            }

            // Check exit formula sign change, with optional dead zone to prevent whipsaw exits.
            if closed_price.is_none() && strategy.use_exit_formula {
                let dz = strategy.exit_dead_zone;
                let sign_changed = (prev_exit_signal > dz && exit_signal < -dz)
                    || (prev_exit_signal < -dz && exit_signal > dz);
                if sign_changed && i > pos.entry_bar + 1 {
                    // Close at open price of current bar
                    let fill = match pos.direction {
                        TradeDirection::Short => ba.ask_open,
                        _ => ba.bid_open,
                    };
                    closed_price = Some(fill);
                    close_reason = CloseReason::Signal;
                }
            }

            // Check force-close at configured time
            if closed_price.is_none() {
                if let Some(ref ct) = strategy.close_trades_at {
                    if should_force_close(ct, &candle.datetime) {
                        let fill = match pos.direction {
                            TradeDirection::Short => ba.ask_open,
                            _ => ba.bid_open,
                        };
                        closed_price = Some(fill);
                        close_reason = CloseReason::TimeClose;
                    }
                }
            }

            if let Some(exit_price) = closed_price {
                let pos = open.take().unwrap();
                let pnl_pips = match pos.direction {
                    TradeDirection::Long | TradeDirection::Both => {
                        (exit_price - pos.entry_price) / instrument.pip_size
                    }
                    TradeDirection::Short => {
                        (pos.entry_price - exit_price) / instrument.pip_size
                    }
                };
                let commission = commission_per_lot(pos.lots);
                let duration_bars = i.saturating_sub(pos.entry_bar);
                let swap = compute_swap(pos.direction, pos.lots, duration_bars, timeframe, pos.entry_price, instrument);
                let pnl = pnl_pips * instrument.pip_value * pos.lots - commission + swap;
                equity += pnl;
                if pnl >= 0.0 { consecutive_losses = 0; } else { consecutive_losses += 1; }
                last_exit_bar = Some(i);

                trades.push(TradeResult {
                    id: trades.len().to_string(),
                    direction: pos.direction,
                    entry_time: pos.entry_time.clone(),
                    entry_price: pos.entry_price,
                    exit_time: candle.datetime.clone(),
                    exit_price,
                    lots: pos.lots,
                    pnl,
                    pnl_pips,
                    commission,
                    swap,
                    close_reason,
                    duration_bars,
                    duration_time: format!("{}b", duration_bars),
                    mae: pos.mae_pips,
                    mfe: pos.mfe_pips,
                });
            }
        }

        // ── Phase 2: update MAE/MFE and trailing stop of open position ────────
        // When using sub-bar precision, these are already updated inside process_subbars_*.
        if let Some(ref mut pos) = open {
            if matches!(sub_bars, SubBarData::None) || sub_start >= sub_end {
                let (excursion_positive, excursion_negative) = match pos.direction {
                    TradeDirection::Long | TradeDirection::Both => (
                        (ba.bid_high - pos.entry_price) / instrument.pip_size,
                        (pos.entry_price - ba.bid_low) / instrument.pip_size,
                    ),
                    TradeDirection::Short => (
                        (pos.entry_price - ba.ask_low) / instrument.pip_size,
                        (ba.ask_high - pos.entry_price) / instrument.pip_size,
                    ),
                };
                pos.mfe_pips = pos.mfe_pips.max(excursion_positive);
                pos.mae_pips = pos.mae_pips.max(excursion_negative);
                update_trailing_stop(pos, &ba);
            }
        }

        // ── Phase 3: entry evaluation (when no position open) ────────────────
        if open.is_none() {
            // Trade frequency filter: max N entries per calendar day
            let can_enter = if let Some(max_tpd) = max_trades_per_day {
                let bar_date = candle.datetime.get(..10).unwrap_or(&candle.datetime).to_string();
                if bar_date != last_trade_date {
                    last_trade_date = bar_date;
                    trades_today = 0;
                }
                trades_today < max_tpd
            } else {
                true
            };
            // Trading hours filter
            let within_hours = strategy.trading_hours.as_ref()
                .map_or(true, |th| { let (h, m) = extract_hm(&candle.datetime); within_trading_hours(th, h, m) });
            // Cooldown filter: min N bars between a close and the next entry
            let cooldown_ok = match (last_exit_bar, strategy.min_bars_between_trades) {
                (Some(last), Some(cd)) => i.saturating_sub(last) >= cd,
                _ => true,
            };

            if can_enter && within_hours && cooldown_ok {
                let entry_dir = eval_entry(strategy, i - 1, cache, &ba);
                if let Some(direction) = entry_dir {
                    // Check allowed direction
                    let allowed = match strategy.trade_direction {
                        TradeDirection::Long => direction == TradeDirection::Long,
                        TradeDirection::Short => direction == TradeDirection::Short,
                        TradeDirection::Both => true,
                    };
                    if allowed {
                        // Long buys at ASK + slippage, Short sells at BID - slippage.
                        let entry_price = match direction {
                            TradeDirection::Short => ba.bid_open - slippage_price,
                            _ => ba.ask_open + slippage_price,
                        };
                        let atr_val = atr_series.get(i - 1).copied().filter(|v| *v > 0.0);

                        let sl_price = strategy.stop_loss.as_ref().map(|sl| {
                            calculate_stop_loss(sl, entry_price, direction, atr_val, instrument)
                        });
                        let tp_price = strategy.take_profit.as_ref().map(|tp| {
                            calculate_take_profit(
                                tp,
                                entry_price,
                                sl_price,
                                direction,
                                atr_val,
                                instrument,
                            )
                        });
                        let ts_distance = strategy.trailing_stop.as_ref().map(|ts| {
                            calculate_trailing_stop_distance(ts, entry_price, sl_price, atr_val, instrument)
                        });
                        let lots = calculate_lots(
                            &strategy.position_sizing,
                            equity,
                            entry_price,
                            sl_price,
                            instrument,
                            consecutive_losses,
                        );

                        open = Some(OpenPosition {
                            direction,
                            entry_price,
                            entry_bar: i,
                            entry_time: candle.datetime.clone(),
                            lots,
                            stop_loss: sl_price,
                            take_profit: tp_price,
                            trailing_stop_distance: ts_distance,
                            highest_since_entry: entry_price,
                            lowest_since_entry: entry_price,
                            mae_pips: 0.0,
                            mfe_pips: 0.0,
                            trailing_stop_activated: false,
                            last_swap_date: String::new(),
                            accumulated_swap: 0.0,
                            sl_moved_to_be: false,
                            trailing_activation_dist: ts_activation_dist,
                        });
                        trades_today += 1;
                    }
                }
            }
        }

        // ── Phase 4: break-even + trailing activation ───────────────────────
        if let Some(ref mut pos) = open {
            // Break-even: move SL to entry price once profit ≥ SL distance
            if strategy.move_sl_to_be && !pos.sl_moved_to_be {
                if let Some(sl) = pos.stop_loss {
                    let sl_distance = (pos.entry_price - sl).abs();
                    let profit = match pos.direction {
                        TradeDirection::Short => pos.entry_price - ba.ask_low,
                        _ => ba.bid_high - pos.entry_price,
                    };
                    if profit >= sl_distance {
                        pos.stop_loss = Some(pos.entry_price);
                        pos.sl_moved_to_be = true;
                    }
                }
            }
            // Trailing stop activation threshold: don't move TS until MFE exceeds threshold
            if let Some(act_dist) = ts_activation_dist {
                if !pos.trailing_stop_activated && pos.trailing_stop_distance.is_some() {
                    let mfe_price = match pos.direction {
                        TradeDirection::Short => pos.entry_price - ba.ask_low,
                        _ => ba.bid_high - pos.entry_price,
                    };
                    if mfe_price < act_dist {
                        // Not yet activated — prevent trailing stop from moving
                        // by resetting highest/lowest to current price
                        pos.highest_since_entry = ba.bid_high.min(pos.entry_price + act_dist);
                        pos.lowest_since_entry = ba.ask_low.max(pos.entry_price - act_dist);
                    }
                }
            }
        }

        equity_curve.push(EquityPoint { timestamp: candle.datetime.clone(), equity });
        prev_exit_signal = exit_signal;
    }

    // Close any remaining open position at end of data.
    // Apply spread consistently: longs close at bid (close - spread), shorts at ask (close + spread).
    if let Some(pos) = open.take() {
        let last = candles.last().unwrap();
        let exit_price = match pos.direction {
            TradeDirection::Short => last.close + spread_price,
            _ => last.close - spread_price,  // Longs pay spread on exit too
        };
        let pnl_pips = match pos.direction {
            TradeDirection::Long | TradeDirection::Both => {
                (exit_price - pos.entry_price) / instrument.pip_size
            }
            TradeDirection::Short => (pos.entry_price - exit_price) / instrument.pip_size,
        };
        let commission = commission_per_lot(pos.lots);
        let dur = n.saturating_sub(pos.entry_bar);
        let swap = compute_swap(pos.direction, pos.lots, dur, timeframe, pos.entry_price, instrument);
        let pnl = pnl_pips * instrument.pip_value * pos.lots - commission + swap;
        equity += pnl;
        trades.push(TradeResult {
            id: trades.len().to_string(),
            direction: pos.direction,
            entry_time: pos.entry_time,
            entry_price: pos.entry_price,
            exit_time: last.datetime.clone(),
            exit_price,
            lots: pos.lots,
            pnl,
            pnl_pips,
            commission,
            swap,
            close_reason: CloseReason::EndOfData,
            duration_bars: dur,
            duration_time: format!("{}b", dur),
            mae: pos.mae_pips,
            mfe: pos.mfe_pips,
        });
        equity_curve.push(EquityPoint { timestamp: last.datetime.clone(), equity });
    }

    if trades.is_empty() {
        return None;
    }

    Some(calculate_metrics(&trades, &equity_curve, initial_capital, timeframe))
}

/// Like `sr_backtest` but uses `cache_index_offset` to look up indicator/ATR values
/// at their absolute position in the pre-computed cache when `candles` is a slice
/// starting somewhere in the middle of the full dataset (e.g. an OOS segment).
///
/// Callers should pass the global start index of `candles` as `cache_index_offset`.
/// For a regular full-range backtest, `cache_index_offset = 0`.
pub fn sr_backtest_with_offset(
    strategy: &SrStrategy,
    candles: &[Candle],
    cache: &SrCache,
    atr_series: &[f64],
    instrument: &InstrumentConfig,
    initial_capital: f64,
    timeframe: Timeframe,
    max_trades_per_day: Option<u32>,
    cache_index_offset: usize,
    sub_bars: &SubBarData,
) -> Option<BacktestMetrics> {
    let n = candles.len();
    if n < 2 { return None; }

    let mut equity = initial_capital;
    let mut trades: Vec<TradeResult> = Vec::new();
    let mut equity_curve: Vec<EquityPoint> = Vec::with_capacity(n);
    let mut open: Option<OpenPosition> = None;
    let mut prev_exit_signal: f64 = 0.0;
    let spread_price = strategy.trading_costs.spread_pips * instrument.pip_size;
    let slippage_price = strategy.trading_costs.slippage_pips * instrument.pip_size;
    let ts_activation_dist = strategy.trailing_stop.as_ref()
        .and_then(|ts| ts.activation_pips)
        .map(|pips| pips * instrument.pip_size);

    let commission_per_lot = |lots: f64| -> f64 {
        use crate::models::strategy::CommissionType;
        match strategy.trading_costs.commission_type {
            CommissionType::FixedPerLot => strategy.trading_costs.commission_value * lots,
            CommissionType::Percentage => {
                strategy.trading_costs.commission_value / 100.0 * lots * instrument.lot_size
            }
        }
    };

    let mut trades_today: u32 = 0;
    let mut last_trade_date = String::new();
    let mut consecutive_losses: u32 = 0;
    let mut last_exit_bar: Option<usize> = None;
    let mut sub_cursor: usize = 0;

    for i in 1..n {
        let candle = &candles[i];
        let ba = BidAskOhlc::from_candle(candle, spread_price);
        let next_dt = if i + 1 < n { candles[i + 1].datetime.as_str() } else { "" };
        let next_ts = if i + 1 < n { candles[i + 1].timestamp } else { i64::MAX };
        let (sub_start, sub_end) = find_subbar_range(
            sub_bars, &mut sub_cursor, &candle.datetime, next_dt,
            candle.timestamp, next_ts,
        );
        // Use global (offset) index for cache/ATR lookups so the correct pre-computed values
        // are used regardless of where this slice starts in the full candle array.
        let abs_i = i + cache_index_offset;
        let exit_signal = tree::evaluate(&strategy.exit, abs_i - 1, cache);

        if let Some(ref mut pos) = open {
            let mut closed_price: Option<f64> = None;
            let mut close_reason = CloseReason::Signal;

            match sub_bars {
                SubBarData::Candles(ref subs) if sub_start < sub_end => {
                    if let Some((fill, _time, reason)) = process_subbars_candle(
                        pos, subs, sub_start, sub_end, instrument, spread_price,
                    ) {
                        closed_price = Some(fill);
                        close_reason = reason;
                    }
                }
                SubBarData::Ticks(ref ticks) if sub_start < sub_end => {
                    if let Some((fill, _time, reason)) = process_subbars_tick_columnar(
                        pos, ticks, sub_start, sub_end, instrument,
                    ) {
                        closed_price = Some(fill);
                        close_reason = reason;
                    }
                }
                _ => {
                    if let Some((fill, reason)) = check_sl_tp_hit(pos, &ba) {
                        closed_price = Some(fill);
                        close_reason = reason;
                    }
                }
            }

            if closed_price.is_none() {
                if let Some(max_bars) = strategy.max_bars_open {
                    if i.saturating_sub(pos.entry_bar) >= max_bars {
                        let fill = match pos.direction {
                            TradeDirection::Short => ba.ask_open,
                            _ => ba.bid_open,
                        };
                        closed_price = Some(fill);
                        close_reason = CloseReason::ExitAfterBars;
                    }
                }
            }

            if closed_price.is_none() && strategy.use_exit_formula {
                let dz = strategy.exit_dead_zone;
                let sign_changed = (prev_exit_signal > dz && exit_signal < -dz)
                    || (prev_exit_signal < -dz && exit_signal > dz);
                if sign_changed && i > pos.entry_bar + 1 {
                    let fill = match pos.direction {
                        TradeDirection::Short => ba.ask_open,
                        _ => ba.bid_open,
                    };
                    closed_price = Some(fill);
                }
            }

            if closed_price.is_none() {
                if let Some(ref ct) = strategy.close_trades_at {
                    if should_force_close(ct, &candle.datetime) {
                        let fill = match pos.direction {
                            TradeDirection::Short => ba.ask_open,
                            _ => ba.bid_open,
                        };
                        closed_price = Some(fill);
                        close_reason = CloseReason::TimeClose;
                    }
                }
            }

            if let Some(exit_price) = closed_price {
                let pos = open.take().unwrap();
                let pnl_pips = match pos.direction {
                    TradeDirection::Long | TradeDirection::Both => {
                        (exit_price - pos.entry_price) / instrument.pip_size
                    }
                    TradeDirection::Short => (pos.entry_price - exit_price) / instrument.pip_size,
                };
                let commission = commission_per_lot(pos.lots);
                let duration_bars = i.saturating_sub(pos.entry_bar);
                let swap = compute_swap(pos.direction, pos.lots, duration_bars, timeframe, pos.entry_price, instrument);
                let pnl = pnl_pips * instrument.pip_value * pos.lots - commission + swap;
                equity += pnl;
                if pnl >= 0.0 { consecutive_losses = 0; } else { consecutive_losses += 1; }
                last_exit_bar = Some(i);
                trades.push(TradeResult {
                    id: trades.len().to_string(),
                    direction: pos.direction,
                    entry_time: pos.entry_time.clone(),
                    entry_price: pos.entry_price,
                    exit_time: candle.datetime.clone(),
                    exit_price,
                    lots: pos.lots,
                    pnl, pnl_pips, commission, swap,
                    close_reason, duration_bars,
                    duration_time: format!("{}b", duration_bars),
                    mae: pos.mae_pips, mfe: pos.mfe_pips,
                });
            }
        }

        if let Some(ref mut pos) = open {
            if matches!(sub_bars, SubBarData::None) || sub_start >= sub_end {
                let (excursion_positive, excursion_negative) = match pos.direction {
                    TradeDirection::Long | TradeDirection::Both => (
                        (ba.bid_high - pos.entry_price) / instrument.pip_size,
                        (pos.entry_price - ba.bid_low) / instrument.pip_size,
                    ),
                    TradeDirection::Short => (
                        (pos.entry_price - ba.ask_low) / instrument.pip_size,
                        (ba.ask_high - pos.entry_price) / instrument.pip_size,
                    ),
                };
                pos.mfe_pips = pos.mfe_pips.max(excursion_positive);
                pos.mae_pips = pos.mae_pips.max(excursion_negative);
                update_trailing_stop(pos, &ba);
            }
        }

        if open.is_none() {
            let can_enter = if let Some(max_tpd) = max_trades_per_day {
                let bar_date = candle.datetime.get(..10).unwrap_or(&candle.datetime).to_string();
                if bar_date != last_trade_date {
                    last_trade_date = bar_date;
                    trades_today = 0;
                }
                trades_today < max_tpd
            } else { true };
            let within_hours = strategy.trading_hours.as_ref()
                .map_or(true, |th| { let (h, m) = extract_hm(&candle.datetime); within_trading_hours(th, h, m) });
            let cooldown_ok = match (last_exit_bar, strategy.min_bars_between_trades) {
                (Some(last), Some(cd)) => i.saturating_sub(last) >= cd,
                _ => true,
            };

            if can_enter && within_hours && cooldown_ok {
                let entry_dir = eval_entry(strategy, abs_i - 1, cache, &ba);
                if let Some(direction) = entry_dir {
                    let allowed = match strategy.trade_direction {
                        TradeDirection::Long => direction == TradeDirection::Long,
                        TradeDirection::Short => direction == TradeDirection::Short,
                        TradeDirection::Both => true,
                    };
                    if allowed {
                        let entry_price = match direction {
                            TradeDirection::Short => ba.bid_open - slippage_price,
                            _ => ba.ask_open + slippage_price,
                        };
                        let atr_val = atr_series.get(abs_i - 1).copied().filter(|v| *v > 0.0);
                        let sl_price = strategy.stop_loss.as_ref().map(|sl| {
                            calculate_stop_loss(sl, entry_price, direction, atr_val, instrument)
                        });
                        let tp_price = strategy.take_profit.as_ref().map(|tp| {
                            calculate_take_profit(tp, entry_price, sl_price, direction, atr_val, instrument)
                        });
                        let ts_distance = strategy.trailing_stop.as_ref().map(|ts| {
                            calculate_trailing_stop_distance(ts, entry_price, sl_price, atr_val, instrument)
                        });
                        let lots = calculate_lots(&strategy.position_sizing, equity, entry_price, sl_price, instrument, consecutive_losses);
                        open = Some(OpenPosition {
                            direction, entry_price, entry_bar: i,
                            entry_time: candle.datetime.clone(), lots,
                            stop_loss: sl_price, take_profit: tp_price,
                            trailing_stop_distance: ts_distance,
                            highest_since_entry: entry_price,
                            lowest_since_entry: entry_price,
                            mae_pips: 0.0, mfe_pips: 0.0,
                            trailing_stop_activated: false,
                            last_swap_date: String::new(), accumulated_swap: 0.0, sl_moved_to_be: false,
                            trailing_activation_dist: ts_activation_dist,
                        });
                        trades_today += 1;
                    }
                }
            }
        }

        // Break-even + trailing activation
        if let Some(ref mut pos) = open {
            if strategy.move_sl_to_be && !pos.sl_moved_to_be {
                if let Some(sl) = pos.stop_loss {
                    let sl_distance = (pos.entry_price - sl).abs();
                    let profit = match pos.direction {
                        TradeDirection::Short => pos.entry_price - ba.ask_low,
                        _ => ba.bid_high - pos.entry_price,
                    };
                    if profit >= sl_distance {
                        pos.stop_loss = Some(pos.entry_price);
                        pos.sl_moved_to_be = true;
                    }
                }
            }
            if let Some(act_dist) = ts_activation_dist {
                if !pos.trailing_stop_activated && pos.trailing_stop_distance.is_some() {
                    let mfe_price = match pos.direction {
                        TradeDirection::Short => pos.entry_price - ba.ask_low,
                        _ => ba.bid_high - pos.entry_price,
                    };
                    if mfe_price < act_dist {
                        pos.highest_since_entry = ba.bid_high.min(pos.entry_price + act_dist);
                        pos.lowest_since_entry = ba.ask_low.max(pos.entry_price - act_dist);
                    }
                }
            }
        }

        equity_curve.push(EquityPoint { timestamp: candle.datetime.clone(), equity });
        prev_exit_signal = exit_signal;
    }

    if let Some(pos) = open.take() {
        let last = candles.last().unwrap();
        let exit_price = match pos.direction {
            TradeDirection::Short => last.close + spread_price,
            _ => last.close - spread_price,
        };
        let pnl_pips = match pos.direction {
            TradeDirection::Long | TradeDirection::Both => (exit_price - pos.entry_price) / instrument.pip_size,
            TradeDirection::Short => (pos.entry_price - exit_price) / instrument.pip_size,
        };
        let commission = commission_per_lot(pos.lots);
        let dur = n.saturating_sub(pos.entry_bar);
        let swap = compute_swap(pos.direction, pos.lots, dur, timeframe, pos.entry_price, instrument);
        let pnl = pnl_pips * instrument.pip_value * pos.lots - commission + swap;
        equity += pnl;
        trades.push(TradeResult {
            id: trades.len().to_string(),
            direction: pos.direction, entry_time: pos.entry_time,
            entry_price: pos.entry_price, exit_time: last.datetime.clone(),
            exit_price, lots: pos.lots, pnl, pnl_pips, commission, swap,
            close_reason: CloseReason::EndOfData, duration_bars: dur,
            duration_time: format!("{}b", dur), mae: pos.mae_pips, mfe: pos.mfe_pips,
        });
        equity_curve.push(EquityPoint { timestamp: last.datetime.clone(), equity });
    }

    if trades.is_empty() { return None; }
    Some(calculate_metrics(&trades, &equity_curve, initial_capital, timeframe))
}

/// Like `sr_backtest` but returns the full `BacktestResults` including trades,
/// equity curve, drawdown curve, and per-trade returns — for display in the Backtest page.
pub fn sr_backtest_full(
    strategy: &SrStrategy,
    candles: &[Candle],
    cache: &SrCache,
    atr_series: &[f64],
    instrument: &InstrumentConfig,
    initial_capital: f64,
    timeframe: Timeframe,
    bt_config: BacktestConfig,
    sub_bars: &SubBarData,
) -> Option<BacktestResults> {
    let n = candles.len();
    if n < 2 {
        return None;
    }

    let mut equity = initial_capital;
    let mut peak_equity = initial_capital;
    let mut trades: Vec<TradeResult> = Vec::new();
    let mut equity_curve: Vec<EquityPoint> = Vec::with_capacity(n);
    let mut drawdown_curve: Vec<DrawdownPoint> = Vec::with_capacity(n);
    let mut open: Option<OpenPosition> = None;
    let mut prev_exit_signal: f64 = 0.0;
    let spread_price = strategy.trading_costs.spread_pips * instrument.pip_size;
    let slippage_price = strategy.trading_costs.slippage_pips * instrument.pip_size;
    let ts_activation_dist = strategy.trailing_stop.as_ref()
        .and_then(|ts| ts.activation_pips)
        .map(|pips| pips * instrument.pip_size);

    let commission_per_lot = |lots: f64| -> f64 {
        use crate::models::strategy::CommissionType;
        match strategy.trading_costs.commission_type {
            CommissionType::FixedPerLot => strategy.trading_costs.commission_value * lots,
            CommissionType::Percentage => {
                strategy.trading_costs.commission_value / 100.0 * lots * instrument.lot_size
            }
        }
    };

    let mut consecutive_losses: u32 = 0;
    let mut trades_today_full: u32 = 0;
    let mut last_trade_date_full = String::new();
    let mut last_exit_bar_full: Option<usize> = None;
    let mut sub_cursor: usize = 0;

    for i in 1..n {
        let candle = &candles[i];
        let ba = BidAskOhlc::from_candle(candle, spread_price);
        let next_dt = if i + 1 < n { candles[i + 1].datetime.as_str() } else { "" };
        let next_ts = if i + 1 < n { candles[i + 1].timestamp } else { i64::MAX };
        let (sub_start, sub_end) = find_subbar_range(
            sub_bars, &mut sub_cursor, &candle.datetime, next_dt,
            candle.timestamp, next_ts,
        );
        // Use previous bar's (i-1) completed indicator values to decide actions
        // at bar i's open — matches MT5 CopyBuffer(shift=1) behaviour.
        let exit_signal = tree::evaluate(&strategy.exit, i - 1, cache);

        if let Some(ref mut pos) = open {
            let mut closed_price: Option<f64> = None;
            let mut close_reason = CloseReason::Signal;

            match sub_bars {
                SubBarData::Candles(ref subs) if sub_start < sub_end => {
                    if let Some((fill, _time, reason)) = process_subbars_candle(
                        pos, subs, sub_start, sub_end, instrument, spread_price,
                    ) {
                        closed_price = Some(fill);
                        close_reason = reason;
                    }
                }
                SubBarData::Ticks(ref ticks) if sub_start < sub_end => {
                    if let Some((fill, _time, reason)) = process_subbars_tick_columnar(
                        pos, ticks, sub_start, sub_end, instrument,
                    ) {
                        closed_price = Some(fill);
                        close_reason = reason;
                    }
                }
                _ => {
                    if let Some((fill, reason)) = check_sl_tp_hit(pos, &ba) {
                        closed_price = Some(fill);
                        close_reason = reason;
                    }
                }
            }

            if closed_price.is_none() {
                if let Some(max_bars) = strategy.max_bars_open {
                    if i.saturating_sub(pos.entry_bar) >= max_bars {
                        let fill = match pos.direction {
                            TradeDirection::Short => ba.ask_open,
                            _ => ba.bid_open,
                        };
                        closed_price = Some(fill);
                        close_reason = CloseReason::ExitAfterBars;
                    }
                }
            }

            if closed_price.is_none() && strategy.use_exit_formula {
                let dz = strategy.exit_dead_zone;
                let sign_changed = (prev_exit_signal > dz && exit_signal < -dz)
                    || (prev_exit_signal < -dz && exit_signal > dz);
                if sign_changed && i > pos.entry_bar + 1 {
                    let fill = match pos.direction {
                        TradeDirection::Short => ba.ask_open,
                        _ => ba.bid_open,
                    };
                    closed_price = Some(fill);
                }
            }

            // Force-close at configured time
            if closed_price.is_none() {
                if let Some(ref ct) = strategy.close_trades_at {
                    if should_force_close(ct, &candle.datetime) {
                        let fill = match pos.direction {
                            TradeDirection::Short => ba.ask_open,
                            _ => ba.bid_open,
                        };
                        closed_price = Some(fill);
                        close_reason = CloseReason::TimeClose;
                    }
                }
            }

            if let Some(exit_price) = closed_price {
                let pos = open.take().unwrap();
                let pnl_pips = match pos.direction {
                    TradeDirection::Long | TradeDirection::Both => {
                        (exit_price - pos.entry_price) / instrument.pip_size
                    }
                    TradeDirection::Short => {
                        (pos.entry_price - exit_price) / instrument.pip_size
                    }
                };
                let commission = commission_per_lot(pos.lots);
                let dur = i.saturating_sub(pos.entry_bar);
                let swap = compute_swap(pos.direction, pos.lots, dur, timeframe, pos.entry_price, instrument);
                let pnl = pnl_pips * instrument.pip_value * pos.lots - commission + swap;
                equity += pnl;
                if pnl >= 0.0 { consecutive_losses = 0; } else { consecutive_losses += 1; }
                last_exit_bar_full = Some(i);
                trades.push(TradeResult {
                    id: trades.len().to_string(),
                    direction: pos.direction,
                    entry_time: pos.entry_time.clone(),
                    entry_price: pos.entry_price,
                    exit_time: candle.datetime.clone(),
                    exit_price,
                    lots: pos.lots,
                    pnl,
                    pnl_pips,
                    commission,
                    swap,
                    close_reason,
                    duration_bars: dur,
                    duration_time: format!("{}b", dur),
                    mae: pos.mae_pips,
                    mfe: pos.mfe_pips,
                });
            }
        }

        if let Some(ref mut pos) = open {
            if matches!(sub_bars, SubBarData::None) || sub_start >= sub_end {
                let (excursion_positive, excursion_negative) = match pos.direction {
                    TradeDirection::Long | TradeDirection::Both => (
                        (ba.bid_high - pos.entry_price) / instrument.pip_size,
                        (pos.entry_price - ba.bid_low) / instrument.pip_size,
                    ),
                    TradeDirection::Short => (
                        (pos.entry_price - ba.ask_low) / instrument.pip_size,
                        (ba.ask_high - pos.entry_price) / instrument.pip_size,
                    ),
                };
                pos.mfe_pips = pos.mfe_pips.max(excursion_positive);
                pos.mae_pips = pos.mae_pips.max(excursion_negative);
                update_trailing_stop(pos, &ba);
            }
        }

        if open.is_none() {
            // Trade frequency filter — mirrors sr_backtest behaviour so displayed
            // metrics match those used during NSGA-II fitness evaluation.
            let can_enter = if let Some(max_tpd) = strategy.max_trades_per_day {
                let bar_date = candle.datetime.get(..10).unwrap_or(&candle.datetime).to_string();
                if bar_date != last_trade_date_full {
                    last_trade_date_full = bar_date;
                    trades_today_full = 0;
                }
                trades_today_full < max_tpd
            } else {
                true
            };
            let within_hours = strategy.trading_hours.as_ref()
                .map_or(true, |th| { let (h, m) = extract_hm(&candle.datetime); within_trading_hours(th, h, m) });
            let cooldown_ok = match (last_exit_bar_full, strategy.min_bars_between_trades) {
                (Some(last), Some(cd)) => i.saturating_sub(last) >= cd,
                _ => true,
            };
            let entry_dir = if can_enter && within_hours && cooldown_ok { eval_entry(strategy, i - 1, cache, &ba) } else { None };
            if let Some(direction) = entry_dir {
                let allowed = match strategy.trade_direction {
                    TradeDirection::Long => direction == TradeDirection::Long,
                    TradeDirection::Short => direction == TradeDirection::Short,
                    TradeDirection::Both => true,
                };
                if allowed {
                    let entry_price = match direction {
                        TradeDirection::Short => ba.bid_open - slippage_price,
                        _ => ba.ask_open + slippage_price,
                    };
                    let atr_val = atr_series.get(i - 1).copied().filter(|v| *v > 0.0);
                    let sl_price = strategy.stop_loss.as_ref().map(|sl| {
                        calculate_stop_loss(sl, entry_price, direction, atr_val, instrument)
                    });
                    let tp_price = strategy.take_profit.as_ref().map(|tp| {
                        calculate_take_profit(tp, entry_price, sl_price, direction, atr_val, instrument)
                    });
                    let ts_distance = strategy.trailing_stop.as_ref().map(|ts| {
                        calculate_trailing_stop_distance(ts, entry_price, sl_price, atr_val, instrument)
                    });
                    let lots = calculate_lots(&strategy.position_sizing, equity, entry_price, sl_price, instrument, consecutive_losses);
                    open = Some(OpenPosition {
                        direction,
                        entry_price,
                        entry_bar: i,
                        entry_time: candle.datetime.clone(),
                        lots,
                        stop_loss: sl_price,
                        take_profit: tp_price,
                        trailing_stop_distance: ts_distance,
                        highest_since_entry: entry_price,
                        lowest_since_entry: entry_price,
                        mae_pips: 0.0,
                        mfe_pips: 0.0,
                        trailing_stop_activated: false,
                        last_swap_date: String::new(),
                        accumulated_swap: 0.0,
                        sl_moved_to_be: false,
                        trailing_activation_dist: ts_activation_dist,
                    });
                    trades_today_full += 1;
                }
            }
        }

        // Break-even + trailing activation
        if let Some(ref mut pos) = open {
            if strategy.move_sl_to_be && !pos.sl_moved_to_be {
                if let Some(sl) = pos.stop_loss {
                    let sl_distance = (pos.entry_price - sl).abs();
                    let profit = match pos.direction {
                        TradeDirection::Short => pos.entry_price - ba.ask_low,
                        _ => ba.bid_high - pos.entry_price,
                    };
                    if profit >= sl_distance {
                        pos.stop_loss = Some(pos.entry_price);
                        pos.sl_moved_to_be = true;
                    }
                }
            }
            if let Some(act_dist) = ts_activation_dist {
                if !pos.trailing_stop_activated && pos.trailing_stop_distance.is_some() {
                    let mfe_price = match pos.direction {
                        TradeDirection::Short => pos.entry_price - ba.ask_low,
                        _ => ba.bid_high - pos.entry_price,
                    };
                    if mfe_price < act_dist {
                        pos.highest_since_entry = ba.bid_high.min(pos.entry_price + act_dist);
                        pos.lowest_since_entry = ba.ask_low.max(pos.entry_price - act_dist);
                    }
                }
            }
        }

        // Include unrealized P&L of open position (matches MT5 equity curve behaviour).
        let unrealized = if let Some(ref pos) = open {
            let current_price = match pos.direction {
                TradeDirection::Short => candle.close + spread_price,
                _ => candle.close,
            };
            let pnl_pips = match pos.direction {
                TradeDirection::Long | TradeDirection::Both => {
                    (current_price - pos.entry_price) / instrument.pip_size
                }
                TradeDirection::Short => (pos.entry_price - current_price) / instrument.pip_size,
            };
            pnl_pips * instrument.pip_value * pos.lots
        } else {
            0.0
        };
        let current_equity = equity + unrealized;
        if peak_equity < current_equity { peak_equity = current_equity; }
        let dd_pct = if peak_equity > 0.0 { (peak_equity - current_equity) / peak_equity * 100.0 } else { 0.0 };
        equity_curve.push(EquityPoint { timestamp: candle.datetime.clone(), equity: current_equity });
        drawdown_curve.push(DrawdownPoint { timestamp: candle.datetime.clone(), drawdown_pct: dd_pct });
        prev_exit_signal = exit_signal;
    }

    if let Some(pos) = open.take() {
        let last = candles.last().unwrap();
        // Apply spread consistently: longs close at bid (close - spread), shorts at ask (close + spread).
        let exit_price = match pos.direction {
            TradeDirection::Short => last.close + spread_price,
            _ => last.close - spread_price,
        };
        let pnl_pips = match pos.direction {
            TradeDirection::Long | TradeDirection::Both => (exit_price - pos.entry_price) / instrument.pip_size,
            TradeDirection::Short => (pos.entry_price - exit_price) / instrument.pip_size,
        };
        let commission = commission_per_lot(pos.lots);
        let dur = n.saturating_sub(pos.entry_bar);
        let swap = compute_swap(pos.direction, pos.lots, dur, timeframe, pos.entry_price, instrument);
        let pnl = pnl_pips * instrument.pip_value * pos.lots - commission + swap;
        equity += pnl;
        trades.push(TradeResult {
            id: trades.len().to_string(),
            direction: pos.direction,
            entry_time: pos.entry_time,
            entry_price: pos.entry_price,
            exit_time: last.datetime.clone(),
            exit_price,
            lots: pos.lots,
            pnl,
            pnl_pips,
            commission,
            swap,
            close_reason: CloseReason::EndOfData,
            duration_bars: dur,
            duration_time: format!("{}b", dur),
            mae: pos.mae_pips,
            mfe: pos.mfe_pips,
        });
        if peak_equity < equity { peak_equity = equity; }
        let dd_pct = if peak_equity > 0.0 { (peak_equity - equity) / peak_equity * 100.0 } else { 0.0 };
        if let Some(last_eq) = equity_curve.last_mut() { last_eq.equity = equity; }
        if let Some(last_dd) = drawdown_curve.last_mut() { last_dd.drawdown_pct = dd_pct; }
    }

    if trades.is_empty() {
        return None;
    }

    let metrics = calculate_metrics(&trades, &equity_curve, initial_capital, timeframe);
    let returns: Vec<f64> = trades.iter().map(|t| t.pnl).collect();

    Some(BacktestResults { trades, equity_curve, drawdown_curve, returns, metrics, backtest_config: bt_config, long_metrics: None, short_metrics: None, warnings: vec![] })
}

/// Evaluate entry signals for the current bar.
fn eval_entry(
    strategy: &SrStrategy,
    idx: usize,
    cache: &SrCache,
    _ba: &BidAskOhlc,
) -> Option<TradeDirection> {
    let long_signal = tree::evaluate(&strategy.entry_long, idx, cache);
    let short_signal = tree::evaluate(&strategy.entry_short, idx, cache);

    let go_long = long_signal.is_finite() && long_signal > strategy.long_threshold;
    let go_short = short_signal.is_finite() && short_signal < strategy.short_threshold;

    match (go_long, go_short) {
        (true, false) => Some(TradeDirection::Long),
        (false, true) => Some(TradeDirection::Short),
        (true, true) => {
            // Both triggered — prefer long (arbitrary tiebreak)
            Some(TradeDirection::Long)
        }
        _ => None,
    }
}

// ── Fitness Evaluation ────────────────────────────────────────────────────────

pub fn compute_objectives_pub(
    metrics: &BacktestMetrics,
    min_trades: usize,
) -> SrObjectives {
    // Expectancy ratio: gain per unit of average loss (dimensionless).
    // avg_loss is stored as a negative number (losing trades have negative PnL).
    let expectancy_ratio = if metrics.avg_loss.abs() > 1e-10 {
        (metrics.expectancy / metrics.avg_loss.abs()).clamp(-5.0, 5.0)
    } else if metrics.expectancy > 0.0 {
        5.0  // profitable with no losses — cap at max
    } else {
        0.0
    };

    // Graduated penalty for insufficient trades: scales linearly from 0 (0 trades)
    // to 1.0 (min_trades or more). Previously a binary 0.5/1.0 cliff that allowed
    // strategies with exactly min_trades/2 trades to compete equally with 500-trade strategies.
    let trade_penalty = if min_trades == 0 {
        1.0
    } else {
        (metrics.total_trades as f64 / min_trades as f64).min(1.0)
    };

    // temporal_consistency is forced to a floor value when there are very few trades.
    // Use the same graduated threshold as trade_penalty: below min_trades/2 trades it
    // is unreliable, above it we trust the measured value (already clamped for NaN safety).
    let temporal_consistency = if min_trades > 0 && metrics.total_trades < min_trades / 2 {
        -2.0_f64
    } else {
        metrics.temporal_consistency
    }.clamp(-5.0, 5.0);

    SrObjectives {
        sharpe: (metrics.sharpe_ratio * trade_penalty).clamp(-5.0, 10.0),
        profit_factor: (metrics.profit_factor * trade_penalty).clamp(0.0, 10.0),
        temporal_consistency,
        neg_max_drawdown: (-metrics.max_drawdown_pct).clamp(-200.0, 0.0),
        expectancy_ratio: expectancy_ratio * trade_penalty,
        neg_complexity: 0.0, // set by caller with actual node count
    }
}

/// Evaluate a single individual, setting its objectives in-place.
/// Propagates trading_hours and close_trades_at from config into the strategy so
/// these constraints are applied consistently during evolution and saved with the strategy.
fn evaluate_individual(
    ind: &mut SrIndividual,
    candles: &[Candle],
    cache: &SrCache,
    atr_series: &[f64],
    instrument: &InstrumentConfig,
    config: &SrConfig,
    timeframe: Timeframe,
    sub_bars: &SubBarData,
) {
    ind.strategy.trading_hours = config.trading_hours.clone();
    ind.strategy.close_trades_at = config.close_trades_at.clone();
    ind.strategy.max_trades_per_day = config.max_trades_per_day;
    ind.strategy.exit_dead_zone = config.exit_dead_zone;
    ind.strategy.max_bars_open = config.max_bars_open;
    ind.strategy.min_bars_between_trades = config.min_bars_between_trades;
    ind.strategy.move_sl_to_be = config.move_sl_to_be;
    let metrics = sr_backtest(
        &ind.strategy,
        candles,
        cache,
        atr_series,
        instrument,
        config.initial_capital,
        timeframe,
        config.max_trades_per_day,
        sub_bars,
    );
    // neg_complexity as a Pareto objective replaces the old bloat multiplier hack.
    // NSGA-II naturally finds the trade-off between performance and simplicity.
    let node_count = tree::count_nodes(&ind.strategy.entry_long)
        + tree::count_nodes(&ind.strategy.entry_short)
        + tree::count_nodes(&ind.strategy.exit);
    ind.objectives = metrics.as_ref().map(|m| {
        let mut obj = compute_objectives_pub(m, config.min_trades);
        obj.neg_complexity = -(node_count as f64);
        obj
    });
    ind.metrics = metrics;
}

// ── Main SR Builder Loop ──────────────────────────────────────────────────────

/// Run the full SR builder: NSGA-II evolution + CMA-ES constant refinement.
///
/// Emits progress via `emit` callback. Returns the final Pareto front items.
pub fn run_sr_builder(
    config: SrConfig,
    candles: Vec<Candle>,
    instrument: InstrumentConfig,
    timeframe: Timeframe,
    cancel_flag: Arc<AtomicBool>,
    emit: impl Fn(SrProgressEvent) + Send + Sync,
    sub_bars: SubBarData,
) -> Result<Vec<SrFrontItem>, AppError> {
    info!(
        "SR builder: pop={} gen={} pool={}",
        config.population_size,
        config.generations,
        config.pool.len()
    );

    if config.pool.is_empty() {
        return Err(AppError::InvalidConfig("SR pool is empty".into()));
    }

    // ── Expand pool (resolve period ranges into concrete entries) ────────────
    let expanded_pool = expand_pool(config.pool.clone());
    info!(
        "SR pool: {} template entries → {} concrete entries after period expansion",
        config.pool.len(),
        expanded_pool.len()
    );

    // ── Pre-compute indicators (always over full candle range) ───────────────
    // The cache covers all bars so OOS evaluation can reuse it with an index offset.
    let cache = build_sr_cache(&expanded_pool, &candles)?;
    let atr_series = build_atr_series(&candles, config.atr_period);
    let atr_arc = Arc::new(atr_series);

    // ── In-sample / out-of-sample split ─────────────────────────────────────
    // When oos_pct is set, evolution only sees the first (1 - oos_pct) fraction of candles.
    // The OOS portion is reserved for final evaluation in build_front_item.
    let is_candle_end: usize = config.oos_pct
        .filter(|&p| p > 0.0 && p < 1.0)
        .map(|p| ((candles.len() as f64 * (1.0 - p)).max(2.0) as usize).min(candles.len()))
        .unwrap_or(candles.len());
    let training_candles: &[Candle] = &candles[0..is_candle_end];
    if is_candle_end < candles.len() {
        info!(
            "SR OOS split: {} in-sample bars, {} OOS bars ({:.1}%)",
            is_candle_end,
            candles.len() - is_candle_end,
            config.oos_pct.unwrap_or(0.0) * 100.0
        );
    }

    // ── Initial population ───────────────────────────────────────────────────
    // Use StdRng always (seeded from entropy or from a user-supplied seed for reproducibility).
    let mut rng: rand::rngs::StdRng = match config.seed {
        Some(s) => rand::rngs::StdRng::seed_from_u64(s),
        None => rand::rngs::StdRng::from_entropy(),
    };

    // ── Island model setup ───────────────────────────────────────────────────
    // When num_islands == 1 the loop below is identical to the previous behaviour.
    let num_islands = config.num_islands.max(1);
    let island_size = (config.population_size / num_islands).max(4);
    let mut islands: Vec<Vec<SrIndividual>> = (0..num_islands)
        .map(|_| random_population(island_size, &config, &expanded_pool, &mut rng))
        .collect();

    // Evaluate all islands in parallel
    {
        let cache_ref = &cache;
        let atr_ref = &*atr_arc;
        let candles_ref: &[Candle] = training_candles;
        let instrument_ref = &instrument;
        let config_ref = &config;
        let sub_ref = &sub_bars;
        for island in &mut islands {
            island.par_iter_mut().for_each(|ind| {
                evaluate_individual(ind, candles_ref, cache_ref, atr_ref, instrument_ref, config_ref, timeframe, sub_ref);
            });
        }
    }

    // ── Continuous NSGA-II loop — stops when databank is full or cancelled ──
    let mut databank: Vec<SrIndividual> = Vec::new();
    let mut databank_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut gen = 0usize;
    let mut total_evaluated: usize = island_size * num_islands;
    let loop_start = std::time::Instant::now();
    let mut sharpe_history: Vec<f64> = Vec::new();
    let scalar_weights_ref = config.scalar_weights.as_ref();

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        // ── Per-island evolution step ───────────────────────────────────────
        for island in &mut islands {
            // Generate offspring for this island
            let offspring = {
                let mut o = make_offspring(
                    island,
                    &expanded_pool,
                    config.max_depth,
                    config.crossover_rate,
                    config.mutation_rate,
                    config.sl_atr_range.as_ref(),
                    config.tp_atr_range.as_ref(),
                    config.constant_min_exp,
                    config.constant_max_exp,
                    &mut rng,
                );
                // Threshold recalibration: after crossover/mutation changes the tree,
                // the output range may shift dramatically. Evaluate on ~20 bars to
                // compute the median output and use it as the new threshold.
                {
                    let n_ref = 20.min(training_candles.len().saturating_sub(2));
                    if n_ref > 0 {
                        let step = (training_candles.len() - 2) / n_ref;
                        let ref_indices: Vec<usize> = (0..n_ref).map(|k| 1 + k * step).collect();
                        for ind in o.iter_mut() {
                            let mut vals_l: Vec<f64> = ref_indices.iter()
                                .map(|&ri| tree::evaluate(&ind.strategy.entry_long, ri, &cache))
                                .filter(|v| v.is_finite())
                                .collect();
                            if vals_l.len() >= 5 {
                                vals_l.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                                ind.strategy.long_threshold = vals_l[vals_l.len() / 2];
                            }
                            let mut vals_s: Vec<f64> = ref_indices.iter()
                                .map(|&ri| tree::evaluate(&ind.strategy.entry_short, ri, &cache))
                                .filter(|v| v.is_finite())
                                .collect();
                            if vals_s.len() >= 5 {
                                vals_s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                                ind.strategy.short_threshold = vals_s[vals_s.len() / 2];
                            }
                        }
                    }
                }
                // Evaluate offspring in parallel
                {
                    let cache_ref = &cache;
                    let atr_ref = &*atr_arc;
                    let candles_ref = training_candles;
                    let instrument_ref = &instrument;
                    let config_ref = &config;
                    let sub_ref = &sub_bars;
                    o.par_iter_mut().for_each(|ind| {
                        evaluate_individual(ind, candles_ref, cache_ref, atr_ref, instrument_ref, config_ref, timeframe, sub_ref);
                    });
                }
                o
            };
            total_evaluated += offspring.len();
            let mut combined = island.clone();
            combined.extend(offspring);
            *island = nsga2_select(combined, island_size);

            // ── Semantic deduplication ───────────────────────────────────────
            // Evaluate all formulas on a small set of reference bars. Individuals
            // with identical discretized signal vectors are redundant — keep only the
            // one with the best scalar fitness and replace the rest with fresh blood.
            {
                use std::collections::HashMap as DedupMap;
                let n_ref = 30.min(training_candles.len().saturating_sub(2));
                if n_ref > 0 {
                    let step = (training_candles.len() - 2) / n_ref;
                    let ref_indices: Vec<usize> = (0..n_ref).map(|k| 1 + k * step).collect();
                    let mut seen: DedupMap<u64, usize> = DedupMap::new(); // hash → best index
                    let mut to_replace: Vec<usize> = Vec::new();
                    for (idx, ind) in island.iter().enumerate() {
                        // Compute signal hash: evaluate long+short+exit at each ref bar
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        for &ri in &ref_indices {
                            let vl = tree::evaluate(&ind.strategy.entry_long, ri, &cache);
                            let vs = tree::evaluate(&ind.strategy.entry_short, ri, &cache);
                            let ve = tree::evaluate(&ind.strategy.exit, ri, &cache);
                            // Discretize to reduce floating-point noise
                            use std::hash::Hash;
                            ((vl * 1000.0) as i64).hash(&mut hasher);
                            ((vs * 1000.0) as i64).hash(&mut hasher);
                            ((ve * 1000.0) as i64).hash(&mut hasher);
                        }
                        use std::hash::Hasher;
                        let h = hasher.finish();
                        match seen.get(&h) {
                            Some(&prev) => {
                                // Keep the one with better fitness
                                if ind.scalar_fitness() > island[prev].scalar_fitness() {
                                    to_replace.push(prev);
                                    seen.insert(h, idx);
                                } else {
                                    to_replace.push(idx);
                                }
                            }
                            None => { seen.insert(h, idx); }
                        }
                    }
                    // Replace duplicates with fresh random individuals
                    if !to_replace.is_empty() {
                        let fresh = random_population(to_replace.len(), &config, &expanded_pool, &mut rng);
                        for (slot_idx, new_ind) in to_replace.iter().zip(fresh) {
                            island[*slot_idx] = new_ind;
                        }
                        {
                            let cache_ref = &cache;
                            let atr_ref = &*atr_arc;
                            let candles_ref = training_candles;
                            let instrument_ref = &instrument;
                            let config_ref = &config;
                            let sub_ref = &sub_bars;
                            for &slot_idx in &to_replace {
                                evaluate_individual(&mut island[slot_idx], candles_ref, cache_ref, atr_ref, instrument_ref, config_ref, timeframe, sub_ref);
                            }
                        }
                        total_evaluated += to_replace.len();
                    }
                }
            }
        }

        // ── Migration (ring topology) ───────────────────────────────────────
        if num_islands > 1 && config.migration_interval > 0 && gen % config.migration_interval == 0 {
            let k = ((island_size as f64 * config.migration_rate).ceil() as usize).max(1);
            // Collect emigrants first to avoid borrow conflicts
            let emigrants: Vec<Vec<SrIndividual>> = islands.iter()
                .map(|isl| top_k_by_fitness(isl, k))
                .collect();
            for dest_idx in 0..num_islands {
                let src_idx = if dest_idx == 0 { num_islands - 1 } else { dest_idx - 1 };
                replace_weakest(&mut islands[dest_idx], emigrants[src_idx].clone());
            }
        }

        // ── Fresh blood (anti-stagnation) ───────────────────────────────────
        if config.fresh_blood_interval > 0 && gen > 0 && gen % config.fresh_blood_interval == 0 {
            let n_replace = ((island_size as f64 * config.fresh_blood_pct).ceil() as usize).max(1);
            for island in &mut islands {
                let fresh = random_population(n_replace, &config, &expanded_pool, &mut rng);
                // Sort island ascending by fitness so replace_weakest finds the worst
                island.sort_by(|a, b| {
                    a.scalar_fitness()
                        .partial_cmp(&b.scalar_fitness())
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                for (slot, new_ind) in island[0..n_replace].iter_mut().zip(fresh) {
                    *slot = new_ind;
                }
                // Evaluate the fresh individuals in parallel
                {
                    let cache_ref = &cache;
                    let atr_ref = &*atr_arc;
                    let candles_ref = training_candles;
                    let instrument_ref = &instrument;
                    let config_ref = &config;
                    let sub_ref = &sub_bars;
                    island[0..n_replace].par_iter_mut().for_each(|ind| {
                        evaluate_individual(ind, candles_ref, cache_ref, atr_ref, instrument_ref, config_ref, timeframe, sub_ref);
                    });
                }
                total_evaluated += n_replace;
            }
        }

        // ── Combined view of all islands for databank and progress ──────────
        let population: Vec<&SrIndividual> = islands.iter().flatten().collect();

        // Sort combined (by rank+crowding) to collect Pareto front
        let pareto_size = population.iter().filter(|i| i.rank == 0).count();

        for ind in population.iter().filter(|i| i.rank == 0) {
            if ind.objectives.as_ref().map(|o| o.is_valid()).unwrap_or(false) {
                if let Some(obj) = &ind.objectives {
                    if let Some(min_s) = config.initial_min_sharpe {
                        if obj.sharpe < min_s { continue; }
                    }
                    if let Some(min_pf) = config.initial_min_profit_factor {
                        if obj.profit_factor < min_pf { continue; }
                    }
                    if let Some(max_dd) = config.initial_max_drawdown_pct {
                        if -obj.neg_max_drawdown > max_dd { continue; }
                    }
                }
                let fl = tree::format_tree(&ind.strategy.entry_long);
                let fs = tree::format_tree(&ind.strategy.entry_short);
                let fe = tree::format_tree(&ind.strategy.exit);
                let key = format!("{fl}|{fs}|{fe}");
                match databank_index.get(&key).copied() {
                    Some(idx) => {
                        if ind.scalar_fitness() > databank[idx].scalar_fitness() {
                            databank[idx] = (*ind).clone();
                        }
                    }
                    None => {
                        let idx = databank.len();
                        databank_index.insert(key, idx);
                        databank.push((*ind).clone());
                    }
                }
            }
        }

        databank.sort_by(|a, b| {
            b.scalar_fitness()
                .partial_cmp(&a.scalar_fitness())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if databank.len() > config.databank_limit {
            databank.truncate(config.databank_limit);
            databank_index.clear();
            for (i, ind) in databank.iter().enumerate() {
                let fl = tree::format_tree(&ind.strategy.entry_long);
                let fs = tree::format_tree(&ind.strategy.entry_short);
                let fe = tree::format_tree(&ind.strategy.exit);
                databank_index.insert(format!("{fl}|{fs}|{fe}"), i);
            }
        }
        let databank_count = databank.len();
        let elapsed_secs = loop_start.elapsed().as_secs_f64().max(0.001);
        let strategies_per_sec = total_evaluated as f64 / elapsed_secs;
        let front_sharpe = population.iter()
            .filter(|i| i.rank == 0)
            .filter_map(|i| i.objectives.as_ref())
            .map(|o| o.sharpe)
            .fold(f64::NEG_INFINITY, f64::max);

        // ── Diversity metrics ───────────────────────────────────────────────
        let total_depth: usize = population.iter().map(|ind| {
            tree_depth(&ind.strategy.entry_long)
                + tree_depth(&ind.strategy.entry_short)
                + tree_depth(&ind.strategy.exit)
        }).sum();
        let avg_depth = total_depth as f64 / (population.len() as f64 * 3.0).max(1.0);

        let unique_formula_count = {
            let mut seen = std::collections::HashSet::new();
            for ind in &population {
                let fl = tree::format_tree(&ind.strategy.entry_long);
                let fs = tree::format_tree(&ind.strategy.entry_short);
                let fe = tree::format_tree(&ind.strategy.exit);
                seen.insert(format!("{fl}|{fs}|{fe}"));
            }
            seen.len()
        };

        // Mean and std of scalar fitness
        let fitnesses: Vec<f64> = population.iter()
            .filter_map(|ind| ind.objectives.as_ref())
            .map(|obj| obj.scalar_with_weights(scalar_weights_ref))
            .collect();
        let avg_fitness = if fitnesses.is_empty() {
            0.0
        } else {
            fitnesses.iter().sum::<f64>() / fitnesses.len() as f64
        };
        let fitness_std = if fitnesses.len() < 2 {
            0.0
        } else {
            let variance = fitnesses.iter()
                .map(|x| (x - avg_fitness).powi(2))
                .sum::<f64>() / (fitnesses.len() - 1) as f64;
            variance.sqrt()
        };

        // Pareto front crowding diversity (mean of finite crowding distances)
        let pareto_crowding: Vec<f64> = population.iter()
            .filter(|i| i.rank == 0 && i.crowding.is_finite())
            .map(|i| i.crowding)
            .collect();
        let pareto_diversity = if pareto_crowding.is_empty() {
            0.0
        } else {
            pareto_crowding.iter().sum::<f64>() / pareto_crowding.len() as f64
        };

        // Operator usage counts: [Add, Sub, Mul, ProtectedDiv, Sqrt, Abs, Log, Neg]
        let operator_counts = {
            use crate::models::sr_result::{BinaryOpType, UnaryOpType};
            let mut counts = [0usize; 8];
            fn count_ops(node: &crate::models::sr_result::SrNode, counts: &mut [usize; 8]) {
                match node {
                    crate::models::sr_result::SrNode::BinaryOp { op, left, right } => {
                        let idx = match op {
                            BinaryOpType::Add => 0,
                            BinaryOpType::Sub => 1,
                            BinaryOpType::Mul => 2,
                            BinaryOpType::ProtectedDiv => 3,
                        };
                        counts[idx] += 1;
                        count_ops(left, counts);
                        count_ops(right, counts);
                    }
                    crate::models::sr_result::SrNode::UnaryOp { op, child } => {
                        let idx = match op {
                            UnaryOpType::Sqrt => 4,
                            UnaryOpType::Abs  => 5,
                            UnaryOpType::Log  => 6,
                            UnaryOpType::Neg  => 7,
                        };
                        counts[idx] += 1;
                        count_ops(child, counts);
                    }
                    _ => {}
                }
            }
            for ind in &population {
                count_ops(&ind.strategy.entry_long, &mut counts);
                count_ops(&ind.strategy.entry_short, &mut counts);
                count_ops(&ind.strategy.exit, &mut counts);
            }
            counts
        };

        emit(SrProgressEvent::Generation {
            gen,
            total: config.generations,
            pareto_size,
            best_sharpe: front_sharpe,
            databank_count,
            databank_limit: config.databank_limit,
            total_evaluated,
            strategies_per_sec,
            avg_depth,
            unique_formula_count,
            avg_fitness,
            fitness_std,
            pareto_diversity,
            operator_counts,
        });

        // Stagnation detection — smarter: detect relative improvement < 1% over window,
        // not just identical values. On stagnation, inject 30% fresh blood.
        let stagnation_window = config.stagnation_window;
        if stagnation_window > 0 {
            sharpe_history.push(front_sharpe);
            if sharpe_history.len() > stagnation_window {
                sharpe_history.remove(0);
            }
            if sharpe_history.len() == stagnation_window {
                let max_s = sharpe_history.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                let min_s = sharpe_history.iter().cloned().fold(f64::INFINITY, f64::min);
                let relative_improvement = (max_s - min_s) / max_s.abs().max(1.0);
                if relative_improvement < 0.01 {
                    emit(SrProgressEvent::Stagnation {
                        gen,
                        window: stagnation_window,
                        best_sharpe_in_window: max_s,
                    });
                    // Aggressive fresh blood injection on stagnation (30% of each island)
                    let n_inject = (island_size as f64 * 0.30).ceil() as usize;
                    for island in &mut islands {
                        let fresh = random_population(n_inject.min(island.len()), &config, &expanded_pool, &mut rng);
                        island.sort_by(|a, b| a.scalar_fitness().partial_cmp(&b.scalar_fitness()).unwrap_or(std::cmp::Ordering::Equal));
                        let actual_inject = n_inject.min(island.len());
                        for (slot, new_ind) in island[0..actual_inject].iter_mut().zip(fresh) {
                            *slot = new_ind;
                        }
                        {
                            let cache_ref = &cache;
                            let atr_ref = &*atr_arc;
                            let candles_ref = training_candles;
                            let instrument_ref = &instrument;
                            let config_ref = &config;
                            let sub_ref = &sub_bars;
                            island[0..actual_inject].par_iter_mut().for_each(|ind| {
                                evaluate_individual(ind, candles_ref, cache_ref, atr_ref, instrument_ref, config_ref, timeframe, sub_ref);
                            });
                        }
                        total_evaluated += actual_inject;
                    }
                    sharpe_history.clear(); // reset history after injection
                }
            }
        }

        // Stop when databank is full
        if databank_count >= config.databank_limit {
            info!(
                "SR: databank full ({}/{}), ending Phase 1 at gen {}",
                databank_count, config.databank_limit, gen
            );
            break;
        }

        // Safety: max generations limit (0 = no limit, rely on databank_limit)
        if config.generations > 0 && gen + 1 >= config.generations {
            break;
        }

        gen += 1;
    }

    if cancel_flag.load(Ordering::Relaxed) {
        return Ok(vec![]);
    }

    // Pre-compute OOS start index once so all build_front_item calls are consistent.
    let oos_start_opt: Option<usize> = config.oos_pct.and_then(|pct| {
        if pct <= 0.0 || pct >= 1.0 { return None; }
        let start = (candles.len() as f64 * (1.0 - pct)).max(2.0) as usize;
        if start >= candles.len().saturating_sub(1) { None } else { Some(start) }
    });

    // ── Emit NSGA-II front (pre-CMA-ES) for the "builder" databank ──────────
    {
        let nsga_items: Vec<SrFrontItem> = databank
            .par_iter()
            .filter_map(|ind| {
                build_front_item(ind, &candles, &cache, &atr_arc, &instrument, &config, timeframe, oos_start_opt, &sub_bars)
            })
            .collect();
        emit(SrProgressEvent::NsgaDone { front: nsga_items });
    }

    // ── CMA-ES constant refinement on top-K databank strategies ─────────────
    let top_k = config.cmaes_top_k.min(databank.len());
    let mut refined: Vec<SrIndividual> = databank.iter().take(top_k).cloned().collect();

    let completed = AtomicUsize::new(0);
    let improved = AtomicUsize::new(0);

    refined.par_iter_mut().for_each(|ind| {
        if cancel_flag.load(Ordering::Relaxed) {
            return;
        }
        let before = ind.scalar_fitness();
        let new_ind = optimize_constants(
            ind,
            &candles,
            &cache,
            &atr_arc,
            &instrument,
            &config,
            timeframe,
            &cancel_flag,
            &sub_bars,
        );
        if new_ind.scalar_fitness() > before {
            *ind = new_ind;
            improved.fetch_add(1, Ordering::Relaxed);
        }
        let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
        emit(SrProgressEvent::CmaesProgress { current: done, total: top_k });
    });

    let improved_count = improved.load(Ordering::Relaxed);
    emit(SrProgressEvent::CmaesComplete { improved_count });

    // ── Build final front items (parallel) ──────────────────────────────────
    // refined items are listed first so they win dedup over unrefined databank copies.
    let all_candidates: Vec<&SrIndividual> = refined.iter()
        .chain(databank.iter())
        .collect();

    // Run full backtests in parallel (each gets complete BacktestMetrics).
    let all_built: Vec<Option<SrFrontItem>> = all_candidates
        .par_iter()
        .map(|ind| build_front_item(ind, &candles, &cache, &atr_arc, &instrument, &config, timeframe, oos_start_opt, &sub_bars))
        .collect();

    // Dedup by formula key sequentially (refined items win because they come first).
    let mut seen_formulas = std::collections::HashSet::new();
    let mut final_items: Vec<SrFrontItem> = Vec::new();
    for maybe_item in all_built {
        if let Some(item) = maybe_item {
            let key = format!("{}|{}|{}", item.formula_long, item.formula_short, item.formula_exit);
            if seen_formulas.insert(key) {
                final_items.push(item);
            }
        }
    }

    // Sort by scalar fitness descending
    final_items.sort_by(|a, b| {
        b.objectives.scalar().partial_cmp(&a.objectives.scalar()).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Apply final filters to the Pareto front
    if config.final_min_sharpe.is_some()
        || config.final_min_profit_factor.is_some()
        || config.final_min_trades.is_some()
        || config.final_max_drawdown_pct.is_some()
    {
        final_items.retain(|item| {
            if let Some(min_s) = config.final_min_sharpe {
                if item.objectives.sharpe < min_s { return false; }
            }
            if let Some(min_pf) = config.final_min_profit_factor {
                if item.objectives.profit_factor < min_pf { return false; }
            }
            if let Some(min_t) = config.final_min_trades {
                if item.metrics.total_trades < min_t { return false; }
            }
            if let Some(max_dd) = config.final_max_drawdown_pct {
                if item.metrics.max_drawdown_pct > max_dd { return false; }
            }
            true
        });
        info!("SR builder: {} items after final filters", final_items.len());
    }

    emit(SrProgressEvent::Done { front: final_items.clone() });
    info!("SR builder done: {} items from {} databank strategies", final_items.len(), databank.len());
    Ok(final_items)
}

/// Build a single `SrFrontItem` from an individual.
/// Reuses the cached `BacktestMetrics` stored in `ind.metrics` (populated by
/// `evaluate_individual`) to avoid re-running the backtest a second time.
///
/// `oos_start`: when `Some(n)`, evaluates the strategy on `candles[n..]` (with the same
/// pre-computed indicator cache — valid because the cache covers all candle indices).
fn build_front_item(
    ind: &SrIndividual,
    candles: &[Candle],
    cache: &SrCache,
    atr_series: &Arc<Vec<f64>>,
    instrument: &InstrumentConfig,
    config: &SrConfig,
    timeframe: Timeframe,
    oos_start: Option<usize>,
    sub_bars: &SubBarData,
) -> Option<SrFrontItem> {
    let obj = ind.objectives.as_ref()?;
    if !obj.is_valid() { return None; }

    let fl = tree::format_tree(&ind.strategy.entry_long);
    let fs = tree::format_tree(&ind.strategy.entry_short);
    let fe = tree::format_tree(&ind.strategy.exit);

    // Prefer cached metrics; fall back to re-running only if cache is missing
    // (e.g. for individuals loaded from a checkpoint without cached metrics).
    let metrics = if let Some(m) = ind.metrics.clone() {
        m
    } else {
        sr_backtest(
            &ind.strategy,
            candles,
            cache,
            atr_series,
            instrument,
            config.initial_capital,
            timeframe,
            config.max_trades_per_day,
            sub_bars,
        )?
    };

    // Out-of-sample evaluation: run backtest on the reserved OOS candle range.
    // The indicator cache covers the full candle array, so the OOS slice can use the
    // same cache by offsetting the bar index lookup by `oos_start`.
    let oos_metrics = oos_start.and_then(|start| {
        let oos_candles = candles.get(start..)?;
        if oos_candles.len() < 2 { return None; }
        sr_backtest_with_offset(
            &ind.strategy,
            oos_candles,
            cache,
            atr_series,
            instrument,
            config.initial_capital,
            timeframe,
            config.max_trades_per_day,
            start,
            sub_bars,
        )
    });

    Some(SrFrontItem {
        rank: ind.rank,
        crowding_distance: ind.crowding,
        objectives: compute_objectives_pub(&metrics, config.min_trades),
        metrics,
        formula_long: fl,
        formula_short: fs,
        formula_exit: fe,
        strategy: ind.strategy.clone(),
        oos_metrics,
    })
}
