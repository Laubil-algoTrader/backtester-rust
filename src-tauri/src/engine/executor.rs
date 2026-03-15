use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Datelike;
use polars::prelude::*;
use tracing::info;

use crate::errors::AppError;
use crate::models::candle::{Candle, TickColumns};
use crate::models::config::InstrumentConfig;
use crate::models::result::{BacktestResults, DrawdownPoint, EquityPoint};
use crate::models::strategy::{
    BacktestConfig, CloseTradesAt, IndicatorConfig, IndicatorType, OrderType, Strategy,
    TradeDirection, TradingHours,
};
use crate::models::trade::{CloseReason, TradeResult};

use super::metrics::calculate_metrics;
use super::orders;
use super::position::{
    calculate_lots, calculate_stop_loss, calculate_take_profit,
    calculate_trailing_stop_distance, calculate_swap_charge, check_sl_tp_hit,
    enforce_stops_level_sl, enforce_stops_level_tp,
    should_charge_swap, update_mae_mfe_ba, update_trailing_stop,
    OpenPosition, PendingOrder,
};
use super::strategy::{compile_rules_streaming, compute_candle_pattern_cache, compute_daily_ohlc, compute_time_cache, evaluate_rule_groups, evaluate_rules, evaluate_rules_fast, init_strategy_hashes, max_lookback, pre_compute_indicators, pre_compute_indicators_with_shared_cache, precompute_cross_prev_vals, strategy_uses_candle_patterns, strategy_uses_time_fields};
use super::strategy::IndicatorCache;
use super::streaming;

// ══════════════════════════════════════════════════════════════
// Sub-bar data types
// ══════════════════════════════════════════════════════════════

/// Sub-bar data for high-precision backtest modes.
/// Loaded once and shared across all backtest runs in optimization.
pub enum SubBarData {
    /// No sub-bar data — SelectedTfOnly mode.
    None,
    /// M1 candles or tick-as-OHLCV candles for sub-bar resolution.
    Candles(Vec<Candle>),
    /// Columnar tick data (SoA layout) with i64 timestamps for tick-level resolution.
    /// Uses struct-of-arrays layout for better cache locality and no per-tick allocations.
    Ticks(TickColumns),
}

// ══════════════════════════════════════════════════════════════
// Main backtest
// ══════════════════════════════════════════════════════════════

/// Run a complete backtest.
///
/// When `shared_indicator_cache` is provided (optimizer use case), indicators that have
/// already been computed by another rayon thread are reused instead of recalculated,
/// giving a significant speedup for Grid Search and Genetic Algorithm runs.
pub fn run_backtest(
    candles: &[Candle],
    sub_bars: &SubBarData,
    strategy: &Strategy,
    config: &BacktestConfig,
    instrument: &InstrumentConfig,
    cancel_flag: &AtomicBool,
    progress_callback: impl Fn(u8, usize, usize),
) -> Result<BacktestResults, AppError> {
    run_backtest_inner(candles, sub_bars, strategy, config, instrument, cancel_flag, progress_callback, None)
}

/// Internal implementation allowing an optional shared indicator cache for optimization.
pub fn run_backtest_with_cache(
    candles: &[Candle],
    sub_bars: &SubBarData,
    strategy: &Strategy,
    config: &BacktestConfig,
    instrument: &InstrumentConfig,
    cancel_flag: &AtomicBool,
    progress_callback: impl Fn(u8, usize, usize),
    shared_cache: Arc<Mutex<IndicatorCache>>,
) -> Result<BacktestResults, AppError> {
    run_backtest_inner(candles, sub_bars, strategy, config, instrument, cancel_flag, progress_callback, Some(shared_cache))
}

