use std::sync::atomic::{AtomicBool, Ordering};

use rand::Rng;
use rayon::prelude::*;

use crate::models::result::{MonteCarloConfig, MonteCarloConfidenceRow, MonteCarloResult};
use crate::models::trade::TradeResult;

/// Maximum number of simulation equity curves returned for visualization.
const MAX_DISPLAY_CURVES: usize = 200;

/// Maximum number of points per equity curve (downsampled if the trade count is higher).
const MAX_CURVE_POINTS: usize = 300;

/// Confidence levels shown in the results table (mirrors StrategyQuant X).
const CONFIDENCE_LEVELS: &[f64] = &[50.0, 60.0, 70.0, 80.0, 90.0, 92.0, 95.0, 97.0, 98.0];

// ── helpers ──────────────────────────────────────────────────────────────────

/// Downsample a curve to at most `max_points` by taking evenly spaced indices.
fn downsample(curve: &[f64], max_points: usize) -> Vec<f64> {
    if curve.len() <= max_points {
        return curve.to_vec();
    }
    let step = curve.len() as f64 / max_points as f64;
    (0..max_points)
        .map(|i| curve[(i as f64 * step) as usize])
        .collect()
}

/// Compute the p-th percentile (0–100) of a **pre-sorted ascending** slice
/// using linear interpolation.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = (p / 100.0) * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = (lower + 1).min(sorted.len() - 1);
    let frac = rank - lower as f64;
    sorted[lower] + frac * (sorted[upper] - sorted[lower])
}

/// Build the confidence table from collected simulation metrics.
///
/// For profit-like metrics (net_profit, ret_dd_ratio, expectancy), confidence C%
/// means "only (100−C)% chance of being worse", so we take the (100−C)th percentile
/// of the ascending-sorted values (the pessimistic tail).
///
/// For drawdown (lower is better), confidence C% means "only (100−C)% chance of
/// being worse (higher)", so we take the C-th percentile of the ascending-sorted
/// drawdown values (the worst-case tail).
fn build_confidence_table(
    mut net_profits: Vec<f64>,
    mut drawdowns_abs: Vec<f64>,
    mut ret_dd_ratios: Vec<f64>,
    mut expectancies: Vec<f64>,
) -> Vec<MonteCarloConfidenceRow> {
    net_profits.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    drawdowns_abs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ret_dd_ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    expectancies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    CONFIDENCE_LEVELS
        .iter()
        .map(|&conf| {
            let pessimistic_p = 100.0 - conf; // e.g. 95% → 5th pct
            MonteCarloConfidenceRow {
                level: conf,
                net_profit: percentile(&net_profits, pessimistic_p),
                max_drawdown_abs: percentile(&drawdowns_abs, conf), // drawdown: higher = worse
                ret_dd_ratio: percentile(&ret_dd_ratios, pessimistic_p),
                expectancy: percentile(&expectancies, pessimistic_p),
            }
        })
        .collect()
}

/// Returns an empty result used when inputs are invalid or all simulations were cancelled.
fn empty_result() -> MonteCarloResult {
    MonteCarloResult {
        n_simulations: 0,
        ruin_probability: 0.0,
        original_net_profit: 0.0,
        original_max_drawdown_abs: 0.0,
        original_ret_dd_ratio: 0.0,
        original_expectancy: 0.0,
        original_return_pct: 0.0,
        original_max_drawdown_pct: 0.0,
        confidence_table: vec![],
        sim_equity_curves: vec![],
        original_equity_curve: vec![],
    }
}

// ── Simulation helpers ────────────────────────────────────────────────────────

/// Per-simulation result tuple:
/// (net_profit, max_dd_abs, max_dd_pct, n_executed, ruined, equity_curve)
type SimOut = (f64, f64, f64, usize, bool, Vec<f64>);

/// Run equity curve simulation on `sim_pnls` and return the full metrics tuple.
fn run_sim_equity(sim_pnls: Vec<f64>, initial_capital: f64) -> SimOut {
    let n_executed = sim_pnls.len();
    let mut equity = initial_capital;
    let mut peak = equity;
    let mut max_dd_abs = 0.0f64;
    let mut max_dd_pct = 0.0f64;
    let mut ruined = false;
    let mut curve = Vec::with_capacity(n_executed + 1);
    curve.push(equity);

    for pnl in sim_pnls {
        equity += pnl;
        curve.push(equity);
        if equity > peak {
            peak = equity;
        }
        // Ruin = account blown out (equity at or below zero, trading must stop).
        if equity <= 0.0 {
            ruined = true;
        }
        let dd_abs = peak - equity;
        let dd_pct = dd_abs / peak * 100.0;
        if dd_abs > max_dd_abs {
            max_dd_abs = dd_abs;
        }
        if dd_pct > max_dd_pct {
            max_dd_pct = dd_pct;
        }
    }

    let net_profit = equity - initial_capital;
    (net_profit, max_dd_abs, max_dd_pct, n_executed, ruined, curve)
}

// ── public API ────────────────────────────────────────────────────────────────

