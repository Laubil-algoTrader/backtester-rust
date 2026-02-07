use std::sync::atomic::{AtomicBool, Ordering};

use polars::prelude::*;
use tracing::info;

use crate::errors::AppError;
use crate::models::candle::Candle;
use crate::models::config::InstrumentConfig;
use crate::models::result::{BacktestResults, DrawdownPoint, EquityPoint};
use crate::models::strategy::{
    BacktestConfig, IndicatorConfig, IndicatorType, Strategy, TradeDirection,
};
use crate::models::trade::{CloseReason, TradeResult};

use super::metrics::calculate_metrics;
use super::orders;
use super::position::{
    calculate_lots, calculate_stop_loss, calculate_take_profit,
    calculate_trailing_stop_distance, check_sl_tp_hit, update_mae_mfe, update_trailing_stop,
    OpenPosition,
};
use super::strategy::{evaluate_rules, max_lookback, pre_compute_indicators};

/// Run a complete backtest.
pub fn run_backtest(
    candles: &[Candle],
    strategy: &Strategy,
    config: &BacktestConfig,
    instrument: &InstrumentConfig,
    cancel_flag: &AtomicBool,
    progress_callback: impl Fn(u8, usize, usize),
) -> Result<BacktestResults, AppError> {
    let total_bars = candles.len();
    info!("Starting backtest: {} bars, strategy={}", total_bars, strategy.name);

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
    let mut position: Option<OpenPosition> = None;
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

        // ── 1. Update open position ──
        if let Some(ref mut pos) = position {
            // Update trailing stop
            update_trailing_stop(pos, candle);

            // Update MAE/MFE
            update_mae_mfe(pos, candle, instrument);

            // Check SL/TP hit
            if let Some((exit_price, reason)) = check_sl_tp_hit(pos, candle) {
                let trade = close_position(pos, exit_price, candle, i, reason, instrument, strategy);
                equity += trade.pnl - trade.commission;
                trades.push(trade);
                position = None;
            }
            // Check exit rules
            else if evaluate_rules(&strategy.exit_rules, i, &cache, candles) {
                let exit_price = candle.close;
                let trade =
                    close_position(pos, exit_price, candle, i, CloseReason::Signal, instrument, strategy);
                equity += trade.pnl - trade.commission;
                trades.push(trade);
                position = None;
            }
        }

        // ── 2. Open new position if no position open ──
        if position.is_none() && evaluate_rules(&strategy.entry_rules, i, &cache, candles) {
            // Determine direction: default to Long for "Both"
            let direction = if can_go_long {
                TradeDirection::Long
            } else if can_go_short {
                TradeDirection::Short
            } else {
                continue;
            };

            let atr_val = atr_values.as_ref().and_then(|v| {
                if i < v.len() && !v[i].is_nan() {
                    Some(v[i])
                } else {
                    None
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
        }

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
            last_candle,
            total_bars - 1,
            CloseReason::EndOfData,
            instrument,
            strategy,
        );
        let _ = trade.pnl - trade.commission; // equity no longer tracked after final close
        trades.push(trade);
    }

    progress_callback(100, total_bars, total_bars);
    info!("Backtest complete: {} trades", trades.len());

    // ── 5. Calculate metrics ──
    let metrics = calculate_metrics(&trades, &equity_curve, config.initial_capital);

    let returns: Vec<f64> = trades.iter().map(|t| t.pnl).collect();

    Ok(BacktestResults {
        trades,
        equity_curve,
        drawdown_curve,
        returns,
        metrics,
    })
}

/// Close a position and create a TradeResult.
fn close_position(
    pos: &OpenPosition,
    exit_price: f64,
    exit_candle: &Candle,
    exit_bar: usize,
    reason: CloseReason,
    instrument: &InstrumentConfig,
    strategy: &Strategy,
) -> TradeResult {
    let pnl = orders::calculate_pnl(pos.direction, pos.entry_price, exit_price, pos.lots, instrument);
    let pnl_pips =
        orders::calculate_pnl_pips(pos.direction, pos.entry_price, exit_price, instrument);
    let commission =
        orders::calculate_commission(&strategy.trading_costs, pos.lots, pos.entry_price, instrument);
    let duration_bars = exit_bar - pos.entry_bar;

    TradeResult {
        id: uuid::Uuid::new_v4().to_string(),
        direction: pos.direction,
        entry_time: pos.entry_time.clone(),
        entry_price: pos.entry_price,
        exit_time: exit_candle.datetime.clone(),
        exit_price,
        lots: pos.lots,
        pnl,
        pnl_pips,
        commission,
        close_reason: reason,
        duration_bars,
        duration_time: format_duration_bars(duration_bars),
        mae: pos.mae_pips,
        mfe: pos.mfe_pips,
    }
}

/// Format duration in bars to a human-readable string.
fn format_duration_bars(bars: usize) -> String {
    if bars < 60 {
        format!("{}m", bars)
    } else if bars < 1440 {
        format!("{}h {}m", bars / 60, bars % 60)
    } else {
        format!("{}d {}h", bars / 1440, (bars % 1440) / 60)
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

/// Convert a Polars DataFrame to a Vec<Candle>.
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
    let open = open_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;
    let high = high_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;
    let low = low_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;
    let close = close_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;
    let volume = volume_col.f64().map_err(|e| AppError::Internal(e.to_string()))?;

    let mut candles = Vec::with_capacity(len);
    for i in 0..len {
        let dt_str = format!("{}", datetime_col.get(i).map_err(|e| AppError::Internal(e.to_string()))?);
        candles.push(Candle {
            datetime: dt_str,
            open: open.get(i).unwrap_or(0.0),
            high: high.get(i).unwrap_or(0.0),
            low: low.get(i).unwrap_or(0.0),
            close: close.get(i).unwrap_or(0.0),
            volume: volume.get(i).unwrap_or(0.0),
        });
    }

    Ok(candles)
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
