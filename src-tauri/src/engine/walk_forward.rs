use std::sync::atomic::{AtomicBool, Ordering};

use tracing::info;

use crate::errors::AppError;
use crate::models::candle::Candle;
use crate::models::config::InstrumentConfig;
use crate::models::result::{
    BacktestMetrics, OptimizationMethod, WalkForwardConfig, WalkForwardResult,
    WalkForwardWindowResult,
};

use super::executor::{run_backtest, SubBarData};
use super::optimizer::{apply_params, run_genetic_algorithm, run_grid_search};

/// Run a Walk-Forward Analysis.
///
/// Divides `candles` into `config.num_windows` sequential windows.
/// For each window:
///   - In-sample portion (config.in_sample_pct): optimize parameters.
///   - Out-of-sample portion: test the best found parameters (no fitting).
///
/// This measures how well optimized parameters generalize to unseen data.
/// An `efficiency_ratio` ≥ 0.5 suggests the strategy is robust.
pub fn run_walk_forward(
    candles: &[Candle],
    strategy: &crate::models::strategy::Strategy,
    config: &WalkForwardConfig,
    instrument: &InstrumentConfig,
    cancel_flag: &AtomicBool,
    progress_callback: impl Fn(u8, usize, usize),
) -> Result<WalkForwardResult, AppError> {
    let total_bars = candles.len();

    if total_bars < 200 {
        return Err(AppError::InsufficientData {
            needed: 200,
            available: total_bars,
        });
    }

    let num_windows = config.num_windows;
    if num_windows < 2 {
        return Err(AppError::InvalidConfig(
            "num_windows must be at least 2".into(),
        ));
    }

    let in_sample_pct = config.in_sample_pct.clamp(0.1, 0.9);
    let window_size = total_bars / num_windows;
    let in_sample_bars = (window_size as f64 * in_sample_pct).round() as usize;
    let out_of_sample_bars = window_size - in_sample_bars;

    if in_sample_bars < 50 {
        return Err(AppError::InvalidConfig(format!(
            "In-sample window too small ({} bars). Use fewer windows or more data.",
            in_sample_bars
        )));
    }
    if out_of_sample_bars < 20 {
        return Err(AppError::InvalidConfig(format!(
            "Out-of-sample window too small ({} bars). Use fewer windows or more data.",
            out_of_sample_bars
        )));
    }

    info!(
        "Walk-forward: {} windows, {}/{} in/out-of-sample bars each",
        num_windows, in_sample_bars, out_of_sample_bars
    );

    let opt_config = &config.optimization_config;
    // Walk-forward always uses SelectedTfOnly mode (fastest — statistical analysis)
    let sub_bars = SubBarData::None;

    let mut windows: Vec<WalkForwardWindowResult> = Vec::with_capacity(num_windows);

    for window_idx in 0..num_windows {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::OptimizationCancelled);
        }

        let start = window_idx * window_size;
        let in_sample_end = start + in_sample_bars;
        let out_of_sample_end = (in_sample_end + out_of_sample_bars).min(total_bars);

        let in_sample = &candles[start..in_sample_end];
        let out_of_sample = &candles[in_sample_end..out_of_sample_end];

        if out_of_sample.is_empty() {
            break;
        }

        info!(
            "WFA window {}/{}: IS={} bars, OOS={} bars",
            window_idx + 1,
            num_windows,
            in_sample.len(),
            out_of_sample.len()
        );

        // ── Optimize on in-sample ──
        let opt_results = match opt_config.method {
            OptimizationMethod::GridSearch => run_grid_search(
                in_sample,
                &sub_bars,
                strategy,
                &opt_config.backtest_config,
                instrument,
                &opt_config.parameter_ranges,
                &opt_config.objectives,
                cancel_flag,
                |_, _, _, _| {},
            )?,
            OptimizationMethod::GeneticAlgorithm => {
                let ga_cfg = opt_config.ga_config.as_ref().ok_or_else(|| {
                    AppError::InvalidConfig(
                        "GeneticAlgorithmConfig required for walk-forward GA mode".into(),
                    )
                })?;
                run_genetic_algorithm(
                    in_sample,
                    &sub_bars,
                    strategy,
                    &opt_config.backtest_config,
                    instrument,
                    &opt_config.parameter_ranges,
                    &opt_config.objectives,
                    ga_cfg,
                    cancel_flag,
                    |_, _, _, _| {},
                )?
            }
        };

        let best = opt_results
            .first()
            .ok_or_else(|| AppError::OptimizationError(format!("No results for window {}", window_idx)))?;

        // Reconstruct parameter vector in range order (HashMap → Vec<f64>)
        let best_values: Vec<f64> = opt_config
            .parameter_ranges
            .iter()
            .map(|range| best.params.get(&range.display_name).copied().unwrap_or(range.min))
            .collect();

        let best_strategy = apply_params(strategy, &opt_config.parameter_ranges, &best_values);

        // ── Evaluate best params on in-sample (for efficiency ratio) ──
        let is_bt = run_backtest(
            in_sample,
            &sub_bars,
            &best_strategy,
            &opt_config.backtest_config,
            instrument,
            cancel_flag,
            |_, _, _| {},
        )?;

        // ── Evaluate best params on out-of-sample ──
        let oos_bt = run_backtest(
            out_of_sample,
            &sub_bars,
            &best_strategy,
            &opt_config.backtest_config,
            instrument,
            cancel_flag,
            |_, _, _| {},
        )?;

        windows.push(WalkForwardWindowResult {
            window_index: window_idx,
            in_sample_start: in_sample
                .first()
                .map(|c| c.datetime.clone())
                .unwrap_or_default(),
            in_sample_end: in_sample
                .last()
                .map(|c| c.datetime.clone())
                .unwrap_or_default(),
            out_of_sample_start: out_of_sample
                .first()
                .map(|c| c.datetime.clone())
                .unwrap_or_default(),
            out_of_sample_end: out_of_sample
                .last()
                .map(|c| c.datetime.clone())
                .unwrap_or_default(),
            best_params: best.params.clone(),
            in_sample_metrics: is_bt.metrics,
            out_of_sample_metrics: oos_bt.metrics,
        });

        let pct = (((window_idx + 1) as f64 / num_windows as f64) * 100.0) as u8;
        progress_callback(pct, window_idx + 1, num_windows);
    }

    let combined = combine_oos_metrics(&windows);
    let efficiency_ratio = compute_efficiency_ratio(&windows);

    info!(
        "Walk-forward complete: {} windows, efficiency_ratio={:.2}",
        windows.len(),
        efficiency_ratio
    );

    Ok(WalkForwardResult {
        windows,
        combined_out_of_sample_metrics: combined,
        efficiency_ratio,
    })
}