fn run_backtest_inner(
    candles: &[Candle],
    sub_bars: &SubBarData,
    strategy: &Strategy,
    config: &BacktestConfig,
    instrument: &InstrumentConfig,
    cancel_flag: &AtomicBool,
    progress_callback: impl Fn(u8, usize, usize),
    shared_indicator_cache: Option<Arc<Mutex<IndicatorCache>>>,
) -> Result<BacktestResults, AppError> {
    let total_bars = candles.len();
    info!("Starting backtest: {} bars, strategy={}, precision={:?}",
        total_bars, strategy.name, config.precision);

    if total_bars == 0 {
        return Err(AppError::NoDataInRange);
    }

    // Clone strategy and pre-compute cached_hash for all IndicatorConfigs once.
    // This eliminates O(bars × operands) hash computations in the hot bar loop.
    let mut strategy_owned = strategy.clone();
    init_strategy_hashes(&mut strategy_owned);
    let strategy = &strategy_owned;

    // Pre-compute all indicators (use shared cache in optimizer context)
    let cache = if let Some(shared) = shared_indicator_cache {
        pre_compute_indicators_with_shared_cache(strategy, candles, &shared)?
    } else {
        pre_compute_indicators(strategy, candles)?
    };

    // Pre-compute daily OHLC boundaries for Daily price fields
    let daily_ohlc = compute_daily_ohlc(candles);

    // Pre-compute time cache for BarTime operands (only if used)
    let time_cache = if strategy_uses_time_fields(strategy) {
        Some(compute_time_cache(candles))
    } else {
        None
    };

    // Pre-compute candle pattern cache (only if used)
    let pattern_cache = if strategy_uses_candle_patterns(strategy) {
        Some(compute_candle_pattern_cache(candles))
    } else {
        None
    };

    // Get ATR values if needed for SL/TP/trailing stop
    let atr_values = compute_atr_if_needed(strategy, candles);

    // Pre-compute order-price indicator values (for Stop/Limit target price)
    let order_price_values = compute_order_price_indicator(strategy, candles);

    let lookback = max_lookback(strategy);
    // Must start at least at bar 1: the loop uses i-1 for indicator values
    let start_bar = lookback.max(1).min(total_bars);

    // Pre-compute spread in price units (used for bid/ask OHLC derivation)
    let spread = orders::spread_price(&strategy.trading_costs, instrument);

    let mut equity = config.initial_capital;
    let mut peak_equity = equity;
    let mut position: Option<OpenPosition> = Option::None;
    let mut pending_order: Option<PendingOrder> = None;
    let mut trades: Vec<TradeResult> = Vec::new();
    let mut equity_curve: Vec<EquityPoint> = Vec::with_capacity(total_bars);
    let mut drawdown_curve: Vec<DrawdownPoint> = Vec::with_capacity(total_bars);
    // Track consecutive losses for AntiMartingale position sizing
    let mut consecutive_losses: u32 = 0;

    // Determine allowed trade direction
    let can_go_long = matches!(
        strategy.trade_direction,
        TradeDirection::Long | TradeDirection::Both
    );
    let can_go_short = matches!(
        strategy.trade_direction,
        TradeDirection::Short | TradeDirection::Both
    );

    // Tick-mode gate: Phase 2.5 only runs for real-tick precision modes
    let is_tick_mode = matches!(
        config.precision,
        crate::models::strategy::BacktestPrecision::RealTickCustomSpread
            | crate::models::strategy::BacktestPrecision::RealTickRealSpread
    );

    // Sub-bar cursor for O(n+m) range lookups
    let mut sub_cursor: usize = 0;

    // Daily trade tracking
    let mut daily_trade_count: usize = 0;
    let mut current_date = String::new();

    // MT5-matching execution model:
    // At bar i, evaluate rules using bar[i-1]'s indicator data (last completed bar)
    // but bar[i]'s time (time_offset=1). Execute entries/exits at bar[i]'s open.
    // This matches MT5 "Open prices only" mode exactly.

    // Pre-compute early-stop bar for zero-trade builder optimization (None = disabled)
    let early_stop_bar: Option<usize> = config.early_stop_no_trades_pct.map(|pct| {
        start_bar + ((total_bars - start_bar) as f32 * pct.clamp(0.0, 1.0)) as usize
    });

    for i in start_bar..total_bars {
        // Check cancellation
        if i % 1000 == 0 {
            if cancel_flag.load(Ordering::Relaxed) {
                info!("Backtest cancelled at bar {}/{}", i, total_bars);
                return Err(AppError::BacktestCancelled);
            }
            let pct = ((i - start_bar) as f64 / (total_bars - start_bar) as f64 * 100.0) as u8;
            progress_callback(pct, i, total_bars);
        }

        // Early termination: abort if no trades have occurred by the early-stop checkpoint
        if let Some(stop_bar) = early_stop_bar {
            if i == stop_bar && trades.is_empty() && position.is_none() {
                return Ok(crate::models::result::BacktestResults {
                    metrics: crate::models::result::BacktestMetrics::default(),
                    trades: vec![],
                    equity_curve: vec![],
                    drawdown_curve: vec![],
                    returns: vec![],
                    backtest_config: config.clone(),
                });
            }
        }

        let candle = &candles[i];
        let next_dt = if i + 1 < total_bars {
            candles[i + 1].datetime.as_str()
        } else {
            ""
        };
        let candle_ts = candle.timestamp;
        let next_ts = if i + 1 < total_bars { candles[i + 1].timestamp } else { i64::MAX };

        // Find sub-bar range for this TF candle (advance cursor)
        let (sub_start, sub_end) = find_subbar_range(
            sub_bars,
            &mut sub_cursor,
            candle.datetime.as_str(),
            next_dt,
            candle_ts,
            next_ts,
        );

        // ── Phase 0: Fill or expire pending limit/stop order ──
        if position.is_none() {
            if let Some(ref pending) = pending_order {
                let filled = match (pending.direction, pending.order_type) {
                    (TradeDirection::Long,  OrderType::Limit) => candle.open <= pending.target_price || candle.low  <= pending.target_price,
                    (TradeDirection::Long,  OrderType::Stop)  => candle.open >= pending.target_price || candle.high >= pending.target_price,
                    (TradeDirection::Short, OrderType::Limit) => candle.open >= pending.target_price || candle.high >= pending.target_price,
                    (TradeDirection::Short, OrderType::Stop)  => candle.open <= pending.target_price || candle.low  <= pending.target_price,
                    _ => false,
                };
                let expired = i.saturating_sub(pending.created_bar) > 20;
                if filled {
                    let fill_price = orders::apply_entry_costs(pending.target_price, pending.direction, &strategy.trading_costs, instrument);
                    let sl_price = strategy.stop_loss.as_ref().map(|sl_cfg| {
                        let sl = calculate_stop_loss(sl_cfg, fill_price, pending.direction, pending.atr_for_sl, instrument);
                        enforce_stops_level_sl(sl, fill_price, pending.direction, instrument)
                    });
                    let fill_lots = calculate_lots(&strategy.position_sizing, equity, fill_price, sl_price, instrument, consecutive_losses);
                    let tp_price = strategy.take_profit.as_ref().map(|tp_cfg| {
                        let tp = calculate_take_profit(tp_cfg, fill_price, sl_price, pending.direction, pending.atr_for_tp, instrument);
                        enforce_stops_level_tp(tp, fill_price, pending.direction, instrument)
                    });
                    let ts_distance = strategy.trailing_stop.as_ref().map(|ts_cfg| {
                        calculate_trailing_stop_distance(ts_cfg, fill_price, sl_price, pending.atr_for_ts, instrument)
                    });
                    position = Some(OpenPosition {
                        direction: pending.direction,
                        entry_price: fill_price,
                        entry_bar: i,
                        entry_time: candle.datetime.clone(),
                        lots: fill_lots,
                        stop_loss: sl_price,
                        take_profit: tp_price,
                        trailing_stop_distance: ts_distance,
                        highest_since_entry: candle.high,
                        lowest_since_entry: candle.low,
                        mae_pips: 0.0,
                        mfe_pips: 0.0,
                        trailing_stop_activated: false,
                        last_swap_date: candle.datetime[..10.min(candle.datetime.len())].to_string(),
                        accumulated_swap: 0.0,
                        sl_moved_to_be: false,
                    });
                    daily_trade_count += 1;
                    pending_order = None;
                } else if expired {
                    pending_order = None;
                }
            }
        }

        // ── Phase 1: Rule-based exit at bar[i] open ──
        // Evaluate using bar[i-1]'s indicator data, bar[i]'s time (time_offset=1)
        if let Some(ref pos) = position {
            let exit_signal = match pos.direction {
                TradeDirection::Long | TradeDirection::Both => {
                    if !strategy.long_exit_groups.is_empty() {
                        evaluate_rule_groups(&strategy.long_exit_groups, i - 1, &cache, candles, Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(), 1)
                    } else {
                        !strategy.long_exit_rules.is_empty() && evaluate_rules(&strategy.long_exit_rules, i - 1, &cache, candles, Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(), 1)
                    }
                }
                TradeDirection::Short => {
                    if !strategy.short_exit_groups.is_empty() {
                        evaluate_rule_groups(&strategy.short_exit_groups, i - 1, &cache, candles, Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(), 1)
                    } else {
                        !strategy.short_exit_rules.is_empty() && evaluate_rules(&strategy.short_exit_rules, i - 1, &cache, candles, Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(), 1)
                    }
                }
            };
            if exit_signal
            {
                let exit_price = candle.open;
                let trade = close_position(
                    pos, exit_price, &candle.datetime, i, CloseReason::Signal,
                    instrument, strategy, config,
                );
                // Swap was already deducted from equity per-bar; only PnL and commission remain
                equity += trade.pnl - trade.commission;
                if trade.pnl >= 1e-6 { consecutive_losses = 0; }
                else if trade.pnl <= -1e-6 { consecutive_losses = consecutive_losses.saturating_add(1); }
                trades.push(trade);
                position = None;
            }
        }

        // ── Phase 1.5: Close after N bars ──
        if let Some(ref pos) = position {
            if let Some(max_bars) = strategy.close_after_bars {
                if i.saturating_sub(pos.entry_bar) >= max_bars as usize {
                    let trade = close_position(
                        pos, candle.open, &candle.datetime, i, CloseReason::ExitAfterBars,
                        instrument, strategy, config,
                    );
                    equity += trade.pnl - trade.commission;
                    if trade.pnl >= 1e-6 { consecutive_losses = 0; }
                    else if trade.pnl <= -1e-6 { consecutive_losses = consecutive_losses.saturating_add(1); }
                    trades.push(trade);
                    position = None;
                }
            }
        }

        // ── Phase 2: Rule-based entry at bar[i] open ──
        // Evaluate using bar[i-1]'s indicator data, bar[i]'s time (time_offset=1)
        if position.is_none() && pending_order.is_none() {
            let bar_date = &candle.datetime[..10.min(candle.datetime.len())];
            if bar_date != current_date {
                current_date = bar_date.to_string();
                daily_trade_count = 0;
            }

            let within_hours = strategy.trading_hours.as_ref()
                .map_or(true, |th| {
                    let (h, m) = extract_hour_minute(&candle.datetime);
                    is_within_trading_hours(th, h, m)
                });
            let under_daily_limit = strategy.max_daily_trades
                .map_or(true, |max| daily_trade_count < max as usize);

            if within_hours && under_daily_limit {
                let mut entry_dir: Option<TradeDirection> = None;

                let long_entry_signal = if !strategy.long_entry_groups.is_empty() {
                    evaluate_rule_groups(&strategy.long_entry_groups, i - 1, &cache, candles, Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(), 1)
                } else {
                    !strategy.long_entry_rules.is_empty() && evaluate_rules(&strategy.long_entry_rules, i - 1, &cache, candles, Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(), 1)
                };
                let short_entry_signal = if !strategy.short_entry_groups.is_empty() {
                    evaluate_rule_groups(&strategy.short_entry_groups, i - 1, &cache, candles, Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(), 1)
                } else {
                    !strategy.short_entry_rules.is_empty() && evaluate_rules(&strategy.short_entry_rules, i - 1, &cache, candles, Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(), 1)
                };

                if can_go_long && long_entry_signal {
                    entry_dir = Some(TradeDirection::Long);
                } else if can_go_short && short_entry_signal {
                    entry_dir = Some(TradeDirection::Short);
                }

                if let Some(dir) = entry_dir {
                    // Use bar[i-1]'s ATR (last completed bar before entry).
                    // Each component uses its own ATR period to avoid silent
                    // "first found wins" contamination across SL/TP/trailing stop.
                    let get_atr = |v: &Option<Vec<f64>>| -> Option<f64> {
                        v.as_ref().and_then(|arr| {
                            let idx = i - 1;
                            if idx < arr.len() && !arr[idx].is_nan() { Some(arr[idx]) } else { None }
                        })
                    };
                    let atr_for_sl = get_atr(&atr_values.for_sl);
                    let atr_for_tp = get_atr(&atr_values.for_tp);
                    let atr_for_ts = get_atr(&atr_values.for_ts);

                    let raw_price = candle.open;

                    match strategy.entry_order {
                        OrderType::Market => {
                            let entry_price = orders::apply_entry_costs(raw_price, dir, &strategy.trading_costs, instrument);
                            let sl_price = strategy.stop_loss.as_ref().map(|sl_cfg| {
                                let sl = calculate_stop_loss(sl_cfg, entry_price, dir, atr_for_sl, instrument);
                                enforce_stops_level_sl(sl, entry_price, dir, instrument)
                            });
                            let lots = calculate_lots(&strategy.position_sizing, equity, entry_price, sl_price, instrument, consecutive_losses);
                            let tp_price = strategy.take_profit.as_ref().map(|tp_cfg| {
                                let tp = calculate_take_profit(tp_cfg, entry_price, sl_price, dir, atr_for_tp, instrument);
                                enforce_stops_level_tp(tp, entry_price, dir, instrument)
                            });
                            let ts_distance = strategy.trailing_stop.as_ref().map(|ts_cfg| {
                                calculate_trailing_stop_distance(ts_cfg, entry_price, sl_price, atr_for_ts, instrument)
                            });
                            position = Some(OpenPosition {
                                direction: dir,
                                entry_price,
                                entry_bar: i,
                                entry_time: candle.datetime.clone(),
                                lots,
                                stop_loss: sl_price,
                                take_profit: tp_price,
                                trailing_stop_distance: ts_distance,
                                highest_since_entry: candle.high + if dir == TradeDirection::Short { spread } else { 0.0 },
                                lowest_since_entry: candle.low + if dir == TradeDirection::Short { spread } else { 0.0 },
                                mae_pips: 0.0,
                                mfe_pips: 0.0,
                                trailing_stop_activated: false,
                                last_swap_date: candle.datetime[..10.min(candle.datetime.len())].to_string(),
                                accumulated_swap: 0.0,
                                sl_moved_to_be: false,
                            });
                            daily_trade_count += 1;
                        }
                        order_type @ (OrderType::Limit | OrderType::Stop) => {
                            let target = if let (Some(opi), Some(ref vals)) = (strategy.entry_order_indicator.as_ref(), order_price_values.as_ref()) {
                                // Use indicator-based offset from the signal bar (i-1)
                                let indicator_val = vals.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
                                let offset = indicator_val * opi.multiplier;
                                let prev = &candles[i.saturating_sub(1)];
                                let base_stop = match opi.base_price_stop {
                                    crate::models::strategy::PriceField::Open  => prev.open,
                                    crate::models::strategy::PriceField::High  => prev.high,
                                    crate::models::strategy::PriceField::Low   => prev.low,
                                    crate::models::strategy::PriceField::Close => prev.close,
                                    _ => prev.high,
                                };
                                let base_limit = match opi.base_price_limit {
                                    crate::models::strategy::PriceField::Open  => prev.open,
                                    crate::models::strategy::PriceField::High  => prev.high,
                                    crate::models::strategy::PriceField::Low   => prev.low,
                                    crate::models::strategy::PriceField::Close => prev.close,
                                    _ => prev.low,
                                };
                                match (dir, order_type) {
                                    (TradeDirection::Long,  OrderType::Stop)  => base_stop + offset,
                                    (TradeDirection::Long,  OrderType::Limit) => base_limit - offset,
                                    (TradeDirection::Short, OrderType::Stop)  => base_stop - offset,
                                    (TradeDirection::Short, OrderType::Limit) => base_limit + offset,
                                    _ => raw_price,
                                }
                            } else {
                                // Fallback: fixed pip offset from bar open
                                let offset = strategy.entry_order_offset_pips * instrument.pip_size;
                                match (dir, order_type) {
                                    (TradeDirection::Long,  OrderType::Limit) => raw_price - offset,
                                    (TradeDirection::Long,  OrderType::Stop)  => raw_price + offset,
                                    (TradeDirection::Short, OrderType::Limit) => raw_price + offset,
                                    (TradeDirection::Short, OrderType::Stop)  => raw_price - offset,
                                    _ => raw_price,
                                }
                            };
                            // Pre-calculate lots using signal-bar price as proxy
                            let proxy_lots = calculate_lots(&strategy.position_sizing, equity, target, None, instrument, consecutive_losses);
                            pending_order = Some(PendingOrder {
                                direction: dir,
                                order_type,
                                target_price: target,
                                lots: proxy_lots,
                                created_bar: i,
                                atr_for_sl,
                                atr_for_tp,
                                atr_for_ts,
                            });
                            daily_trade_count += 1;
                        }
                    }
                }
            }
        }

        // ── Phase 2.5: Tick-by-tick entry (TICK MODE ONLY) ──
        // When no position was opened at bar open (Phase 2), scan each tick of bar[i]
        // with streaming indicator values — matching MT5's "Every Tick" entry behavior.
        // Entry fires at the first tick where the rules become true; subsequent ticks
        // in this bar are handed to Phase 3 (SL/TP) as the start of the open position.
        let mut phase3_sub_start = sub_start; // adjusted when entry fires mid-bar
        if is_tick_mode && position.is_none() && i > 0 {
            if let SubBarData::Ticks(ref ticks) = *sub_bars {
                if sub_start < sub_end {
                    let streaming_state =
                        streaming::build_streaming_state(strategy, &cache, candles, i - 1);
                    let mut streaming_vals = streaming::init_streaming_vals(&streaming_state);

                    // Pre-compile rules ONCE per bar — resolves cache_key() + Vec indices.
                    // Eliminates String allocation + HashMap lookup on every tick.
                    let fast_long = compile_rules_streaming(&strategy.long_entry_rules, &streaming_state);
                    let fast_short = compile_rules_streaming(&strategy.short_entry_rules, &streaming_state);

                    // Pre-compute CrossAbove/CrossBelow "previous bar" values ONCE before the tick loop.
                    let long_cross_prev = precompute_cross_prev_vals(
                        &strategy.long_entry_rules, i, &cache, candles,
                        Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(),
                    );
                    let short_cross_prev = precompute_cross_prev_vals(
                        &strategy.short_entry_rules, i, &cache, candles,
                        Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(),
                    );

                    let mut running_high = candle.open;
                    let mut running_low = candle.open;

                    'tick_entry: for j in sub_start..sub_end {
                        let tick_bid = ticks.bids[j];
                        let tick_ask = ticks.asks[j];
                        let tick_mid = (tick_bid + tick_ask) * 0.5;

                        // Update running OHLCV (open is fixed; high/low track mid-price extremes)
                        if tick_mid > running_high { running_high = tick_mid; }
                        if tick_mid < running_low  { running_low = tick_mid; }

                        // Refresh streaming indicator values for this tick
                        streaming::update_streaming_vals(
                            &streaming_state, &mut streaming_vals,
                            running_high, running_low, tick_mid,
                        );

                        // Synthetic in-progress candle for Price operand resolution
                        let running_candle = Candle {
                            timestamp: ticks.timestamps[j],
                            datetime: String::new(),
                            open: candle.open,
                            high: running_high,
                            low: running_low,
                            close: tick_mid,
                            volume: 0.0,
                        };

                        // Trading-hours guard: use integer arithmetic — no String allocation per tick
                        let within_hours = strategy.trading_hours.as_ref().map_or(true, |th| {
                            let (h, m) = hour_minute_from_micros(ticks.timestamps[j]);
                            is_within_trading_hours(th, h, m)
                        });
                        let under_daily_limit = strategy
                            .max_daily_trades
                            .map_or(true, |max| daily_trade_count < max as usize);
                        if !within_hours || !under_daily_limit {
                            continue 'tick_entry;
                        }

                        // Evaluate entry rules — zero allocation per tick (FastOp path)
                        let mut tick_dir: Option<TradeDirection> = None;
                        if can_go_long && !fast_long.is_empty()
                            && evaluate_rules_fast(
                                &fast_long, &strategy.long_entry_rules, i, &cache,
                                &streaming_state, &streaming_vals, &long_cross_prev,
                                candles, &running_candle,
                                Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(),
                            )
                        {
                            tick_dir = Some(TradeDirection::Long);
                        } else if can_go_short && !fast_short.is_empty()
                            && evaluate_rules_fast(
                                &fast_short, &strategy.short_entry_rules, i, &cache,
                                &streaming_state, &streaming_vals, &short_cross_prev,
                                candles, &running_candle,
                                Some(&daily_ohlc), time_cache.as_ref(), pattern_cache.as_ref(),
                            )
                        {
                            tick_dir = Some(TradeDirection::Short);
                        }

                        if let Some(dir) = tick_dir {
                            // Execute at tick's actual bid/ask price
                            let raw_price = match dir {
                                TradeDirection::Long => tick_ask,
                                TradeDirection::Short | TradeDirection::Both => tick_bid,
                            };
                            let entry_price = orders::apply_entry_costs(
                                raw_price, dir, &strategy.trading_costs, instrument,
                            );

                            // ATR from bar[i-1] (last completed bar)
                            let get_atr = |v: &Option<Vec<f64>>| -> Option<f64> {
                                v.as_ref().and_then(|arr| {
                                    let idx = i - 1;
                                    if idx < arr.len() && !arr[idx].is_nan() { Some(arr[idx]) } else { None }
                                })
                            };
                            let atr_for_sl = get_atr(&atr_values.for_sl);
                            let atr_for_tp = get_atr(&atr_values.for_tp);
                            let atr_for_ts = get_atr(&atr_values.for_ts);

                            let sl_price = strategy.stop_loss.as_ref().map(|sl_cfg| {
                                let sl = calculate_stop_loss(sl_cfg, entry_price, dir, atr_for_sl, instrument);
                                enforce_stops_level_sl(sl, entry_price, dir, instrument)
                            });
                            let lots = calculate_lots(
                                &strategy.position_sizing, equity, entry_price,
                                sl_price, instrument, consecutive_losses,
                            );
                            let tp_price = strategy.take_profit.as_ref().map(|tp_cfg| {
                                let tp = calculate_take_profit(tp_cfg, entry_price, sl_price, dir, atr_for_tp, instrument);
                                enforce_stops_level_tp(tp, entry_price, dir, instrument)
                            });
                            let ts_distance = strategy.trailing_stop.as_ref().map(|ts_cfg| {
                                calculate_trailing_stop_distance(ts_cfg, entry_price, sl_price, atr_for_ts, instrument)
                            });

                            // Only convert timestamp to String when an entry actually fires (rare event)
                            let tick_dt = micros_to_datetime_string(ticks.timestamps[j]);
                            position = Some(OpenPosition {
                                direction: dir,
                                entry_price,
                                entry_bar: i,
                                entry_time: tick_dt,
                                lots,
                                stop_loss: sl_price,
                                take_profit: tp_price,
                                trailing_stop_distance: ts_distance,
                                // running_high/low are mid-prices (avg of bid+ask ticks).
                                // Convert to bid side (Long TS) and ask side (Short TS):
                                //   bid ≈ mid − spread/2,  ask ≈ mid + spread/2
                                highest_since_entry: running_high - spread * 0.5,
                                lowest_since_entry: running_low + spread * 0.5,
                                mae_pips: 0.0,
                                mfe_pips: 0.0,
                                trailing_stop_activated: false,
                                last_swap_date: candle.datetime
                                    [..10.min(candle.datetime.len())]
                                    .to_string(),
                                accumulated_swap: 0.0,
                                sl_moved_to_be: false,
                            });
                            daily_trade_count += 1;
                            // Phase 3 must start from the tick AFTER entry
                            phase3_sub_start = j + 1;
                            break 'tick_entry;
                        }
                    }
                }
            }
        }

        // ── Phase 3: Check SL/TP for existing position ──
        if let Some(ref mut pos) = position {
            let exit_result = resolve_exit(
                pos, candle, sub_bars, phase3_sub_start, sub_end, instrument, spread,
            );

            if let Some((exit_price, exit_time, reason)) = exit_result {
                let trade = close_position(
                    pos, exit_price, &exit_time, i, reason, instrument, strategy, config,
                );
                equity += trade.pnl - trade.commission;
                if trade.pnl >= 1e-6 { consecutive_losses = 0; }
                else if trade.pnl <= -1e-6 { consecutive_losses = consecutive_losses.saturating_add(1); }
                trades.push(trade);
                position = None;
            }
        }

        // ── Phase 4: Check force-close at specified time ──
        if let Some(ref pos) = position {
            if should_close_at_time(&strategy.close_trades_at, &candle.datetime) {
                let exit_price = candle.close;
                let trade = close_position(
                    pos, exit_price, &candle.datetime, i, CloseReason::TimeClose,
                    instrument, strategy, config,
                );
                equity += trade.pnl - trade.commission;
                if trade.pnl >= 1e-6 { consecutive_losses = 0; }
                else if trade.pnl <= -1e-6 { consecutive_losses = consecutive_losses.saturating_add(1); }
                trades.push(trade);
                position = None;
            }
        }

        // ── Phase 4.5: Charge overnight swap ──
        if let Some(ref mut pos) = position {
            let (charge, multiplier) = should_charge_swap(pos, &candle.datetime, instrument);
            if charge {
                let swap = calculate_swap_charge(pos.direction, pos.lots, pos.entry_price, instrument, multiplier);
                pos.accumulated_swap += swap;
                pos.last_swap_date = candle.datetime[..10.min(candle.datetime.len())].to_string();
                equity += swap; // swap is negative when it costs the trader
            }
        }

        // ── Phase 5: Update trailing stop (if position survived all exit checks) ──
        if let Some(ref mut pos) = position {
            if matches!(sub_bars, SubBarData::None) {
                let ba = orders::BidAskOhlc::from_candle(candle, spread);
                update_trailing_stop(pos, &ba);
            }
        }

        // ── Phase 5.5: Move SL to breakeven ──
        if strategy.move_sl_to_be {
            if let Some(ref mut pos) = position {
                if !pos.sl_moved_to_be {
                    if let Some(sl) = pos.stop_loss {
                        let sl_distance = (pos.entry_price - sl).abs();
                        let profit = match pos.direction {
                            TradeDirection::Long | TradeDirection::Both => candle.close - pos.entry_price,
                            TradeDirection::Short => pos.entry_price - candle.close,
                        };
                        if profit >= sl_distance {
                            pos.stop_loss = Some(pos.entry_price);
                            pos.sl_moved_to_be = true;
                        }
                    }
                }
            }
        }

        // ── Phase 6: Record equity and drawdown ──
        let unrealized = if let Some(ref pos) = position {
            let pnl_pips = orders::calculate_pnl_pips(
                pos.direction,
                pos.entry_price,
                candle.close,
                instrument,
            );
            pnl_pips * instrument.pip_value * pos.lots
        } else {
            0.0
        };

        let current_equity = equity + unrealized;
        if current_equity > peak_equity {
            peak_equity = current_equity;
        }
        let drawdown_pct = if peak_equity > 0.0 {
            (peak_equity - current_equity) / peak_equity * 100.0
        } else {
            0.0
        };

        equity_curve.push(EquityPoint {
            timestamp: candle.datetime.clone(),
            equity: current_equity,
        });
        drawdown_curve.push(DrawdownPoint {
            timestamp: candle.datetime.clone(),
            drawdown_pct,
        });
    }

    // ── 4. Close any remaining position at end of data ──
    if let Some(ref pos) = position {
        let last_candle = &candles[total_bars - 1];
        let trade = close_position(
            pos,
            last_candle.close,
            &last_candle.datetime,
            total_bars - 1,
            CloseReason::EndOfData,
            instrument,
            strategy,
            config,
        );
        equity += trade.pnl - trade.commission;
        // Note: swap was already deducted from equity per-bar; no adjustment needed here
        trades.push(trade);

        // Update the last equity/drawdown curve point to reflect the settled
        // (closed) value instead of the unrealized value written during the loop.
        if peak_equity < equity {
            peak_equity = equity;
        }
        let final_dd_pct = if peak_equity > 0.0 {
            (peak_equity - equity) / peak_equity * 100.0
        } else {
            0.0
        };
        if let Some(last) = equity_curve.last_mut() {
            last.equity = equity;
        }
        if let Some(last) = drawdown_curve.last_mut() {
            last.drawdown_pct = final_dd_pct;
        }
    }

    progress_callback(100, total_bars, total_bars);
    info!("Backtest complete: {} trades", trades.len());

    // ── 5. Calculate metrics ──
    let metrics = calculate_metrics(&trades, &equity_curve, config.initial_capital, config.timeframe);

    let returns: Vec<f64> = trades.iter().map(|t| t.pnl).collect();

    Ok(BacktestResults {
        trades,
        equity_curve,
        drawdown_curve,
        returns,
        metrics,
        backtest_config: config.clone(),
    })
}

