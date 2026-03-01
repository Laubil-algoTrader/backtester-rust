use std::sync::atomic::{AtomicBool, Ordering};

use rand::Rng;
use rayon::prelude::*;

use crate::models::result::MonteCarloResult;
use crate::models::trade::TradeResult;

/// Run a Monte Carlo simulation using bootstrap resampling WITH replacement.
///
/// For each simulation:
/// 1. Draw `n_trades` trade P&Ls randomly from the historical pool (with replacement).
///    This means the same trade can appear multiple times in one simulation, and some
///    trades may not appear at all — producing genuinely different final equities.
/// 2. Replay the resampled sequence: apply each trade's net P&L to a running equity.
/// 3. Record the final equity and the maximum percentage drawdown encountered.
///
/// Returns percentile statistics across all simulations plus a ruin probability
/// (fraction of simulations where equity dropped below `initial_capital` at any point,
/// i.e. the account showed a net loss relative to the starting capital).
pub fn run_monte_carlo(
    trades: &[TradeResult],
    initial_capital: f64,
    n_simulations: usize,
    cancel_flag: &AtomicBool,
) -> MonteCarloResult {
    if trades.is_empty() || n_simulations == 0 || initial_capital <= 0.0 {
        return MonteCarloResult {
            n_simulations: 0,
            median_return_pct: 0.0,
            p5_return_pct: 0.0,
            p25_return_pct: 0.0,
            p75_return_pct: 0.0,
            p95_return_pct: 0.0,
            ruin_probability: 0.0,
            median_max_drawdown_pct: 0.0,
            p95_max_drawdown_pct: 0.0,
        };
    }

    // Extract net P&L per trade (after commission)
    let pnls: Vec<f64> = trades.iter().map(|t| t.pnl - t.commission).collect();

    // Run simulations in parallel
    let sim_results: Vec<Option<(f64, f64, bool)>> = (0..n_simulations)
        .into_par_iter()
        .map(|_| {
            if cancel_flag.load(Ordering::Relaxed) {
                return None;
            }

            let mut rng = rand::thread_rng();
            let n_trades = pnls.len();

            let mut equity = initial_capital;
            let mut peak = equity;
            let mut max_dd_pct = 0.0f64;
            let mut ruined = false;

            // Bootstrap WITH replacement: draw n_trades samples randomly from the pool.
            // Unlike shuffle, this allows repeats and omissions, so each simulation
            // produces a genuinely different total return.
            for _ in 0..n_trades {
                let pnl = pnls[rng.gen_range(0..n_trades)];
                equity += pnl;
                if equity > peak {
                    peak = equity;
                }
                // Ruin = account is below the starting capital at any point (net loss)
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

            let return_pct = (equity - initial_capital) / initial_capital * 100.0;
            Some((return_pct, max_dd_pct, ruined))
        })
        .collect();

    // Collect valid results
    let mut returns: Vec<f64> = Vec::with_capacity(n_simulations);
    let mut drawdowns: Vec<f64> = Vec::with_capacity(n_simulations);
    let mut ruin_count = 0usize;

    for r in sim_results.into_iter().flatten() {
        returns.push(r.0);
        drawdowns.push(r.1);
        if r.2 {
            ruin_count += 1;
        }
    }

    let completed = returns.len();
    if completed == 0 {
        return MonteCarloResult {
            n_simulations: 0,
            median_return_pct: 0.0,
            p5_return_pct: 0.0,
            p25_return_pct: 0.0,
            p75_return_pct: 0.0,
            p95_return_pct: 0.0,
            ruin_probability: 0.0,
            median_max_drawdown_pct: 0.0,
            p95_max_drawdown_pct: 0.0,
        };
    }

    returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    drawdowns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    MonteCarloResult {
        n_simulations: completed,
        median_return_pct: percentile(&returns, 50.0),
        p5_return_pct: percentile(&returns, 5.0),
        p25_return_pct: percentile(&returns, 25.0),
        p75_return_pct: percentile(&returns, 75.0),
        p95_return_pct: percentile(&returns, 95.0),
        ruin_probability: ruin_count as f64 / completed as f64,
        median_max_drawdown_pct: percentile(&drawdowns, 50.0),
        p95_max_drawdown_pct: percentile(&drawdowns, 95.0),
    }
}

/// Compute the p-th percentile of a pre-sorted slice using linear interpolation.
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