// ══════════════════════════════════════════════════════════════
// Helpers
// ══════════════════════════════════════════════════════════════

/// Compute the WFA efficiency ratio: OOS net profit / IS net profit.
/// Values ≥ 0.5 suggest the strategy is robust (OOS captures at least half of IS returns).
fn compute_efficiency_ratio(windows: &[WalkForwardWindowResult]) -> f64 {
    let is_total: f64 = windows.iter().map(|w| w.in_sample_metrics.net_profit).sum();
    let oos_total: f64 = windows
        .iter()
        .map(|w| w.out_of_sample_metrics.net_profit)
        .sum();

    if is_total.abs() > f64::EPSILON {
        oos_total / is_total
    } else {
        0.0
    }
}

/// Aggregate out-of-sample metrics across all windows into a combined summary.
fn combine_oos_metrics(windows: &[WalkForwardWindowResult]) -> BacktestMetrics {
    if windows.is_empty() {
        return default_metrics();
    }

    let n = windows.len() as f64;
    let total_trades: usize = windows.iter().map(|w| w.out_of_sample_metrics.total_trades).sum();
    let winning_trades: usize = windows.iter().map(|w| w.out_of_sample_metrics.winning_trades).sum();
    let losing_trades: usize = windows.iter().map(|w| w.out_of_sample_metrics.losing_trades).sum();
    let gross_profit: f64 = windows.iter().map(|w| w.out_of_sample_metrics.gross_profit).sum();
    let gross_loss: f64 = windows.iter().map(|w| w.out_of_sample_metrics.gross_loss).sum();
    let net_profit: f64 = windows.iter().map(|w| w.out_of_sample_metrics.net_profit).sum();
    let avg_sharpe = windows.iter().map(|w| w.out_of_sample_metrics.sharpe_ratio).sum::<f64>() / n;
    let max_dd = windows
        .iter()
        .map(|w| w.out_of_sample_metrics.max_drawdown_pct)
        .fold(f64::NEG_INFINITY, f64::max);

    let win_rate = if total_trades > 0 {
        winning_trades as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };
    let profit_factor = if gross_loss.abs() > f64::EPSILON {
        gross_profit / gross_loss.abs()
    } else if gross_profit > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };

    BacktestMetrics {
        final_capital: 0.0,
        total_return_pct: net_profit,
        annualized_return_pct: 0.0,
        monthly_return_avg_pct: 0.0,
        sharpe_ratio: avg_sharpe,
        sortino_ratio: 0.0,
        calmar_ratio: 0.0,
        max_drawdown_pct: if max_dd == f64::NEG_INFINITY { 0.0 } else { max_dd },
        max_drawdown_duration_bars: 0,
        max_drawdown_duration_time: String::new(),
        avg_drawdown_pct: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.avg_drawdown_pct)
            .sum::<f64>()
            / n,
        recovery_factor: 0.0,
        total_trades,
        winning_trades,
        losing_trades,
        breakeven_trades: 0,
        win_rate_pct: win_rate,
        gross_profit,
        gross_loss,
        net_profit,
        profit_factor,
        avg_trade: if total_trades > 0 { net_profit / total_trades as f64 } else { 0.0 },
        avg_win: if winning_trades > 0 { gross_profit / winning_trades as f64 } else { 0.0 },
        avg_loss: if losing_trades > 0 { gross_loss / losing_trades as f64 } else { 0.0 },
        largest_win: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.largest_win)
            .fold(f64::NEG_INFINITY, f64::max),
        largest_loss: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.largest_loss)
            .fold(f64::INFINITY, f64::min),
        expectancy: if total_trades > 0 { net_profit / total_trades as f64 } else { 0.0 },
        max_consecutive_wins: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.max_consecutive_wins)
            .max()
            .unwrap_or(0),
        max_consecutive_losses: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.max_consecutive_losses)
            .max()
            .unwrap_or(0),
        avg_consecutive_wins: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.avg_consecutive_wins)
            .sum::<f64>()
            / n,
        avg_consecutive_losses: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.avg_consecutive_losses)
            .sum::<f64>()
            / n,
        avg_trade_duration: String::new(),
        avg_bars_in_trade: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.avg_bars_in_trade)
            .sum::<f64>()
            / n,
        avg_winner_duration: String::new(),
        avg_loser_duration: String::new(),
        mae_avg: windows.iter().map(|w| w.out_of_sample_metrics.mae_avg).sum::<f64>() / n,
        mae_max: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.mae_max)
            .fold(f64::NEG_INFINITY, f64::max),
        mfe_avg: windows.iter().map(|w| w.out_of_sample_metrics.mfe_avg).sum::<f64>() / n,
        mfe_max: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.mfe_max)
            .fold(f64::NEG_INFINITY, f64::max),
        stagnation_bars: 0,
        stagnation_time: String::new(),
        ulcer_index_pct: windows
            .iter()
            .map(|w| w.out_of_sample_metrics.ulcer_index_pct)
            .sum::<f64>()
            / n,
        return_dd_ratio: if max_dd > 0.0 { net_profit / max_dd } else { 0.0 },
        k_ratio: windows.iter().map(|w| w.out_of_sample_metrics.k_ratio).sum::<f64>() / n,
        omega_ratio: windows.iter().map(|w| w.out_of_sample_metrics.omega_ratio).sum::<f64>() / n,
        monthly_returns: vec![],
        total_swap_charged: windows.iter().map(|w| w.out_of_sample_metrics.total_swap_charged).sum(),
        total_commission_charged: windows.iter().map(|w| w.out_of_sample_metrics.total_commission_charged).sum(),
    }
}