// ══════════════════════════════════════════════════════════════
// Sub-bar resolution
// ══════════════════════════════════════════════════════════════

/// Find the sub-bar range [start, end) for a TF candle and advance the cursor.
/// O(n+m) total across all candles in the backtest.
/// For Candles: uses string datetime comparison.
/// For Ticks: uses i64 microsecond timestamp comparison (~10x faster).
fn find_subbar_range(
    sub_bars: &SubBarData,
    cursor: &mut usize,
    candle_dt: &str,
    next_dt: &str,
    candle_ts: i64,
    next_ts: i64,
) -> (usize, usize) {
    match sub_bars {
        SubBarData::None => (0, 0),
        SubBarData::Candles(subs) => {
            let total = subs.len();
            // Skip past sub-bars before this candle (string comparison)
            while *cursor < total && subs[*cursor].datetime.as_str() < candle_dt {
                *cursor += 1;
            }
            let start = *cursor;
            let mut end = start;
            if !next_dt.is_empty() {
                while end < total && subs[end].datetime.as_str() < next_dt {
                    end += 1;
                }
            } else {
                end = total;
            }
            *cursor = end;
            (start, end)
        }
        SubBarData::Ticks(ticks) => {
            let total = ticks.len();
            let ts = &ticks.timestamps;
            // i64 comparison — ~10x faster than string comparison per step
            while *cursor < total && ts[*cursor] < candle_ts {
                *cursor += 1;
            }
            let start = *cursor;
            let mut end = start;
            if next_ts < i64::MAX {
                while end < total && ts[end] < next_ts {
                    end += 1;
                }
            } else {
                end = total;
            }
            *cursor = end;
            (start, end)
        }
    }
}

