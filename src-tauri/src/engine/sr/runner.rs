/// SR builder: indicator pre-computation, NSGA-II main loop, CMA-ES refinement.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use rayon::prelude::*;
use tracing::info;

use crate::engine::indicators::{
    compute_indicator_with_slices, CandleSlices, IndicatorOutput,
};
use crate::engine::metrics::calculate_metrics;
use crate::engine::orders::BidAskOhlc;
use crate::engine::position::{
    calculate_lots, calculate_stop_loss, calculate_take_profit, check_sl_tp_hit, OpenPosition,
};
use crate::errors::AppError;
use crate::models::candle::Candle;
use crate::models::config::{InstrumentConfig, Timeframe};
use crate::models::result::{BacktestMetrics, BacktestResults, DrawdownPoint, EquityPoint};
use crate::models::sr_result::{
    PoolLeaf, SrConfig, SrFrontItem, SrObjectives, SrProgressEvent, SrStrategy,
};
use crate::models::strategy::{
    BacktestConfig, IndicatorConfig, IndicatorParams, IndicatorType, TradeDirection,
};
use crate::models::trade::{CloseReason, TradeResult};

use super::cmaes::optimize_constants;
use super::nsga2::{
    best_front_sharpe, make_offspring, nsga2_select, random_population,
    SrIndividual,
};
use super::tree::{self, SrCache};

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

