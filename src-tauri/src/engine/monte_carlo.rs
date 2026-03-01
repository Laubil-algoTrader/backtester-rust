use std::sync::atomic::{AtomicBool, Ordering};

use rand::Rng;
use rayon::prelude::*;

use crate::models::result::{MonteCarloConfig, MonteCarloMethod, MonteCarloResult};
use crate::models::trade::TradeResult;

/// Maximum number of simulation equity curves returned for visualization.
const MAX_DISPLAY_CURVES: usize = 200;

/// Maximum number of points per equity curve (downsampled if the trade count is higher).
const MAX_CURVE_POINTS: usize = 300;

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

/// Compute the p-th percentile of a **pre-sorted** slice using linear interpolation.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = p / 100.0 * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = (lower + 1).min(sorted.len() - 1);
    let frac = rank - lower as f64;
    sorted[lower] + frac * (sorted[upper] - sorted[lower])
}

/// Returns an empty result used when inputs are invalid or all simulations were cancelled.
fn empty_result() -> MonteCarloResult {
    MonteCarloResult {
        n_simulations: 0,
        median_return_pct: 0.0,
        p5_return_pct: 0.0,
        p25_return_pct: 0.0,
        p75_return_pct: 0.0,
        p95_return_pct: 0.0,
        ruin_probability: 0.0,
        median_max_drawdown_pct: 0.0,
        p25_max_drawdown_pct: 0.0,
        p75_max_drawdown_pct: 0.0,
        p95_max_drawdown_pct: 0.0,
        sim_equity_curves: vec![],
        original_equity_curve: vec![],
        original_return_pct: 0.0,
        original_max_drawdown_pct: 0.0,
    }
}

// ── public API ────────────────────────────────────────────────────────────────