/// Resolve SL/TP exit for an open position.
/// Uses sub-bar data when available, otherwise falls back to TF candle OHLC.
/// `spread` is the full spread in price units (used to derive BidAsk prices in bar mode).
/// Returns (exit_price, exit_time, reason) if an exit is triggered.
fn resolve_exit(
    pos: &mut OpenPosition,
    candle: &Candle,
    sub_bars: &SubBarData,
    sub_start: usize,
    sub_end: usize,
    instrument: &InstrumentConfig,
    spread: f64,
) -> Option<(f64, String, CloseReason)> {
    match sub_bars {
        SubBarData::None => {
            // SelectedTfOnly: derive bid/ask OHLC from TF candle + spread
            let ba = orders::BidAskOhlc::from_candle(candle, spread);
            update_mae_mfe_ba(pos, &ba, instrument);
            check_sl_tp_hit(pos, &ba)
                .map(|(price, reason)| (price, candle.datetime.clone(), reason))
        }
        SubBarData::Candles(subs) => {
            // M1TickSimulation: iterate M1 sub-candles
            process_subbars_candle(pos, subs, sub_start, sub_end, instrument, spread)
        }
        SubBarData::Ticks(ticks) => {
            // RealTick modes: optimized columnar tick processing
            process_subbars_tick_columnar(pos, ticks, sub_start, sub_end, instrument)
        }
    }
}