/// Pre-compute ATR-14 series for SL/TP (public for external callers).
pub fn build_atr_series_pub(candles: &[Candle]) -> Vec<f64> {
    build_atr_series(candles)
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

/// Pre-compute an ATR series (period 14) for SL/TP calculation when ATR-based types are used.
fn build_atr_series(candles: &[Candle]) -> Vec<f64> {
    let slices = CandleSlices::from_candles(candles);
    let config = IndicatorConfig {
        indicator_type: IndicatorType::ATR,
        params: IndicatorParams { period: Some(14), ..Default::default() },
        output_field: None,
        cached_hash: 0,
    };
    compute_indicator_with_slices(&config, &slices, candles)
        .map(|o| o.primary)
        .unwrap_or_else(|_| vec![0.0; candles.len()])
}

// ── Lightweight SR Backtest ───────────────────────────────────────────────────

/// Evaluate an SR strategy on historical candles.
///
/// Uses simplified SelectedTfOnly execution (no sub-bar precision).
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

    for i in 1..n {
        let candle = &candles[i];
        let ba = BidAskOhlc::from_candle(candle, spread_price);
        // Evaluate signals using the PREVIOUS bar's completed data (i-1),
        // then act at the OPEN of the current bar (i). This matches MT5's
        // CopyBuffer(..., shift=1, ...) behaviour and avoids look-ahead bias.
        let exit_signal = tree::evaluate(&strategy.exit, i - 1, cache);

        // ── Phase 1: manage open position ───────────────────────────────────
        if let Some(ref pos) = open {
            let mut closed_price: Option<f64> = None;
            let mut close_reason = CloseReason::Signal;

            // Check SL/TP
            if let Some((fill, reason)) = check_sl_tp_hit(pos, &ba) {
                closed_price = Some(fill);
                close_reason = reason;
            }

            // Check exit formula sign change
            if closed_price.is_none() {
                let sign_changed = (prev_exit_signal >= 0.0 && exit_signal < 0.0)
                    || (prev_exit_signal <= 0.0 && exit_signal > 0.0);
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
                let pnl = pnl_pips * instrument.pip_value * pos.lots - commission;
                equity += pnl;

                let duration_bars = i.saturating_sub(pos.entry_bar);
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
                    swap: 0.0,
                    close_reason,
                    duration_bars,
                    duration_time: format!("{}b", duration_bars),
                    mae: pos.mae_pips,
                    mfe: pos.mfe_pips,
                });
            }
        }

        // ── Phase 2: update MAE/MFE of open position ─────────────────────────
        if let Some(ref mut pos) = open {
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

            if can_enter {
                let entry_dir = eval_entry(strategy, i - 1, cache, &ba);
                if let Some(direction) = entry_dir {
                    // Check allowed direction
                    let allowed = match strategy.trade_direction {
                        TradeDirection::Long => direction == TradeDirection::Long,
                        TradeDirection::Short => direction == TradeDirection::Short,
                        TradeDirection::Both => true,
                    };
                    if allowed {
                        // Long buys at ASK, Short sells at BID — matches MT5 EA behaviour.
                        let entry_price = match direction {
                            TradeDirection::Short => ba.bid_open,
                            _ => ba.ask_open,
                        };
                        let atr_val = atr_series.get(i).copied().filter(|v| *v > 0.0);

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
                        let lots = calculate_lots(
                            &strategy.position_sizing,
                            equity,
                            entry_price,
                            sl_price,
                            instrument,
                            0,
                        );

                        let commission = commission_per_lot(lots);
                        equity -= commission;

                        open = Some(OpenPosition {
                            direction,
                            entry_price,
                            entry_bar: i,
                            entry_time: candle.datetime.clone(),
                            lots,
                            stop_loss: sl_price,
                            take_profit: tp_price,
                            trailing_stop_distance: None,
                            highest_since_entry: candle.high,
                            lowest_since_entry: candle.low,
                            mae_pips: 0.0,
                            mfe_pips: 0.0,
                            trailing_stop_activated: false,
                            last_swap_date: String::new(),
                            accumulated_swap: 0.0,
                            sl_moved_to_be: false,
                        });
                        trades_today += 1;
                    }
                }
            }
        }

        equity_curve.push(EquityPoint { timestamp: candle.datetime.clone(), equity });
        prev_exit_signal = exit_signal;
    }

    // Close any remaining open position at end of data
    if let Some(pos) = open.take() {
        let last = candles.last().unwrap();
        let exit_price = match pos.direction {
            TradeDirection::Short => last.close + spread_price,
            _ => last.close,
        };
        let pnl_pips = match pos.direction {
            TradeDirection::Long | TradeDirection::Both => {
                (exit_price - pos.entry_price) / instrument.pip_size
            }
            TradeDirection::Short => (pos.entry_price - exit_price) / instrument.pip_size,
        };
        let commission = commission_per_lot(pos.lots);
        let pnl = pnl_pips * instrument.pip_value * pos.lots - commission;
        equity += pnl;
        let dur = n.saturating_sub(pos.entry_bar);
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
            swap: 0.0,
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

    let commission_per_lot = |lots: f64| -> f64 {
        use crate::models::strategy::CommissionType;
        match strategy.trading_costs.commission_type {
            CommissionType::FixedPerLot => strategy.trading_costs.commission_value * lots,
            CommissionType::Percentage => {
                strategy.trading_costs.commission_value / 100.0 * lots * instrument.lot_size
            }
        }
    };

    for i in 1..n {
        let candle = &candles[i];
        let ba = BidAskOhlc::from_candle(candle, spread_price);
        // Use previous bar's (i-1) completed indicator values to decide actions
        // at bar i's open — matches MT5 CopyBuffer(shift=1) behaviour.
        let exit_signal = tree::evaluate(&strategy.exit, i - 1, cache);

        if let Some(ref pos) = open {
            let mut closed_price: Option<f64> = None;
            let mut close_reason = CloseReason::Signal;

            if let Some((fill, reason)) = check_sl_tp_hit(pos, &ba) {
                closed_price = Some(fill);
                close_reason = reason;
            }

            if closed_price.is_none() {
                let sign_changed = (prev_exit_signal >= 0.0 && exit_signal < 0.0)
                    || (prev_exit_signal <= 0.0 && exit_signal > 0.0);
                if sign_changed && i > pos.entry_bar + 1 {
                    let fill = match pos.direction {
                        TradeDirection::Short => ba.ask_open,
                        _ => ba.bid_open,
                    };
                    closed_price = Some(fill);
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
                let pnl = pnl_pips * instrument.pip_value * pos.lots - commission;
                equity += pnl;
                let dur = i.saturating_sub(pos.entry_bar);
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
                    swap: 0.0,
                    close_reason,
                    duration_bars: dur,
                    duration_time: format!("{}b", dur),
                    mae: pos.mae_pips,
                    mfe: pos.mfe_pips,
                });
            }
        }

        if let Some(ref mut pos) = open {
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
        }

        if open.is_none() {
            let entry_dir = eval_entry(strategy, i - 1, cache, &ba);
            if let Some(direction) = entry_dir {
                let allowed = match strategy.trade_direction {
                    TradeDirection::Long => direction == TradeDirection::Long,
                    TradeDirection::Short => direction == TradeDirection::Short,
                    TradeDirection::Both => true,
                };
                if allowed {
                    // Long buys at ASK, Short sells at BID — matches MT5 EA behaviour.
                    let entry_price = match direction {
                        TradeDirection::Short => ba.bid_open,
                        _ => ba.ask_open,
                    };
                    let atr_val = atr_series.get(i).copied().filter(|v| *v > 0.0);
                    let sl_price = strategy.stop_loss.as_ref().map(|sl| {
                        calculate_stop_loss(sl, entry_price, direction, atr_val, instrument)
                    });
                    let tp_price = strategy.take_profit.as_ref().map(|tp| {
                        calculate_take_profit(tp, entry_price, sl_price, direction, atr_val, instrument)
                    });
                    let lots = calculate_lots(&strategy.position_sizing, equity, entry_price, sl_price, instrument, 0);
                    let commission = commission_per_lot(lots);
                    equity -= commission;
                    open = Some(OpenPosition {
                        direction,
                        entry_price,
                        entry_bar: i,
                        entry_time: candle.datetime.clone(),
                        lots,
                        stop_loss: sl_price,
                        take_profit: tp_price,
                        trailing_stop_distance: None,
                        highest_since_entry: candle.high,
                        lowest_since_entry: candle.low,
                        mae_pips: 0.0,
                        mfe_pips: 0.0,
                        trailing_stop_activated: false,
                        last_swap_date: String::new(),
                        accumulated_swap: 0.0,
                        sl_moved_to_be: false,
                    });
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
        let exit_price = match pos.direction {
            TradeDirection::Short => last.close + spread_price,
            _ => last.close,
        };
        let pnl_pips = match pos.direction {
            TradeDirection::Long | TradeDirection::Both => (exit_price - pos.entry_price) / instrument.pip_size,
            TradeDirection::Short => (pos.entry_price - exit_price) / instrument.pip_size,
        };
        let commission = commission_per_lot(pos.lots);
        let pnl = pnl_pips * instrument.pip_value * pos.lots - commission;
        equity += pnl;
        let dur = n.saturating_sub(pos.entry_bar);
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
            swap: 0.0,
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

    Some(BacktestResults { trades, equity_curve, drawdown_curve, returns, metrics, backtest_config: bt_config })
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

    // Apply a soft penalty when there are very few trades (less than half min_trades).
    // temporal_consistency will already be low for such strategies, but we also
    // cap the other objectives to prevent degenerate cases from polluting the front.
    let trade_penalty = if metrics.total_trades < min_trades / 2 { 0.5 } else { 1.0 };

    SrObjectives {
        sharpe: (metrics.sharpe_ratio * trade_penalty).max(-5.0).min(10.0),
        profit_factor: (metrics.profit_factor * trade_penalty).max(0.0).min(10.0),
        temporal_consistency: if metrics.total_trades < min_trades / 2 {
            -2.0
        } else {
            metrics.temporal_consistency
        },
        neg_max_drawdown: -metrics.max_drawdown_pct,
        expectancy_ratio: expectancy_ratio * trade_penalty,
    }
}

/// Evaluate a single individual, setting its objectives in-place.
fn evaluate_individual(
    ind: &mut SrIndividual,
    candles: &[Candle],
    cache: &SrCache,
    atr_series: &[f64],
    instrument: &InstrumentConfig,
    config: &SrConfig,
    timeframe: Timeframe,
) {
    ind.objectives = sr_backtest(
        &ind.strategy,
        candles,
        cache,
        atr_series,
        instrument,
        config.initial_capital,
        timeframe,
        config.max_trades_per_day,
    )
    .map(|m| compute_objectives_pub(&m, config.min_trades));
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

    // ── Pre-compute indicators ───────────────────────────────────────────────
    let cache = build_sr_cache(&expanded_pool, &candles)?;
    let atr_series = build_atr_series(&candles);
    let atr_arc = Arc::new(atr_series);

    // ── Initial population ───────────────────────────────────────────────────
    let mut rng = rand::thread_rng();
    let mut population = random_population(config.population_size, &config, &expanded_pool, &mut rng);

    // Evaluate initial population
    {
        let cache_ref = &cache;
        let atr_ref = &*atr_arc;
        let candles_ref = &candles;
        let instrument_ref = &instrument;
        let config_ref = &config;
        population.par_iter_mut().for_each(|ind| {
            evaluate_individual(
                ind,
                candles_ref,
                cache_ref,
                atr_ref,
                instrument_ref,
                config_ref,
                timeframe,
            );
        });
    }

    // ── Continuous NSGA-II loop — stops when databank is full or cancelled ──
    let mut databank: Vec<SrIndividual> = Vec::new();
    let mut seen_databank: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut gen = 0usize;

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }
        // Sort current population (assigns rank and crowding distance)
        population = nsga2_select(population, config.population_size);

        // Collect rank-0 individuals with valid objectives into databank
        let pareto_size = population.iter().filter(|i| i.rank == 0).count();
        for ind in population.iter().filter(|i| i.rank == 0) {
            if ind.objectives.as_ref().map(|o| o.is_valid()).unwrap_or(false) {
                let fl = tree::format_tree(&ind.strategy.entry_long);
                let fs = tree::format_tree(&ind.strategy.entry_short);
                let fe = tree::format_tree(&ind.strategy.exit);
                let key = format!("{fl}|{fs}|{fe}");
                if seen_databank.insert(key) {
                    databank.push(ind.clone());
                }
            }
        }

        // Keep databank sorted by fitness, trimmed to limit
        databank.sort_by(|a, b| {
            b.scalar_fitness()
                .partial_cmp(&a.scalar_fitness())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        databank.truncate(config.databank_limit);
        let databank_count = databank.len();

        let front_sharpe = best_front_sharpe(&population);
        emit(SrProgressEvent::Generation {
            gen,
            total: config.generations,
            pareto_size,
            best_sharpe: front_sharpe,
            databank_count,
            databank_limit: config.databank_limit,
        });

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

        // Generate offspring
        let mut offspring = {
            let mut rng = rand::thread_rng();
            make_offspring(
                &population,
                &expanded_pool,
                config.max_depth,
                config.crossover_rate,
                config.mutation_rate,
                &mut rng,
            )
        };

        // Evaluate offspring in parallel
        {
            let cache_ref = &cache;
            let atr_ref = &*atr_arc;
            let candles_ref = &candles;
            let instrument_ref = &instrument;
            let config_ref = &config;
            offspring.par_iter_mut().for_each(|ind| {
                evaluate_individual(
                    ind,
                    candles_ref,
                    cache_ref,
                    atr_ref,
                    instrument_ref,
                    config_ref,
                    timeframe,
                );
            });
        }

        // Combine parent + offspring, select survivors
        let mut combined = population;
        combined.extend(offspring);
        population = nsga2_select(combined, config.population_size);
        gen += 1;
    }

    if cancel_flag.load(Ordering::Relaxed) {
        return Ok(vec![]);
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
        .map(|ind| build_front_item(ind, &candles, &cache, &atr_arc, &instrument, &config, timeframe))
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

    emit(SrProgressEvent::Done { front: final_items.clone() });
    info!("SR builder done: {} items from {} databank strategies", final_items.len(), databank.len());
    Ok(final_items)
}

/// Build a single `SrFrontItem` from an individual — runs a full backtest to
/// get complete `BacktestMetrics`. Returns `None` if objectives are invalid or
/// the strategy produces no trades.
fn build_front_item(
    ind: &SrIndividual,
    candles: &[Candle],
    cache: &SrCache,
    atr_series: &Arc<Vec<f64>>,
    instrument: &InstrumentConfig,
    config: &SrConfig,
    timeframe: Timeframe,
) -> Option<SrFrontItem> {
    let obj = ind.objectives.as_ref()?;
    if !obj.is_valid() { return None; }

    let fl = tree::format_tree(&ind.strategy.entry_long);
    let fs = tree::format_tree(&ind.strategy.entry_short);
    let fe = tree::format_tree(&ind.strategy.exit);

    let metrics = sr_backtest(
        &ind.strategy,
        candles,
        cache,
        atr_series,
        instrument,
        config.initial_capital,
        timeframe,
        config.max_trades_per_day,
    )?;

    Some(SrFrontItem {
        rank: ind.rank,
        crowding_distance: ind.crowding,
        objectives: compute_objectives_pub(&metrics, config.min_trades),
        metrics,
        formula_long: fl,
        formula_short: fs,
        formula_exit: fe,
        strategy: ind.strategy.clone(),
    })
}