/// Run a Monte Carlo simulation on historical trades.
///
/// # Methods
/// - **`Resampling`**: Bootstrap with replacement — draws `n_trades` samples randomly
///   from the pool, allowing repeats. Every simulation produces a genuinely different
///   total return and equity path.
/// - **`SkipTrades`**: Iterates the original trade sequence but randomly skips each
///   trade with probability `config.skip_probability`. Models missed executions,
///   technical failures, or selective filtering.
///
/// Returns percentile statistics for return and max-drawdown distributions, the ruin
/// probability (equity < initial_capital at any point), and sampled equity curves for
/// visualization (max 200 curves, each downsampled to ≤300 points).
pub fn run_monte_carlo(
    trades: &[TradeResult],
    initial_capital: f64,
    config: &MonteCarloConfig,
    cancel_flag: &AtomicBool,
) -> MonteCarloResult {
    if trades.is_empty() || config.n_simulations == 0 || initial_capital <= 0.0 {
        return empty_result();
    }

    // Extract net P&L per trade (after commission).
    let pnls: Vec<f64> = trades.iter().map(|t| t.pnl - t.commission).collect();
    let n_trades = pnls.len();

    // ── Original equity curve (for chart overlay + filter reference) ──────────
    let original_equity: Vec<f64> = {
        let mut eq = initial_capital;
        let mut curve = Vec::with_capacity(n_trades + 1);
        curve.push(eq);
        for &pnl in &pnls {
            eq += pnl;
            curve.push(eq);
        }
        curve
    };

    let original_final = original_equity.last().copied().unwrap_or(initial_capital);
    let original_return_pct = (original_final - initial_capital) / initial_capital * 100.0;

    // Max drawdown of original equity curve.
    let original_max_dd = {
        let mut peak = initial_capital;
        let mut max_dd = 0.0f64;
        for &eq in &original_equity {
            if eq > peak {
                peak = eq;
            }
            if peak > 0.0 {
                let dd = (peak - eq) / peak * 100.0;
                if dd > max_dd {
                    max_dd = dd;
                }
            }
        }
        max_dd
    };

    // ── Run simulations in parallel ───────────────────────────────────────────
    // Each result: (return_pct, max_dd_pct, ruined, equity_curve)
    let sim_results: Vec<Option<(f64, f64, bool, Vec<f64>)>> = (0..config.n_simulations)
        .into_par_iter()
        .map(|_| {
            if cancel_flag.load(Ordering::Relaxed) {
                return None;
            }

            let mut rng = rand::thread_rng();
            let mut equity = initial_capital;
            let mut peak = equity;
            let mut max_dd_pct = 0.0f64;
            let mut ruined = false;
            let mut curve = Vec::with_capacity(n_trades + 1);
            curve.push(equity);

            match config.method {
                MonteCarloMethod::Resampling => {
                    // Bootstrap WITH replacement: each of the N slots draws a random trade.
                    for _ in 0..n_trades {
                        let pnl = pnls[rng.gen_range(0..n_trades)];
                        equity += pnl;
                        curve.push(equity);
                        if equity > peak {
                            peak = equity;
                        }
                        if equity < initial_capital {
                            ruined = true;
                        }
                        if peak > 0.0 {
                            let dd = (peak - equity) / peak * 100.0;
                            if dd > max_dd_pct {
                                max_dd_pct = dd;
                            }
                        }
                    }
                }

                MonteCarloMethod::SkipTrades => {
                    // Walk the original sequence; skip each trade with probability p.
                    let skip_prob = config.skip_probability.clamp(0.0, 0.99);
                    for &pnl in &pnls {
                        if rng.gen::<f64>() < skip_prob {
                            // Trade skipped — equity stays flat for this step.
                            curve.push(equity);
                            continue;
                        }
                        equity += pnl;
                        curve.push(equity);
                        if equity > peak {
                            peak = equity;
                        }
                        if equity < initial_capital {
                            ruined = true;
                        }
                        if peak > 0.0 {
                            let dd = (peak - equity) / peak * 100.0;
                            if dd > max_dd_pct {
                                max_dd_pct = dd;
                            }
                        }
                    }
                }
            }

            let return_pct = (equity - initial_capital) / initial_capital * 100.0;
            Some((return_pct, max_dd_pct, ruined, curve))
        })
        .collect();

    // ── Collect valid results ─────────────────────────────────────────────────
    let mut returns: Vec<f64> = Vec::with_capacity(config.n_simulations);
    let mut drawdowns: Vec<f64> = Vec::with_capacity(config.n_simulations);
    let mut ruin_count = 0usize;
    let mut all_curves: Vec<Vec<f64>> = Vec::with_capacity(config.n_simulations);

    for r in sim_results.into_iter().flatten() {
        returns.push(r.0);
        drawdowns.push(r.1);
        if r.2 {
            ruin_count += 1;
        }
        all_curves.push(r.3);
    }

    let completed = returns.len();
    if completed == 0 {
        return empty_result();
    }

    returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    drawdowns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // ── Sample curves for display ─────────────────────────────────────────────
    // Take at most MAX_DISPLAY_CURVES evenly spaced curves and downsample each
    // to MAX_CURVE_POINTS so the JSON payload stays manageable.
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
        median_return_pct: percentile(&returns, 50.0),
        p5_return_pct: percentile(&returns, 5.0),
        p25_return_pct: percentile(&returns, 25.0),
        p75_return_pct: percentile(&returns, 75.0),
        p95_return_pct: percentile(&returns, 95.0),
        ruin_probability: ruin_count as f64 / completed as f64,
        median_max_drawdown_pct: percentile(&drawdowns, 50.0),
        p25_max_drawdown_pct: percentile(&drawdowns, 25.0),
        p75_max_drawdown_pct: percentile(&drawdowns, 75.0),
        p95_max_drawdown_pct: percentile(&drawdowns, 95.0),
        sim_equity_curves: display_curves,
        original_equity_curve: orig_downsampled,
        original_return_pct,
        original_max_drawdown_pct: original_max_dd,
    }
}