/// Process M1 sub-candles for SL/TP resolution within a TF bar.
/// Derives bid/ask OHLC per sub-candle using the configured spread.
/// Updates MAE/MFE and trailing stop on each sub-candle.
fn process_subbars_candle(
    pos: &mut OpenPosition,
    sub_candles: &[Candle],
    start: usize,
    end: usize,
    instrument: &InstrumentConfig,
    spread: f64,
) -> Option<(f64, String, CloseReason)> {
    for i in start..end {
        let sc = &sub_candles[i];
        let ba = orders::BidAskOhlc::from_candle(sc, spread);
        // Update MAE/MFE using bid/ask split
        update_mae_mfe_ba(pos, &ba, instrument);
        // Check SL/TP (with current trailing stop level)
        if let Some((exit_price, reason)) = check_sl_tp_hit(pos, &ba) {
            return Some((exit_price, sc.datetime.clone(), reason));
        }
        // Update trailing stop for next sub-bar
        update_trailing_stop(pos, &ba);
    }
    None
}

/// High-performance tick processing using columnar (SoA) data layout.
///
/// Optimizations vs the old `process_subbars_tick`:
/// - Direction check hoisted OUTSIDE the loop (eliminates match per tick)
/// - MAE/MFE, SL/TP check, and trailing stop inlined into one pass
/// - Contiguous f64 slices for bid/ask maximize CPU cache hits
/// - No String allocations during iteration (timestamps are i64)
/// - Exit time string conversion only happens on the rare trade-close event
fn process_subbars_tick_columnar(
    pos: &mut OpenPosition,
    ticks: &TickColumns,
    start: usize,
    end: usize,
    instrument: &InstrumentConfig,
) -> Option<(f64, String, CloseReason)> {
    if start >= end {
        return None;
    }

    let pip_size = instrument.pip_size;
    let bids = &ticks.bids[start..end];
    let asks = &ticks.asks[start..end];
    let timestamps = &ticks.timestamps[start..end];
    let is_long = matches!(pos.direction, TradeDirection::Long | TradeDirection::Both);

    if is_long {
        // ── Long: exits at bid ──
        for j in 0..bids.len() {
            let bid = bids[j];

            // MAE/MFE (inlined — no function call overhead)
            let adverse = (pos.entry_price - bid) / pip_size;
            let favorable = (bid - pos.entry_price) / pip_size;
            if adverse > pos.mae_pips { pos.mae_pips = adverse; }
            if favorable > pos.mfe_pips { pos.mfe_pips = favorable; }

            // SL check (long exits at bid — stop-market fills at bid)
            if let Some(sl) = pos.stop_loss {
                if bid <= sl {
                    let reason = if pos.trailing_stop_activated { CloseReason::TrailingStop } else { CloseReason::StopLoss };
                    return Some((bid, micros_to_datetime_string(timestamps[j]), reason));
                }
            }
            // TP check (long exits at bid — limit fills at TP level)
            if let Some(tp) = pos.take_profit {
                if bid >= tp {
                    return Some((tp, micros_to_datetime_string(timestamps[j]), CloseReason::TakeProfit));
                }
            }
            // Trailing stop (track highest bid)
            if let Some(distance) = pos.trailing_stop_distance {
                if bid > pos.highest_since_entry {
                    pos.highest_since_entry = bid;
                    let new_sl = bid - distance;
                    let moved = match pos.stop_loss {
                        Some(ref mut sl) if new_sl > *sl => { *sl = new_sl; true }
                        None => { pos.stop_loss = Some(new_sl); true }
                        _ => false,
                    };
                    if moved { pos.trailing_stop_activated = true; }
                }
            }
        }
    } else {
        // ── Short: exits at ask ──
        for j in 0..asks.len() {
            let ask = asks[j];

            // MAE/MFE
            let adverse = (ask - pos.entry_price) / pip_size;
            let favorable = (pos.entry_price - ask) / pip_size;
            if adverse > pos.mae_pips { pos.mae_pips = adverse; }
            if favorable > pos.mfe_pips { pos.mfe_pips = favorable; }

            // SL check (short exits at ask — stop-market fills at ask)
            if let Some(sl) = pos.stop_loss {
                if ask >= sl {
                    let reason = if pos.trailing_stop_activated { CloseReason::TrailingStop } else { CloseReason::StopLoss };
                    return Some((ask, micros_to_datetime_string(timestamps[j]), reason));
                }
            }
            // TP check (short exits at ask — limit fills at TP level)
            if let Some(tp) = pos.take_profit {
                if ask <= tp {
                    return Some((tp, micros_to_datetime_string(timestamps[j]), CloseReason::TakeProfit));
                }
            }
            // Trailing stop (track lowest ask)
            if let Some(distance) = pos.trailing_stop_distance {
                if ask < pos.lowest_since_entry {
                    pos.lowest_since_entry = ask;
                    let new_sl = ask + distance;
                    let moved = match pos.stop_loss {
                        Some(ref mut sl) if new_sl < *sl => { *sl = new_sl; true }
                        None => { pos.stop_loss = Some(new_sl); true }
                        _ => false,
                    };
                    if moved { pos.trailing_stop_activated = true; }
                }
            }
        }
    }
    None
}