/// Run a Monte Carlo simulation on historical trades.
///
/// # Methods (controlled by `config.use_resampling` and `config.use_skip_trades`)
///
/// - **Resampling only**: Bootstrap with replacement — draws `n_trades` samples
///   randomly from the pool, allowing the same trade to appear multiple times.
///   Every simulation produces a genuinely different total return and equity path.
///
/// - **Skip Trades only**: Walks the original trade sequence but randomly skips each
///   trade with probability `config.skip_probability`. Models missed executions.
///
/// - **Both enabled** (StrategyQuant X default): Each simulation first resamples the
///   trades via bootstrap, then randomly skips some of the resampled trades. This is
///   the most conservative combination and mirrors SQX behaviour when both checkboxes
///   are checked.
///
/// Returns a confidence-level table at [50, 60, 70, 80, 90, 92, 95, 97, 98]% for
/// Net Profit, Max Drawdown, Ret/DD Ratio and Expectancy, plus sampled equity curves
/// for visualization (max 200 curves, each downsampled to ≤300 points).
pub fn run_monte_carlo(
    trades: &[TradeResult],
    initial_capital: f64,
    config: &MonteCarloConfig,
    cancel_flag: &AtomicBool,
) -> MonteCarloResult {
    if trades.is_empty() || config.n_simulations == 0 || initial_capital <= 0.0 {
        return empty_result();
    }

    // Neither method selected → nothing to do.
    if !config.use_resampling && !config.use_skip_trades {
        return empty_result();
    }

    // Extract net P&L per trade (after commission).
    let pnls: Vec<f64> = trades.iter().map(|t| t.pnl - t.commission).collect();
    let n_trades = pnls.len();

    // ── Original equity curve ──────────────────────────────────────────────────
    let (
        orig_net_profit,
        orig_dd_abs,
        orig_dd_pct,
        _,
        _,
        original_equity,
    ) = run_sim_equity(pnls.clone(), initial_capital);

    let original_return_pct = orig_net_profit / initial_capital * 100.0;
    let original_ret_dd_ratio = if orig_dd_abs > f64::EPSILON {
        orig_net_profit / orig_dd_abs
    } else {
        0.0
    };
    let original_expectancy = if n_trades > 0 {
        orig_net_profit / n_trades as f64
    } else {
        0.0
    };

    let skip_prob = config.skip_probability.clamp(0.0, 0.99);

    // ── Run simulations in parallel ───────────────────────────────────────────
    let sim_outputs: Vec<Option<SimOut>> = (0..config.n_simulations)
        .into_par_iter()
        .map(|_| {
            if cancel_flag.load(Ordering::Relaxed) {
                return None;
            }

            let mut rng = rand::thread_rng();

            // Step 1: Resample (bootstrap with replacement) if enabled.
            let after_resample: Vec<f64> = if config.use_resampling {
                (0..n_trades)
                    .map(|_| pnls[rng.gen_range(0..n_trades)])
                    .collect()
            } else {
                pnls.clone()
            };

            // Step 2: Skip trades if enabled (applied to the resampled set).
            let sim_pnls: Vec<f64> = if config.use_skip_trades {
                after_resample
                    .into_iter()
                    .filter(|_| rng.gen::<f64>() >= skip_prob)
                    .collect()
            } else {
                after_resample
            };

            if sim_pnls.is_empty() {
                return None;
            }

            Some(run_sim_equity(sim_pnls, initial_capital))
        })
        .collect();

    // ── Collect valid results ─────────────────────────────────────────────────
    let mut net_profits: Vec<f64> = Vec::with_capacity(config.n_simulations);
    let mut dd_abs_vec: Vec<f64> = Vec::with_capacity(config.n_simulations);
    let mut ret_dd_ratios: Vec<f64> = Vec::with_capacity(config.n_simulations);
    let mut expectancies: Vec<f64> = Vec::with_capacity(config.n_simulations);
    let mut ruin_count = 0usize;
    let mut all_curves: Vec<Vec<f64>> = Vec::with_capacity(config.n_simulations);

    for output in sim_outputs.into_iter().flatten() {
        let (net_p, dd_abs, _dd_pct, n_exec, ruined, curve) = output;
        let ret_dd = if dd_abs > f64::EPSILON {
            net_p / dd_abs
        } else {
            0.0
        };
        let exp = if n_exec > 0 {
            net_p / n_exec as f64
        } else {
            0.0
        };
        net_profits.push(net_p);
        dd_abs_vec.push(dd_abs);
        ret_dd_ratios.push(ret_dd);
        expectancies.push(exp);
        if ruined {
            ruin_count += 1;
        }
        all_curves.push(curve);
    }

    let completed = net_profits.len();
    if completed == 0 {
        return empty_result();
    }

    // ── Build confidence table ────────────────────────────────────────────────
    let confidence_table = build_confidence_table(
        net_profits,
        dd_abs_vec,
        ret_dd_ratios,
        expectancies,
    );

    // ── Sample curves for display ─────────────────────────────────────────────
    let display_curves: Vec<Vec<f64>> = {
        let total = all_curves.len();
        let step = if total <= MAX_DISPLAY_CURVES { 1 } else { total / MAX_DISPLAY_CURVES };
        all_curves
            .iter()
            .step_by(step)
            .take(MAX_DISPLAY_CURVES)
            .map(|c| downsample(c, MAX_CURVE_POINTS))
            .collect()
    };

    let orig_downsampled = downsample(&original_equity, MAX_CURVE_POINTS);

    MonteCarloResult {
        n_simulations: completed,
        ruin_probability: ruin_count as f64 / completed as f64,
        original_net_profit: orig_net_profit,
        original_max_drawdown_abs: orig_dd_abs,
        original_ret_dd_ratio,
        original_expectancy,
        original_return_pct,
        original_max_drawdown_pct: orig_dd_pct,
        confidence_table,
        sim_equity_curves: display_curves,
        original_equity_curve: orig_downsampled,
    }
}
