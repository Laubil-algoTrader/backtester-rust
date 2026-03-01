use std::sync::atomic::{AtomicBool, Ordering};

use rand::seq::SliceRandom;
use rayon::prelude::*;

use crate::models::result::MonteCarloResult;
use crate::models::trade::TradeResult;

/// Run a Monte Carlo simulation by randomly reordering the historical trade sequence.
///
/// For each simulation:
/// 1. Shuffle the trade P&L values into a random order.
/// 2. Replay the shuffled sequence: apply each trade's net P&L to a running equity.
/// 3. Record the final equity and the maximum percentage drawdown encountered.
///
/// Returns percentile statistics across all simulations plus a ruin probability
/// (fraction of simulations where equity fell below `initial_capital` at any point).
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

            let mut shuffled = pnls.clone();
            let mut rng = rand::thread_rng();
            shuffled.shuffle(&mut rng);

            let mut equity = initial_capital;
            let mut peak = equity;
            let mut max_dd_pct = 0.0f64;
            let mut ruined = false;

            for pnl in &shuffled {
                equity += pnl;
                if equity > peak {
                    peak = equity;
                }
                if equity <= 0.0 {
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