// ══════════════════════════════════════════════════════════════
// Helpers
// ══════════════════════════════════════════════════════════════

/// Close a position and create a TradeResult.
fn close_position(
    pos: &OpenPosition,
    exit_price: f64,
    exit_time: &str,
    exit_bar: usize,
    reason: CloseReason,
    instrument: &InstrumentConfig,
    strategy: &Strategy,
    config: &BacktestConfig,
) -> TradeResult {
    // Apply exit costs (slippage on exit)
    let adjusted_exit = orders::apply_exit_costs(
        exit_price,
        pos.direction,
        &strategy.trading_costs,
        instrument,
    );
    let pnl = orders::calculate_pnl(pos.direction, pos.entry_price, adjusted_exit, pos.lots, instrument);
    let pnl_pips =
        orders::calculate_pnl_pips(pos.direction, pos.entry_price, adjusted_exit, instrument);
    let commission =
        orders::calculate_commission(&strategy.trading_costs, pos.lots, pos.entry_price, instrument);
    let duration_bars = exit_bar - pos.entry_bar;
    let mpb = config.timeframe.minutes().max(1);

    TradeResult {
        id: uuid::Uuid::new_v4().to_string(),
        direction: pos.direction,
        entry_time: pos.entry_time.clone(),
        entry_price: pos.entry_price,
        exit_time: exit_time.to_string(),
        exit_price: adjusted_exit,
        lots: pos.lots,
        pnl,
        pnl_pips,
        commission,
        swap: pos.accumulated_swap,
        close_reason: reason,
        duration_bars,
        duration_time: format_duration_bars(duration_bars, mpb),
        mae: pos.mae_pips,
        mfe: pos.mfe_pips,
    }
}

/// Format duration in bars to a human-readable string.
fn format_duration_bars(bars: usize, minutes_per_bar: u32) -> String {
    let total_minutes = bars as u64 * minutes_per_bar as u64;
    if total_minutes < 60 {
        format!("{}m", total_minutes)
    } else if total_minutes < 1440 {
        format!("{}h {}m", total_minutes / 60, total_minutes % 60)
    } else {
        format!("{}d {}h", total_minutes / 1440, (total_minutes % 1440) / 60)
    }
}

/// Per-component ATR series, one for each SL/TP/TrailingStop that uses ATR.
/// Each field is `None` if the component doesn't use ATR or its period is unset.
pub(super) struct AtrValues {
    pub for_sl: Option<Vec<f64>>,
    pub for_tp: Option<Vec<f64>>,
    pub for_ts: Option<Vec<f64>>,
}

/// Compute separate ATR series for SL, TP, and trailing stop.
///
/// Each component may specify its own `atr_period`. Previously a single
/// "first found wins" period was shared, silently giving wrong ATR values
/// when SL, TP, and TS used different periods.
fn compute_atr_if_needed(strategy: &Strategy, candles: &[Candle]) -> AtrValues {
    AtrValues {
        for_sl: compute_atr_for_period(
            strategy
                .stop_loss
                .as_ref()
                .filter(|sl| matches!(sl.sl_type, crate::models::strategy::StopLossType::ATR))
                .and_then(|sl| sl.atr_period),
            candles,
        ),
        for_tp: compute_atr_for_period(
            strategy
                .take_profit
                .as_ref()
                .filter(|tp| matches!(tp.tp_type, crate::models::strategy::TakeProfitType::ATR))
                .and_then(|tp| tp.atr_period),
            candles,
        ),
        for_ts: compute_atr_for_period(
            strategy
                .trailing_stop
                .as_ref()
                .filter(|ts| matches!(ts.ts_type, crate::models::strategy::TrailingStopType::ATR))
                .and_then(|ts| ts.atr_period),
            candles,
        ),
    }
}

/// Pre-compute the primary output values of the order-price indicator, if configured.
fn compute_order_price_indicator(strategy: &Strategy, candles: &[Candle]) -> Option<Vec<f64>> {
    let opi = strategy.entry_order_indicator.as_ref()?;
    match super::indicators::compute_indicator(&opi.indicator, candles) {
        Ok(output) => Some(output.primary),
        Err(_) => None,
    }
}

/// Compute an ATR series for a specific period. Returns `None` if period is `None`.
fn compute_atr_for_period(period: Option<usize>, candles: &[Candle]) -> Option<Vec<f64>> {
    let period = period?;
    let config = IndicatorConfig {
        indicator_type: IndicatorType::ATR,
        params: crate::models::strategy::IndicatorParams {
            period: Some(period),
            ..Default::default()
        },
        output_field: None,
        cached_hash: 0,
    };
    match super::indicators::compute_indicator(&config, candles) {
        Ok(output) => Some(output.primary),
        Err(_) => None,
    }
}

// ══════════════════════════════════════════════════════════════
// Trading hours helpers
// ══════════════════════════════════════════════════════════════

/// Extract hour and minute from a datetime string "YYYY-MM-DD HH:MM:SS...".
/// Zero-allocation: reads directly from byte positions.
fn extract_hour_minute(datetime: &str) -> (u8, u8) {
    let b = datetime.as_bytes();
    if b.len() >= 16 {
        let h = (b[11] - b'0') * 10 + (b[12] - b'0');
        let m = (b[14] - b'0') * 10 + (b[15] - b'0');
        (h, m)
    } else {
        (0, 0)
    }
}

/// Extract hour and minute from a microseconds-since-epoch timestamp using pure integer arithmetic.
///
/// Called on every tick in the hot tick loop — avoids all String allocation compared to
/// `micros_to_datetime_string` + `extract_hour_minute`.
#[inline(always)]
fn hour_minute_from_micros(micros: i64) -> (u8, u8) {
    let secs = micros / 1_000_000;
    // Seconds within the day (UTC)
    let secs_in_day = secs.rem_euclid(86_400) as u32;
    let h = (secs_in_day / 3600) as u8;
    let m = ((secs_in_day % 3600) / 60) as u8;
    (h, m)
}

/// Check if the current bar's time matches or exceeds the close_trades_at time.
/// Returns true if the position should be force-closed.
fn should_close_at_time(close_at: &Option<CloseTradesAt>, datetime: &str) -> bool {
    if let Some(ref ct) = close_at {
        let (h, m) = extract_hour_minute(datetime);
        let current = h as u16 * 60 + m as u16;
        let target = ct.hour as u16 * 60 + ct.minute as u16;
        current >= target
    } else {
        false
    }
}

