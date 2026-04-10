/// CMA-ES constant optimizer for SR formula trees.
///
/// Implements a simplified diagonal-CMA-ES to refine the numeric constants
/// inside the best SR strategies discovered by NSGA-II. After evolution fixes
/// the formula structure, CMA-ES finds better constant values.
///
/// Reference: Hansen & Ostermeier (2001) "Completely Derandomized Self-Adaptation
/// in Evolution Strategies". Here we use the diagonal approximation (rank-one
/// update only, no full covariance) because the number of constants per formula
/// is small (typically < 20) and speed matters more than full covariance accuracy.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::engine::executor::SubBarData;
use crate::models::candle::Candle;
use crate::models::config::{InstrumentConfig, Timeframe};
use crate::models::sr_result::SrConfig;

use super::nsga2::SrIndividual;
use super::runner::sr_backtest;
use super::tree::{self, SrCache};

// ── CMA-ES Entry Point ────────────────────────────────────────────────────────

/// Refine the constants in `ind` using a simplified (μ/μ_w, λ)-CMA-ES.
/// Returns a new `SrIndividual` with the best constants found.
/// If no constants exist in the formula, returns the individual unchanged.
pub fn optimize_constants(
    ind: &SrIndividual,
    candles: &[Candle],
    cache: &SrCache,
    atr_series: &Arc<Vec<f64>>,
    instrument: &InstrumentConfig,
    config: &SrConfig,
    timeframe: Timeframe,
    cancel_flag: &AtomicBool,
    sub_bars: &SubBarData,
) -> SrIndividual {
    // Extract constants from all 3 trees + thresholds
    let mut strategy = ind.strategy.clone();
    let tree_constants = {
        let mut v = tree::extract_constants(&strategy.entry_long);
        v.extend(tree::extract_constants(&strategy.entry_short));
        v.extend(tree::extract_constants(&strategy.exit));
        v
    };
    // Append thresholds as optimizable parameters (always present)
    let mut constants = tree_constants;
    constants.push(strategy.long_threshold);
    constants.push(strategy.short_threshold);
    let dim = constants.len();
    // dim >= 2 always (at least the two thresholds), but guard against degenerate case
    if dim == 0 {
        return ind.clone();
    }

    // Evaluate initial fitness
    let initial_scalar = evaluate_scalar(
        &strategy,
        candles,
        cache,
        atr_series,
        instrument,
        config,
        timeframe,
        sub_bars,
    );

    // ── CMA-ES parameters ────────────────────────────────────────────────────
    // Minimum 6 offspring so that even single-constant formulas explore enough.
    let lambda: usize = ((4.0 + 3.0 * (dim as f64).ln()).ceil() as usize).max(6); // offspring count
    let mu = lambda / 2; // parents for recombination
    let mu_eff = mu as f64; // simplified (equal weights)

    let c_sigma = (mu_eff + 2.0) / (dim as f64 + mu_eff + 5.0);
    let d_sigma = 1.0
        + 2.0 * ((mu_eff - 1.0) / (dim as f64 + 1.0)).sqrt().max(0.0)
        + c_sigma;
    let chi_n = (dim as f64).sqrt() * (1.0 - 1.0 / (4.0 * dim as f64) + 1.0 / (21.0 * dim as f64 * dim as f64));

    let mut mean: Vec<f64> = constants.clone();
    let mut sigma = initial_sigma(&constants);
    let mut p_sigma: Vec<f64> = vec![0.0; dim]; // evolution path for step size
    let mut best_vals = constants.clone();
    let mut best_scalar = initial_scalar;

    let mut rng = rand::thread_rng();
    let max_evals = config.cmaes_iterations;
    let mut evals = 0usize;

    while evals < max_evals {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }
        // Sample λ offspring using Gaussian noise (required by the path-length control theory).
        let mut samples: Vec<(Vec<f64>, f64)> = Vec::with_capacity(lambda);
        for _ in 0..lambda {
            if evals >= max_evals {
                break;
            }
            let x: Vec<f64> = mean
                .iter()
                .enumerate()
                .map(|(i, &m)| {
                    let raw = m + sigma * standard_normal(&mut rng);
                    // Adaptive bounds: clamp to ±10σ from mean for tree constants,
                    // and to [-1000, 1000] for thresholds (last 2 dimensions).
                    if i >= dim.saturating_sub(2) {
                        // Thresholds
                        raw.clamp(-1000.0, 1000.0)
                    } else {
                        // Tree constants: soft bounds ±10σ from mean
                        let bound = 10.0 * sigma;
                        raw.clamp(m - bound, m + bound)
                    }
                })
                .collect();
            let s = evaluate_with_constants(
                &mut strategy,
                &x,
                candles,
                cache,
                atr_series,
                instrument,
                config,
                timeframe,
                sub_bars,
            );
            samples.push((x, s));
            evals += 1;
        }

        // Sort by scalar fitness (descending — maximize)
        samples.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Guard: if we ran out of evaluations mid-iteration, fewer than mu samples exist
        if samples.is_empty() {
            break;
        }
        let actual_mu = mu.min(samples.len());
        let actual_mu_eff = actual_mu as f64;

        // Update best
        if samples[0].1 > best_scalar {
            best_scalar = samples[0].1;
            best_vals = samples[0].0.clone();
        }

        // Recombination: weighted mean of top-μ (or however many we have)
        let new_mean: Vec<f64> = (0..dim)
            .map(|d| {
                samples[..actual_mu].iter().map(|(x, _)| x[d]).sum::<f64>() / actual_mu_eff
            })
            .collect();

        // Step-size control (cumulative path length control)
        let step: Vec<f64> = new_mean
            .iter()
            .zip(mean.iter())
            .map(|(n, m)| (n - m) / sigma)
            .collect();
        let _step_norm = step.iter().map(|v| v * v).sum::<f64>().sqrt();

        p_sigma = p_sigma
            .iter()
            .zip(step.iter())
            .map(|(ps, s)| (1.0 - c_sigma) * ps + (c_sigma * (2.0 - c_sigma) * actual_mu_eff).sqrt() * s)
            .collect();

        let ps_norm = p_sigma.iter().map(|v| v * v).sum::<f64>().sqrt();
        // Clamp the ARGUMENT of exp (not the result) to avoid asymmetric step-size adaptation.
        // Clamping exp(x).min(2.0) was incorrect: it allowed exp(-inf)=0 but capped exp(inf)=2,
        // creating an asymmetric adaptation that could stall convergence.
        let sigma_exp_arg = ((c_sigma / d_sigma) * (ps_norm / chi_n - 1.0)).clamp(-2.0, 2.0);
        sigma *= sigma_exp_arg.exp();

        // Clamp sigma to reasonable bounds
        let max_sigma = initial_sigma(&mean).max(0.1);
        sigma = sigma.clamp(1e-6, max_sigma * 10.0);

        mean = new_mean;
    }

    // Build the best individual
    let mut result = ind.clone();
    apply_constants_to_strategy(&mut result.strategy, &best_vals);

    // Re-evaluate objectives with best constants
    result.objectives = sr_backtest(
        &result.strategy,
        candles,
        cache,
        atr_series,
        instrument,
        config.initial_capital,
        timeframe,
        config.max_trades_per_day,
        sub_bars,
    )
    .map(|m| crate::engine::sr::runner::compute_objectives_pub(&m, config.min_trades));

    result
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Box-Muller transform: generate one standard-normal sample from two uniform samples.
/// Used instead of uniform noise so that the path-length step-size control (chi_n,
/// c_sigma, d_sigma) is theoretically consistent — those constants assume Gaussian sampling.
///
/// Uses rejection sampling for u1 to avoid `.max(1e-10)` which biases the uniform
/// distribution and compresses the tails of the resulting normal distribution.
#[inline]
fn standard_normal(rng: &mut impl rand::Rng) -> f64 {
    let u1: f64 = loop {
        let v = rng.gen::<f64>();
        if v > 1e-10 { break v; }
    };
    let u2: f64 = rng.gen();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

fn initial_sigma(constants: &[f64]) -> f64 {
    // Start with ~10% of RMS of initial constants, clamped to [0.05, 2.0]
    let rms = (constants.iter().map(|v| v * v).sum::<f64>() / constants.len() as f64).sqrt();
    (rms * 0.1 + 0.1).clamp(0.05, 2.0)
}

/// Apply a flat constant vector to the 3 trees of the strategy (in-order traversal).
/// The last 2 elements are `long_threshold` and `short_threshold`.
fn apply_constants_to_strategy(strategy: &mut crate::models::sr_result::SrStrategy, vals: &[f64]) {
    let mut idx = 0;
    tree::replace_constants(&mut strategy.entry_long, vals, &mut idx);
    tree::replace_constants(&mut strategy.entry_short, vals, &mut idx);
    tree::replace_constants(&mut strategy.exit, vals, &mut idx);
    // Remaining 2 values are thresholds
    if idx < vals.len() {
        strategy.long_threshold = vals[idx];
        idx += 1;
    }
    if idx < vals.len() {
        strategy.short_threshold = vals[idx];
    }
}

fn evaluate_with_constants(
    strategy: &mut crate::models::sr_result::SrStrategy,
    vals: &[f64],
    candles: &[Candle],
    cache: &SrCache,
    atr_series: &[f64],
    instrument: &InstrumentConfig,
    config: &SrConfig,
    timeframe: Timeframe,
    sub_bars: &SubBarData,
) -> f64 {
    apply_constants_to_strategy(strategy, vals);
    evaluate_scalar(strategy, candles, cache, atr_series, instrument, config, timeframe, sub_bars)
}

fn evaluate_scalar(
    strategy: &crate::models::sr_result::SrStrategy,
    candles: &[Candle],
    cache: &SrCache,
    atr_series: &[f64],
    instrument: &InstrumentConfig,
    config: &SrConfig,
    timeframe: Timeframe,
    sub_bars: &SubBarData,
) -> f64 {
    let node_count = tree::count_nodes(&strategy.entry_long)
        + tree::count_nodes(&strategy.entry_short)
        + tree::count_nodes(&strategy.exit);

    // Parsimony pressure: formulas exceeding the threshold get a configurable multiplier.
    // Both threshold (default 20) and multiplier (default 0.8) come from SrConfig.
    let complexity_multiplier = if node_count > config.cmaes_bloat_threshold {
        config.cmaes_bloat_multiplier
    } else {
        1.0
    };

    sr_backtest(strategy, candles, cache, atr_series, instrument, config.initial_capital, timeframe, config.max_trades_per_day, sub_bars)
        .map(|m| {
            crate::engine::sr::runner::compute_objectives_pub(&m, config.min_trades)
                .scalar_with_weights(config.scalar_weights.as_ref())
                * complexity_multiplier
        })
        .unwrap_or(f64::NEG_INFINITY)
}