fn default_metrics() -> BacktestMetrics {
    BacktestMetrics {
        final_capital: 0.0,
        total_return_pct: 0.0,
        annualized_return_pct: 0.0,
        monthly_return_avg_pct: 0.0,
        sharpe_ratio: 0.0,
        sortino_ratio: 0.0,
        calmar_ratio: 0.0,
        max_drawdown_pct: 0.0,
        max_drawdown_duration_bars: 0,
        max_drawdown_duration_time: String::new(),
        avg_drawdown_pct: 0.0,
        recovery_factor: 0.0,
        total_trades: 0,
        winning_trades: 0,
        losing_trades: 0,
        breakeven_trades: 0,
        win_rate_pct: 0.0,
        gross_profit: 0.0,
        gross_loss: 0.0,
        net_profit: 0.0,
        profit_factor: 0.0,
        avg_trade: 0.0,
        avg_win: 0.0,
        avg_loss: 0.0,
        largest_win: 0.0,
        largest_loss: 0.0,
        expectancy: 0.0,
        max_consecutive_wins: 0,
        max_consecutive_losses: 0,
        avg_consecutive_wins: 0.0,
        avg_consecutive_losses: 0.0,
        avg_trade_duration: String::new(),
        avg_bars_in_trade: 0.0,
        avg_winner_duration: String::new(),
        avg_loser_duration: String::new(),
        mae_avg: 0.0,
        mae_max: 0.0,
        mfe_avg: 0.0,
        mfe_max: 0.0,
        stagnation_bars: 0,
        stagnation_time: String::new(),
        ulcer_index_pct: 0.0,
        return_dd_ratio: 0.0,
        k_ratio: 0.0,
        omega_ratio: 0.0,
        monthly_returns: vec![],
        total_swap_charged: 0.0,
        total_commission_charged: 0.0,
    }
}