/// Check if a given hour:minute is within the configured trading hours window.
/// Handles ranges that cross midnight (e.g. 22:00 → 06:00).
fn is_within_trading_hours(hours: &TradingHours, h: u8, m: u8) -> bool {
    let current = h as u16 * 60 + m as u16;
    let start = hours.start_hour as u16 * 60 + hours.start_minute as u16;
    let end = hours.end_hour as u16 * 60 + hours.end_minute as u16;
    if start <= end {
        current >= start && current <= end
    } else {
        // Crosses midnight (e.g. 22:00 → 06:00)
        current >= start || current <= end
    }
}

// ══════════════════════════════════════════════════════════════
// Bulk extraction helpers (vectorized — avoids per-element .get())
// ══════════════════════════════════════════════════════════════

/// Fast bulk extraction of f64 values from a Float64 ChunkedArray.
/// Rechunks into a single contiguous chunk, then copies via memcpy.
/// Nulls are replaced with 0.0.
fn chunked_f64_to_vec(ca: &polars::prelude::Float64Chunked) -> Vec<f64> {
    let rechunked = ca.rechunk();
    if !rechunked.has_nulls() {
        // Fast path: no nulls → direct memcpy from Arrow buffer.
        // rechunk() guarantees exactly one chunk; if somehow empty, fall through to slow path.
        if let Some(arr) = rechunked.downcast_iter().next() {
            return arr.values().as_slice().to_vec();
        }
    }
    // Slow path: null values present or empty array
    (0..rechunked.len()).map(|i| rechunked.get(i).unwrap_or(0.0)).collect()
}

/// Fast bulk extraction of i64 values from an Int64 ChunkedArray.
/// Rechunks into a single contiguous chunk, then copies via memcpy.
/// Nulls are replaced with 0.
fn chunked_i64_to_vec(ca: &polars::prelude::Int64Chunked) -> Vec<i64> {
    let rechunked = ca.rechunk();
    if !rechunked.has_nulls() {
        // Fast path: no nulls → direct memcpy from Arrow buffer.
        // rechunk() guarantees exactly one chunk; if somehow empty, fall through to slow path.
        if let Some(arr) = rechunked.downcast_iter().next() {
            return arr.values().as_slice().to_vec();
        }
    }
    // Slow path: null values present or empty array
    (0..rechunked.len()).map(|i| rechunked.get(i).unwrap_or(0)).collect()
}

// ══════════════════════════════════════════════════════════════
// Timestamp utilities
// ══════════════════════════════════════════════════════════════

/// Extract i64 microsecond timestamps from a Polars datetime column.
/// Uses bulk extraction (rechunk+memcpy) instead of per-element .get().
fn extract_timestamps_micros(
    col: &polars::prelude::Column,
    _len: usize,
) -> Result<Vec<i64>, AppError> {
    match col.dtype() {
        DataType::Datetime(tu, _) => {
            let ts_col = col
                .cast(&DataType::Int64)
                .map_err(|e| AppError::Internal(format!("datetime→i64: {}", e)))?;
            let ca = ts_col
                .i64()
                .map_err(|e| AppError::Internal(format!("i64 chunked: {}", e)))?;
            let raw = chunked_i64_to_vec(ca);
            let (mul, div) = match tu {
                TimeUnit::Milliseconds => (1000i64, 1i64),
                TimeUnit::Microseconds => (1, 1),
                TimeUnit::Nanoseconds => (1, 1000),
            };
            if mul == 1 && div == 1 {
                Ok(raw) // Microseconds — no transformation needed
            } else {
                Ok(raw.iter().map(|&v| v * mul / div).collect())
            }
        }
        _ => {
            // Fallback: parse string datetimes to microseconds
            let str_col = col
                .cast(&DataType::String)
                .map_err(|e| AppError::Internal(format!("datetime str cast: {}", e)))?;
            let ca = str_col
                .str()
                .map_err(|e| AppError::Internal(format!("datetime str: {}", e)))?;
            let len = col.len();
            let mut timestamps = Vec::with_capacity(len);
            for i in 0..len {
                timestamps.push(parse_datetime_to_micros(ca.get(i).unwrap_or("")));
            }
            Ok(timestamps)
        }
    }
}

/// Parse a datetime string to microseconds since epoch.
/// Supports common formats: "YYYY-MM-DD HH:MM:SS", "YYYY-MM-DD HH:MM:SS.ffffff", etc.
fn parse_datetime_to_micros(s: &str) -> i64 {
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M"))
        .map(|dt| dt.and_utc().timestamp_micros())
        .unwrap_or(0)
}

/// Convert microseconds since epoch back to a datetime string.
/// Only called on trade close events (rare), so performance is not critical.
fn micros_to_datetime_string(micros: i64) -> String {
    let secs = micros / 1_000_000;
    let subsec_nanos = ((micros % 1_000_000).unsigned_abs() as u32) * 1000;
    chrono::DateTime::from_timestamp(secs, subsec_nanos)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S%.6f").to_string())
        .unwrap_or_else(|| format!("ts:{}", micros))
}

// ══════════════════════════════════════════════════════════════
// DataFrame → Candle/Tick conversion
// ══════════════════════════════════════════════════════════════

/// Convert a Polars DataFrame to a Vec<Candle>.
/// Also extracts i64 timestamps for fast sub-bar range lookups.
pub fn candles_from_dataframe(df: &DataFrame) -> Result<Vec<Candle>, AppError> {
    let datetime_col = df
        .column("datetime")
        .map_err(|_| AppError::CsvValidation("No 'datetime' column in DataFrame".into()))?;
    let open_col = df
        .column("open")
        .map_err(|_| AppError::CsvValidation("No 'open' column".into()))?;
    let high_col = df
        .column("high")
        .map_err(|_| AppError::CsvValidation("No 'high' column".into()))?;
    let low_col = df
        .column("low")
        .map_err(|_| AppError::CsvValidation("No 'low' column".into()))?;
    let close_col = df
        .column("close")
        .map_err(|_| AppError::CsvValidation("No 'close' column".into()))?;
    let volume_col = df
        .column("volume")
        .map_err(|_| AppError::CsvValidation("No 'volume' column".into()))?;

    let len = df.height();

    // Extract i64 timestamps for sub-bar range lookups
    let timestamps = extract_timestamps_micros(datetime_col, len)?;

    // Vectorized datetime-to-string (still needed for display/frontend)
    let dt_cast = datetime_col
        .cast(&DataType::String)
        .map_err(|e| AppError::Internal(format!("datetime cast: {}", e)))?;
    let dt_ca = dt_cast
        .str()
        .map_err(|e| AppError::Internal(format!("datetime str: {}", e)))?;

    let open_ca = open_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;
    let high_ca = high_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;
    let low_ca = low_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;
    let close_ca = close_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;
    let volume_ca = volume_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;

    // Bulk extraction of f64 columns (rechunk → memcpy)
    let open_v = chunked_f64_to_vec(open_ca);
    let high_v = chunked_f64_to_vec(high_ca);
    let low_v = chunked_f64_to_vec(low_ca);
    let close_v = chunked_f64_to_vec(close_ca);
    let volume_v = chunked_f64_to_vec(volume_ca);

    let mut candles = Vec::with_capacity(len);
    for i in 0..len {
        candles.push(Candle {
            timestamp: timestamps[i],
            datetime: dt_ca.get(i).unwrap_or("").to_string(),
            open: open_v[i],
            high: high_v[i],
            low: low_v[i],
            close: close_v[i],
            volume: volume_v[i],
        });
    }

    Ok(candles)
}

