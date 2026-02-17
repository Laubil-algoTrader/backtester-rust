use std::sync::atomic::{AtomicBool, Ordering};

use polars::prelude::*;
use tracing::info;

use crate::errors::AppError;
use crate::models::candle::{Candle, TickColumns};
use crate::models::config::InstrumentConfig;
use crate::models::result::{BacktestResults, DrawdownPoint, EquityPoint};
use crate::models::strategy::{
    BacktestConfig, CloseTradesAt, IndicatorConfig, IndicatorType, Strategy, TradeDirection,
    TradingHours,
};
use crate::models::trade::{CloseReason, TradeResult};

use super::metrics::calculate_metrics;
use super::orders;
use super::position::{
    calculate_lots, calculate_stop_loss, calculate_take_profit,
    calculate_trailing_stop_distance, check_sl_tp_hit,
    update_mae_mfe, update_trailing_stop,
    OpenPosition,
};
use super::strategy::{evaluate_rules, max_lookback, pre_compute_indicators};

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
    /// ~10x faster than Vec<TickData> due to eliminated String allocations,
    /// i64 comparisons, and better cache locality.
    Ticks(TickColumns),
}

// ══════════════════════════════════════════════════════════════
// Main backtest
// ══════════════════════════════════════════════════════════════

/// Run a complete backtest.
pub fn run_backtest(
    candles: &[Candle],
    sub_bars: &SubBarData,
    strategy: &Strategy,
    config: &BacktestConfig,
    instrument: &InstrumentConfig,
    cancel_flag: &AtomicBool,
    progress_callback: impl Fn(u8, usize, usize),
) -> Result<BacktestResults, AppError> {
    let total_bars = candles.len();
    info!("Starting backtest: {} bars, strategy={}, precision={:?}",
        total_bars, strategy.name, config.precision);

    if total_bars == 0 {
        return Err(AppError::NoDataInRange);
    }

    // Pre-compute all indicators
    let cache = pre_compute_indicators(strategy, candles)?;

    // Get ATR values if needed for SL/TP/trailing stop
    let atr_values = compute_atr_if_needed(strategy, candles);

    let lookback = max_lookback(strategy);
    let start_bar = lookback.min(total_bars);

    let mut equity = config.initial_capital;
    let mut peak_equity = equity;
    let mut position: Option<OpenPosition> = Option::None;
    let mut trades: Vec<TradeResult> = Vec::new();
    let mut equity_curve: Vec<EquityPoint> = Vec::with_capacity(total_bars);
    let mut drawdown_curve: Vec<DrawdownPoint> = Vec::with_capacity(total_bars);

    // Determine allowed trade direction
    let can_go_long = matches!(
        strategy.trade_direction,
        TradeDirection::Long | TradeDirection::Both
    );
    let can_go_short = matches!(
        strategy.trade_direction,
        TradeDirection::Short | TradeDirection::Both
    );

    // Sub-bar cursor for O(n+m) range lookups
    let mut sub_cursor: usize = 0;

    // Daily trade tracking
    let mut daily_trade_count: usize = 0;
    let mut current_date = String::new();

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

        // ── 1. Update open position ──
        if let Some(ref mut pos) = position {
            // Resolve SL/TP exit using sub-bar data (or TF candle for SelectedTfOnly)
            let exit_result = resolve_exit(
                pos, candle, sub_bars, sub_start, sub_end, instrument,
            );

            if let Some((exit_price, exit_time, reason)) = exit_result {
                let trade = close_position(
                    pos, exit_price, &exit_time, i, reason, instrument, strategy, config,
                );
                equity += trade.pnl - trade.commission;
                trades.push(trade);
                position = Option::None;
            }
            // Check force-close at specified time
            else if should_close_at_time(&strategy.close_trades_at, &candle.datetime) {
                let exit_price = candle.close;
                let trade = close_position(
                    pos, exit_price, &candle.datetime, i, CloseReason::TimeClose, instrument, strategy, config,
                );
                equity += trade.pnl - trade.commission;
                trades.push(trade);
                position = Option::None;
            }
            // Check direction-specific exit rules (always evaluated on TF candle)
            else if {
                let exit_rules = match pos.direction {
                    TradeDirection::Long => &strategy.long_exit_rules,
                    TradeDirection::Short => &strategy.short_exit_rules,
                    TradeDirection::Both => &strategy.long_exit_rules,
                };
                evaluate_rules(exit_rules, i, &cache, candles)
            } {
                let exit_price = candle.close;
                let trade = close_position(
                    pos, exit_price, &candle.datetime, i, CloseReason::Signal, instrument, strategy, config,
                );
                equity += trade.pnl - trade.commission;
                trades.push(trade);
                position = Option::None;
            } else {
                // Position survived — for SelectedTfOnly, update trailing stop
                if matches!(sub_bars, SubBarData::None) {
                    update_trailing_stop(pos, candle);
                }
                // For sub-bar modes, trailing stop was already updated in resolve_exit
            }
        }

        // ── 2. Open new position if no position open ──
        // Check trading hours and daily trade limit before evaluating entry rules
        if position.is_none() {
            // Date tracking for daily trade limit
            let bar_date = &candle.datetime[..10];
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

            // Determine direction from direction-specific entry rules
            let direction = if within_hours && under_daily_limit {
                if can_go_long
                    && !strategy.long_entry_rules.is_empty()
                    && evaluate_rules(&strategy.long_entry_rules, i, &cache, candles)
                {
                    Some(TradeDirection::Long)
                } else if can_go_short
                    && !strategy.short_entry_rules.is_empty()
                    && evaluate_rules(&strategy.short_entry_rules, i, &cache, candles)
                {
                    Some(TradeDirection::Short)
                } else {
                    None
                }
            } else {
                None
            };

        if let Some(direction) = direction {
            let atr_val = atr_values.as_ref().and_then(|v| {
                if i < v.len() && !v[i].is_nan() {
                    Some(v[i])
                } else {
                    Option::None
                }
            });

            // Apply entry costs
            let raw_price = candle.close;
            let entry_price =
                orders::apply_entry_costs(raw_price, direction, &strategy.trading_costs, instrument);

            // Calculate SL
            let sl_price = strategy.stop_loss.as_ref().map(|sl_cfg| {
                calculate_stop_loss(sl_cfg, entry_price, direction, atr_val, instrument)
            });

            // Calculate lots
            let lots = calculate_lots(
                &strategy.position_sizing,
                equity,
                entry_price,
                sl_price,
                instrument,
            );

            // Calculate TP
            let tp_price = strategy.take_profit.as_ref().map(|tp_cfg| {
                calculate_take_profit(tp_cfg, entry_price, sl_price, direction, atr_val, instrument)
            });

            // Calculate trailing stop distance
            let ts_distance = strategy.trailing_stop.as_ref().map(|ts_cfg| {
                calculate_trailing_stop_distance(ts_cfg, entry_price, sl_price, atr_val, instrument)
            });

            position = Some(OpenPosition {
                direction,
                entry_price,
                entry_bar: i,
                entry_time: candle.datetime.clone(),
                lots,
                stop_loss: sl_price,
                take_profit: tp_price,
                trailing_stop_distance: ts_distance,
                highest_since_entry: candle.high,
                lowest_since_entry: candle.low,
                mae_pips: 0.0,
                mfe_pips: 0.0,
            });
            daily_trade_count += 1;
        } // if let Some(direction)
        } // if position.is_none()

        // ── 3. Record equity and drawdown ──
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
        let _ = trade.pnl - trade.commission;
        trades.push(trade);
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
/// Returns (exit_price, exit_time, reason) if an exit is triggered.
fn resolve_exit(
    pos: &mut OpenPosition,
    candle: &Candle,
    sub_bars: &SubBarData,
    sub_start: usize,
    sub_end: usize,
    instrument: &InstrumentConfig,
) -> Option<(f64, String, CloseReason)> {
    match sub_bars {
        SubBarData::None => {
            // SelectedTfOnly: check against TF candle OHLC
            update_mae_mfe(pos, candle, instrument);
            check_sl_tp_hit(pos, candle)
                .map(|(price, reason)| (price, candle.datetime.clone(), reason))
        }
        SubBarData::Candles(subs) => {
            // M1TickSimulation: iterate M1 sub-candles
            process_subbars_candle(pos, subs, sub_start, sub_end, instrument)
        }
        SubBarData::Ticks(ticks) => {
            // RealTick modes: optimized columnar tick processing
            process_subbars_tick_columnar(pos, ticks, sub_start, sub_end, instrument)
        }
    }
}

/// Process M1 sub-candles for SL/TP resolution within a TF bar.
/// Updates MAE/MFE and trailing stop on each sub-candle.
fn process_subbars_candle(
    pos: &mut OpenPosition,
    sub_candles: &[Candle],
    start: usize,
    end: usize,
    instrument: &InstrumentConfig,
) -> Option<(f64, String, CloseReason)> {
    for i in start..end {
        let sc = &sub_candles[i];
        // Update MAE/MFE
        update_mae_mfe(pos, sc, instrument);
        // Check SL/TP (with current trailing stop level)
        if let Some((exit_price, reason)) = check_sl_tp_hit(pos, sc) {
            return Some((exit_price, sc.datetime.clone(), reason));
        }
        // Update trailing stop for next sub-bar
        update_trailing_stop(pos, sc);
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
                    return Some((bid, micros_to_datetime_string(timestamps[j]), CloseReason::StopLoss));
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
                    match pos.stop_loss {
                        Some(ref mut sl) if new_sl > *sl => *sl = new_sl,
                        None => pos.stop_loss = Some(new_sl),
                        _ => {}
                    }
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
                    return Some((ask, micros_to_datetime_string(timestamps[j]), CloseReason::StopLoss));
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
                    match pos.stop_loss {
                        Some(ref mut sl) if new_sl < *sl => *sl = new_sl,
                        None => pos.stop_loss = Some(new_sl),
                        _ => {}
                    }
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

/// Compute ATR values if the strategy uses ATR-based SL, TP, or trailing stop.
/// Uses the ATR period configured on the first ATR-based component found.
fn compute_atr_if_needed(strategy: &Strategy, candles: &[Candle]) -> Option<Vec<f64>> {
    // Collect ATR period from whichever component uses ATR (first found wins)
    let atr_period = strategy
        .stop_loss
        .as_ref()
        .filter(|sl| matches!(sl.sl_type, crate::models::strategy::StopLossType::ATR))
        .and_then(|sl| sl.atr_period)
        .or_else(|| {
            strategy
                .take_profit
                .as_ref()
                .filter(|tp| matches!(tp.tp_type, crate::models::strategy::TakeProfitType::ATR))
                .and_then(|tp| tp.atr_period)
        })
        .or_else(|| {
            strategy
                .trailing_stop
                .as_ref()
                .filter(|ts| matches!(ts.ts_type, crate::models::strategy::TrailingStopType::ATR))
                .and_then(|ts| ts.atr_period)
        });

    if let Some(period) = atr_period {
        let config = IndicatorConfig {
            indicator_type: IndicatorType::ATR,
            params: crate::models::strategy::IndicatorParams {
                period: Some(period),
                ..Default::default()
            },
            output_field: None,
        };
        match super::indicators::compute_indicator(&config, candles) {
            Ok(output) => Some(output.primary),
            Err(_) => None,
        }
    } else {
        None
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
        // Fast path: no nulls → direct memcpy from Arrow buffer
        let arr = rechunked.downcast_iter().next().unwrap();
        arr.values().as_slice().to_vec()
    } else {
        // Slow path with null handling (rare for our data)
        (0..rechunked.len()).map(|i| rechunked.get(i).unwrap_or(0.0)).collect()
    }
}

/// Fast bulk extraction of i64 values from an Int64 ChunkedArray.
/// Rechunks into a single contiguous chunk, then copies via memcpy.
/// Nulls are replaced with 0.
fn chunked_i64_to_vec(ca: &polars::prelude::Int64Chunked) -> Vec<i64> {
    let rechunked = ca.rechunk();
    if !rechunked.has_nulls() {
        // Fast path: no nulls → direct memcpy from Arrow buffer
        let arr = rechunked.downcast_iter().next().unwrap();
        arr.values().as_slice().to_vec()
    } else {
        // Slow path with null handling (rare for our data)
        (0..rechunked.len()).map(|i| rechunked.get(i).unwrap_or(0)).collect()
    }
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
        lf = lf.filter(
            col("datetime")
                .cast(DataType::String)
                .lt_eq(lit(end_date)),
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
    candles
        .iter()
        .filter(|c| {
            (start_date.is_empty() || c.datetime.as_str() >= start_date)
                && (end_date.is_empty() || c.datetime.as_str() <= end_date)
        })
        .cloned()
        .collect()
}