/// Convert a raw tick Parquet DataFrame (datetime, bid, ask) to TickColumns (SoA).
/// Extracts i64 timestamps directly from Polars — NO per-tick String allocations.
pub fn tick_columns_from_dataframe(df: &DataFrame) -> Result<TickColumns, AppError> {
    let datetime_col = df
        .column("datetime")
        .map_err(|_| AppError::CsvValidation("No 'datetime' column in tick DataFrame".into()))?;
    let bid_col = df
        .column("bid")
        .map_err(|_| AppError::CsvValidation("No 'bid' column in tick DataFrame".into()))?;
    let ask_col = df
        .column("ask")
        .map_err(|_| AppError::CsvValidation("No 'ask' column in tick DataFrame".into()))?;

    let len = df.height();

    // Extract i64 timestamps directly — avoids 20M+ String allocations
    let timestamps = extract_timestamps_micros(datetime_col, len)?;

    let bid_ca = bid_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;
    let ask_ca = ask_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;

    // Bulk extraction: rechunk → memcpy (instead of 398M .get() calls)
    let bids = chunked_f64_to_vec(bid_ca);
    let asks = chunked_f64_to_vec(ask_ca);

    info!("Loaded {} ticks as TickColumns (SoA, ~{}MB)",
        len,
        (len * (8 + 8 + 8)) / (1024 * 1024)
    );

    Ok(TickColumns { timestamps, bids, asks })
}

/// Convert tick OHLCV DataFrame to TickColumns with custom spread applied.
/// Extracts i64 timestamps + synthetic bid/ask from close ± half_spread.
pub fn tick_columns_from_ohlcv_with_spread(
    df: &DataFrame,
    half_spread: f64,
) -> Result<TickColumns, AppError> {
    let datetime_col = df
        .column("datetime")
        .map_err(|_| AppError::CsvValidation("No 'datetime' column".into()))?;
    let close_col = df
        .column("close")
        .map_err(|_| AppError::CsvValidation("No 'close' column".into()))?;

    let len = df.height();

    let timestamps = extract_timestamps_micros(datetime_col, len)?;
    let close_ca = close_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;

    // Bulk extraction: rechunk → memcpy, then vectorized spread application
    let close_vals = chunked_f64_to_vec(close_ca);
    let bids: Vec<f64> = close_vals.iter().map(|&mid| mid - half_spread).collect();
    let asks: Vec<f64> = close_vals.iter().map(|&mid| mid + half_spread).collect();

    Ok(TickColumns { timestamps, bids, asks })
}

/// Normalize an end_date string for inclusive comparison against datetime strings.
///
/// Datetime strings in the data look like "2024-01-15 23:59:59.000000", so a
/// date-only end_date like "2024-01-15" would compare as LESS than any bar on
/// that day (the space character makes the full string lexicographically greater).
/// Appending end-of-day time ensures the entire last day is included.
fn normalize_end_date(end_date: &str) -> String {
    if end_date.len() == 10 {
        // Pure date "YYYY-MM-DD" — append max time so all bars on this day are included.
        format!("{} 23:59:59.999999", end_date)
    } else {
        end_date.to_string()
    }
}

/// Filter a DataFrame by date range using Polars lazy expressions.
/// Much faster than converting to structs first — filters at the columnar level.
/// Data must have a 'datetime' column.
pub fn filter_dataframe_by_date(
    df: DataFrame,
    start_date: &str,
    end_date: &str,
) -> Result<DataFrame, AppError> {
    if start_date.is_empty() && end_date.is_empty() {
        return Ok(df);
    }

    let mut lf = df.lazy();

    if !start_date.is_empty() {
        lf = lf.filter(
            col("datetime")
                .cast(DataType::String)
                .gt_eq(lit(start_date)),
        );
    }

    if !end_date.is_empty() {
        let end = normalize_end_date(end_date);
        lf = lf.filter(
            col("datetime")
                .cast(DataType::String)
                .lt_eq(lit(end)),
        );
    }

    lf.collect()
        .map_err(|e| AppError::Internal(format!("date filter: {}", e)))
}

/// Filter candles by date range.
pub fn filter_candles_by_date(
    candles: &[Candle],
    start_date: &str,
    end_date: &str,
) -> Vec<Candle> {
    if start_date.is_empty() && end_date.is_empty() {
        return candles.to_vec();
    }
    let end_normalized = normalize_end_date(end_date);
    candles
        .iter()
        .filter(|c| {
            (start_date.is_empty() || c.datetime.as_str() >= start_date)
                && (end_date.is_empty() || c.datetime.as_str() <= end_normalized.as_str())
        })
        .cloned()
        .collect()
}

/// Load TickColumns from flat binary tick files (`tick_raw_YYYY.bin`).
///
/// Binary format: `i64_le timestamp_µs (8B) + f64_le bid (8B) + f64_le ask (8B)` = 24 bytes/tick.
/// Files are sorted chronologically and filtered by year from `start_date`/`end_date`.
/// Date filtering is applied at the record level for precise range slicing.
pub fn tick_columns_from_binary_dir(
    tick_raw_path: &str,
    start_date: &str,
    end_date: &str,
) -> Result<TickColumns, AppError> {
    use std::io::Read as IoRead;

    let dir = std::path::Path::new(tick_raw_path);
    if !dir.is_dir() {
        return Err(AppError::FileNotFound(format!(
            "Binary tick directory not found: {}",
            tick_raw_path
        )));
    }

    // Parse year range from dates to skip irrelevant files
    let parse_year = |s: &str| -> Option<i32> {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S"))
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(
                &format!("{} 00:00:00", &s[..10.min(s.len())]), "%Y-%m-%d %H:%M:%S"))
            .ok()
            .map(|ndt| ndt.year())
    };

    let start_year = if start_date.is_empty() { None } else { parse_year(start_date) };
    let end_year = if end_date.is_empty() { None } else { parse_year(end_date) };

    // Discover .bin files, filter by year range, sort chronologically
    let mut bin_files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| AppError::Internal(format!("read binary tick dir: {}", e)))?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".bin"))
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(year_str) = name.strip_suffix(".bin")
                .and_then(|s| s.rsplit('_').next())
            {
                if let Ok(file_year) = year_str.parse::<i32>() {
                    let after_start = start_year.map_or(true, |sy| file_year >= sy);
                    let before_end = end_year.map_or(true, |ey| file_year <= ey);
                    return after_start && before_end;
                }
            }
            true
        })
        .map(|e| e.path())
        .collect();
    bin_files.sort();

    if bin_files.is_empty() {
        return Err(AppError::NotFound(
            "No binary tick files found for the requested date range".into(),
        ));
    }

    // Compute microsecond bounds for date filtering
    let start_micros: i64 = if start_date.is_empty() {
        i64::MIN
    } else {
        parse_datetime_to_micros(&format!("{} 00:00:00", &start_date[..10.min(start_date.len())]))
    };
    let end_micros: i64 = if end_date.is_empty() {
        i64::MAX
    } else {
        parse_datetime_to_micros(&format!("{} 23:59:59.999999", &end_date[..10.min(end_date.len())]))
    };

    // Estimate capacity: each file is ~24 bytes/tick
    let total_bytes: u64 = bin_files.iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    let capacity = ((total_bytes / 24) as usize).min(500_000_000);

    let mut timestamps = Vec::with_capacity(capacity);
    let mut bids = Vec::with_capacity(capacity);
    let mut asks = Vec::with_capacity(capacity);

    let mut chunk = [0u8; 24];

    for path in &bin_files {
        let file = std::fs::File::open(path)
            .map_err(|e| AppError::FileRead(format!("open binary tick file: {}", e)))?;
        let mut reader = std::io::BufReader::with_capacity(512 * 1024, file);

        loop {
            match reader.read_exact(&mut chunk) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(AppError::FileRead(format!("read binary tick: {}", e))),
            }

            let ts = i64::from_le_bytes(chunk[0..8].try_into().unwrap());

            // Apply date filter
            if ts < start_micros || ts > end_micros {
                continue;
            }

            let bid = f64::from_le_bytes(chunk[8..16].try_into().unwrap());
            let ask = f64::from_le_bytes(chunk[16..24].try_into().unwrap());

            timestamps.push(ts);
            bids.push(bid);
            asks.push(ask);
        }
    }

    info!(
        "Loaded {} ticks from binary (SoA, ~{}MB)",
        timestamps.len(),
        (timestamps.len() * 24) / (1024 * 1024)
    );

    Ok(TickColumns { timestamps, bids, asks })
}
