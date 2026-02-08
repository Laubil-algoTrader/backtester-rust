use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rand::Rng;
use rayon::prelude::*;
use tracing::info;

use crate::errors::AppError;
use crate::models::candle::Candle;
use crate::models::config::InstrumentConfig;
use crate::models::result::{
    BacktestMetrics, GeneticAlgorithmConfig, ObjectiveFunction, OptimizationResult,
    ParameterRange,
};
use crate::models::strategy::{BacktestConfig, IndicatorParams, Strategy};

use super::executor;

/// Maximum allowed combinations for Grid Search.
const MAX_COMBINATIONS: usize = 50_000;

/// Maximum results to return from optimization.
const MAX_RESULTS: usize = 50;

// ══════════════════════════════════════════════════════════════
// Shared helpers
// ══════════════════════════════════════════════════════════════

/// Apply parameter values to a strategy, returning a modified clone.
///
/// Each `ParameterRange` specifies a `rule_index` (0-based across entry_rules first,
/// then exit_rules) and a `param_name` matching an `IndicatorParams` field.
/// The function finds the indicator operand in that rule and updates the parameter.
fn apply_params(strategy: &Strategy, ranges: &[ParameterRange], values: &[f64]) -> Strategy {
    let mut s = strategy.clone();
    let entry_len = s.entry_rules.len();

    for (range, &val) in ranges.iter().zip(values.iter()) {
        let rule = if range.rule_index < entry_len {
            &mut s.entry_rules[range.rule_index]
        } else {
            let exit_idx = range.rule_index - entry_len;
            &mut s.exit_rules[exit_idx]
        };

        // Try to update the indicator in left_operand, then right_operand
        let updated = set_indicator_param(&mut rule.left_operand.indicator, &range.param_name, val)
            || set_indicator_param(&mut rule.right_operand.indicator, &range.param_name, val);

        if !updated {
            tracing::warn!(
                "Could not apply param '{}' to rule index {}",
                range.param_name,
                range.rule_index
            );
        }
    }

    s
}

/// Set a parameter on an IndicatorConfig by name. Returns true if set.
fn set_indicator_param(
    indicator: &mut Option<crate::models::strategy::IndicatorConfig>,
    param_name: &str,
    value: f64,
) -> bool {
    let Some(ref mut ind) = indicator else {
        return false;
    };
    set_param(&mut ind.params, param_name, value)
}

/// Set a named parameter on IndicatorParams. Integer params are rounded.
fn set_param(params: &mut IndicatorParams, name: &str, value: f64) -> bool {
    match name {
        "period" => {
            params.period = Some(value.round() as usize);
            true
        }
        "fast_period" => {
            params.fast_period = Some(value.round() as usize);
            true
        }
        "slow_period" => {
            params.slow_period = Some(value.round() as usize);
            true
        }
        "signal_period" => {
            params.signal_period = Some(value.round() as usize);
            true
        }
        "std_dev" => {
            params.std_dev = Some(value);
            true
        }
        "k_period" => {
            params.k_period = Some(value.round() as usize);
            true
        }
        "d_period" => {
            params.d_period = Some(value.round() as usize);
            true
        }
        "acceleration_factor" => {
            params.acceleration_factor = Some(value);
            true
        }
        "maximum_factor" => {
            params.maximum_factor = Some(value);
            true
        }
        _ => false,
    }
}

/// Extract the objective value from backtest metrics.
fn extract_objective(metrics: &BacktestMetrics, objective: &ObjectiveFunction) -> f64 {
    match objective {
        ObjectiveFunction::TotalProfit => metrics.net_profit,
        ObjectiveFunction::SharpeRatio => metrics.sharpe_ratio,
        ObjectiveFunction::ProfitFactor => metrics.profit_factor,
        ObjectiveFunction::WinRate => metrics.win_rate_pct,
    }
}

/// Build an OptimizationResult from backtest metrics and parameter values.
fn build_result(
    ranges: &[ParameterRange],
    values: &[f64],
    metrics: &BacktestMetrics,
    objective: &ObjectiveFunction,
) -> OptimizationResult {
    let mut params = HashMap::new();
    for (range, &val) in ranges.iter().zip(values.iter()) {
        params.insert(range.display_name.clone(), val);
    }
    OptimizationResult {
        params,
        objective_value: extract_objective(metrics, objective),
        total_return_pct: metrics.total_return_pct,
        sharpe_ratio: metrics.sharpe_ratio,
        max_drawdown_pct: metrics.max_drawdown_pct,
        total_trades: metrics.total_trades,
        profit_factor: metrics.profit_factor,
    }
}

// ══════════════════════════════════════════════════════════════
// Grid Search
// ══════════════════════════════════════════════════════════════

/// Generate all value combinations from parameter ranges (cartesian product).
fn generate_grid(ranges: &[ParameterRange]) -> Result<Vec<Vec<f64>>, AppError> {
    if ranges.is_empty() {
        return Err(AppError::OptimizationError(
            "No parameter ranges specified".into(),
        ));
    }

    // Generate values for each range
    let mut per_range: Vec<Vec<f64>> = Vec::with_capacity(ranges.len());
    let mut total: usize = 1;

    for r in ranges {
        if r.step <= 0.0 {
            return Err(AppError::OptimizationError(format!(
                "Step must be positive for parameter '{}'",
                r.display_name
            )));
        }
        let mut vals = Vec::new();
        let mut v = r.min;
        while v <= r.max + f64::EPSILON {
            vals.push(v);
            v += r.step;
        }
        if vals.is_empty() {
            vals.push(r.min);
        }
        total = total.saturating_mul(vals.len());
        per_range.push(vals);
    }

    if total > MAX_COMBINATIONS {
        return Err(AppError::TooManyCombinations {
            count: total,
            limit: MAX_COMBINATIONS,
        });
    }

    // Build cartesian product iteratively
    let mut combos: Vec<Vec<f64>> = vec![vec![]];
    for vals in &per_range {
        let mut next = Vec::with_capacity(combos.len() * vals.len());
        for combo in &combos {
            for &v in vals {
                let mut c = combo.clone();
                c.push(v);
                next.push(c);
            }
        }
        combos = next;
    }

    Ok(combos)
}

/// Run Grid Search optimization.
///
/// Evaluates all parameter combinations in parallel using rayon.
/// The `progress_callback` receives `(percent, current, total, best_so_far)`.
pub fn run_grid_search(
    candles: &[Candle],
    strategy: &Strategy,
    config: &BacktestConfig,
    instrument: &InstrumentConfig,
    ranges: &[ParameterRange],
    objective: &ObjectiveFunction,
    cancel_flag: &AtomicBool,
    progress_callback: impl Fn(u8, usize, usize, f64) + Send + Sync,
) -> Result<Vec<OptimizationResult>, AppError> {
    let combinations = generate_grid(ranges)?;
    let total = combinations.len();
    info!("Grid search: {} combinations", total);

    let counter = AtomicUsize::new(0);
    let best_so_far = Arc::new(Mutex::new(f64::NEG_INFINITY));
    let start = Instant::now();

    let results: Vec<Option<OptimizationResult>> = combinations
        .par_iter()
        .map(|values| {
            // Check cancellation
            if cancel_flag.load(Ordering::Relaxed) {
                return None;
            }

            let modified = apply_params(strategy, ranges, values);

            // Run backtest with no-op progress callback
            let result = executor::run_backtest(
                candles,
                &modified,
                config,
                instrument,
                cancel_flag,
                |_, _, _| {},
            );

            let current = counter.fetch_add(1, Ordering::Relaxed) + 1;

            match result {
                Ok(bt) => {
                    let opt_result = build_result(ranges, values, &bt.metrics, objective);

                    // Update best
                    {
                        let mut best = best_so_far.lock().unwrap();
                        if opt_result.objective_value > *best {
                            *best = opt_result.objective_value;
                        }
                    }

                    // Report progress periodically (every 1% or every 10 iterations)
                    let report_interval = (total / 100).max(1);
                    if current % report_interval == 0 || current == total {
                        let pct = ((current as f64 / total as f64) * 100.0) as u8;
                        let best_val = *best_so_far.lock().unwrap();
                        progress_callback(pct, current, total, best_val);
                    }

                    Some(opt_result)
                }
                Err(_) => {
                    // Skip failed backtests (e.g. insufficient data for large periods)
                    None
                }
            }
        })
        .collect();

    if cancel_flag.load(Ordering::Relaxed) {
        return Err(AppError::OptimizationCancelled);
    }

    let elapsed = start.elapsed();
    let mut valid: Vec<OptimizationResult> = results.into_iter().flatten().collect();
    valid.sort_by(|a, b| b.objective_value.partial_cmp(&a.objective_value).unwrap_or(std::cmp::Ordering::Equal));
    valid.truncate(MAX_RESULTS);

    info!(
        "Grid search complete: {} valid results in {:.1}s",
        valid.len(),
        elapsed.as_secs_f64()
    );

    Ok(valid)
}

// ══════════════════════════════════════════════════════════════
// Genetic Algorithm
// ══════════════════════════════════════════════════════════════

/// An individual in the GA population: parameter values + fitness.
#[derive(Clone)]
struct Individual {
    genes: Vec<f64>,
    fitness: f64,
}

/// Run Genetic Algorithm optimization.
///
/// Uses tournament selection, single-point crossover, and mutation.
/// Evaluates each generation in parallel with rayon.
pub fn run_genetic_algorithm(
    candles: &[Candle],
    strategy: &Strategy,
    config: &BacktestConfig,
    instrument: &InstrumentConfig,
    ranges: &[ParameterRange],
    objective: &ObjectiveFunction,
    ga_config: &GeneticAlgorithmConfig,
    cancel_flag: &AtomicBool,
    progress_callback: impl Fn(u8, usize, usize, f64) + Send + Sync,
) -> Result<Vec<OptimizationResult>, AppError> {
    let pop_size = ga_config.population_size;
    let generations = ga_config.generations;
    let mutation_rate = ga_config.mutation_rate;
    let crossover_rate = ga_config.crossover_rate;
    let num_params = ranges.len();

    if num_params == 0 {
        return Err(AppError::OptimizationError(
            "No parameter ranges specified".into(),
        ));
    }

    info!(
        "GA: pop={}, gens={}, mut_rate={:.2}, cross_rate={:.2}, params={}",
        pop_size, generations, mutation_rate, crossover_rate, num_params
    );

    let start = Instant::now();

    // Collect all evaluated individuals across all generations
    let all_results: Arc<Mutex<Vec<OptimizationResult>>> = Arc::new(Mutex::new(Vec::new()));

    // Initialize random population
    let mut population: Vec<Individual> = (0..pop_size)
        .map(|_| {
            let mut rng = rand::thread_rng();
            let genes: Vec<f64> = ranges
                .iter()
                .map(|r| snap_to_step(rng.gen_range(r.min..=r.max), r))
                .collect();
            Individual {
                genes,
                fitness: f64::NEG_INFINITY,
            }
        })
        .collect();

    let mut global_best = f64::NEG_INFINITY;

    for gen in 0..generations {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::OptimizationCancelled);
        }

        // Evaluate population in parallel
        let fitnesses: Vec<Option<(f64, OptimizationResult)>> = population
            .par_iter()
            .map(|ind| {
                if cancel_flag.load(Ordering::Relaxed) {
                    return None;
                }

                let modified = apply_params(strategy, ranges, &ind.genes);
                let result = executor::run_backtest(
                    candles,
                    &modified,
                    config,
                    instrument,
                    cancel_flag,
                    |_, _, _| {},
                );

                match result {
                    Ok(bt) => {
                        let opt_result = build_result(ranges, &ind.genes, &bt.metrics, objective);
                        let fitness = opt_result.objective_value;
                        Some((fitness, opt_result))
                    }
                    Err(_) => Some((f64::NEG_INFINITY, build_failed_result(ranges, &ind.genes))),
                }
            })
            .collect();

        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::OptimizationCancelled);
        }

        // Update fitness values and collect results
        for (ind, eval) in population.iter_mut().zip(fitnesses.into_iter()) {
            if let Some((fitness, opt_result)) = eval {
                ind.fitness = fitness;
                if fitness > f64::NEG_INFINITY {
                    all_results.lock().unwrap().push(opt_result);
                }
                if fitness > global_best {
                    global_best = fitness;
                }
            }
        }

        // Report progress
        let pct = (((gen + 1) as f64 / generations as f64) * 100.0) as u8;
        progress_callback(pct, gen + 1, generations, global_best);

        // Don't breed after the last generation
        if gen + 1 >= generations {
            break;
        }

        // Find the best individual for elitism
        let best_idx = population
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.fitness
                    .partial_cmp(&b.fitness)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        let elite = population[best_idx].clone();

        // Build next generation
        let mut next_pop: Vec<Individual> = Vec::with_capacity(pop_size);
        next_pop.push(elite); // Elitism

        let mut rng = rand::thread_rng();

        while next_pop.len() < pop_size {
            // Tournament selection
            let parent1 = tournament_select(&population, &mut rng);
            let parent2 = tournament_select(&population, &mut rng);

            // Crossover
            let (mut child1, mut child2) = if rng.gen::<f64>() < crossover_rate && num_params > 1 {
                crossover(&parent1.genes, &parent2.genes, &mut rng)
            } else {
                (parent1.genes.clone(), parent2.genes.clone())
            };

            // Mutation
            mutate(&mut child1, ranges, mutation_rate, &mut rng);
            mutate(&mut child2, ranges, mutation_rate, &mut rng);

            next_pop.push(Individual {
                genes: child1,
                fitness: f64::NEG_INFINITY,
            });
            if next_pop.len() < pop_size {
                next_pop.push(Individual {
                    genes: child2,
                    fitness: f64::NEG_INFINITY,
                });
            }
        }

        population = next_pop;
    }

    let elapsed = start.elapsed();

    // Deduplicate and sort results
    let mut results = match Arc::try_unwrap(all_results) {
        Ok(mutex) => mutex.into_inner().unwrap(),
        Err(arc) => arc.lock().unwrap().clone(),
    };
    results.sort_by(|a, b| {
        b.objective_value
            .partial_cmp(&a.objective_value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.dedup_by(|a, b| a.params == b.params);
    results.truncate(MAX_RESULTS);

    info!(
        "GA complete: {} unique results in {:.1}s",
        results.len(),
        elapsed.as_secs_f64()
    );

    Ok(results)
}

/// Tournament selection: pick 3 random individuals, return the best.
fn tournament_select<'a>(population: &'a [Individual], rng: &mut impl Rng) -> &'a Individual {
    let n = population.len();
    let mut best_idx = rng.gen_range(0..n);
    for _ in 0..2 {
        let idx = rng.gen_range(0..n);
        if population[idx].fitness > population[best_idx].fitness {
            best_idx = idx;
        }
    }
    &population[best_idx]
}

/// Single-point crossover.
fn crossover(parent1: &[f64], parent2: &[f64], rng: &mut impl Rng) -> (Vec<f64>, Vec<f64>) {
    let point = rng.gen_range(1..parent1.len());
    let mut child1 = parent1[..point].to_vec();
    child1.extend_from_slice(&parent2[point..]);
    let mut child2 = parent2[..point].to_vec();
    child2.extend_from_slice(&parent1[point..]);
    (child1, child2)
}

/// Mutate genes with given probability, keeping values within ranges.
fn mutate(genes: &mut [f64], ranges: &[ParameterRange], mutation_rate: f64, rng: &mut impl Rng) {
    for (gene, range) in genes.iter_mut().zip(ranges.iter()) {
        if rng.gen::<f64>() < mutation_rate {
            *gene = snap_to_step(rng.gen_range(range.min..=range.max), range);
        }
    }
}

/// Snap a value to the nearest step within a range.
fn snap_to_step(value: f64, range: &ParameterRange) -> f64 {
    if range.step <= 0.0 {
        return value;
    }
    let steps = ((value - range.min) / range.step).round();
    let snapped = range.min + steps * range.step;
    snapped.max(range.min).min(range.max)
}

/// Build a result placeholder for failed backtests.
fn build_failed_result(ranges: &[ParameterRange], values: &[f64]) -> OptimizationResult {
    let mut params = HashMap::new();
    for (range, &val) in ranges.iter().zip(values.iter()) {
        params.insert(range.display_name.clone(), val);
    }
    OptimizationResult {
        params,
        objective_value: f64::NEG_INFINITY,
        total_return_pct: 0.0,
        sharpe_ratio: 0.0,
        max_drawdown_pct: 0.0,
        total_trades: 0,
        profit_factor: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_grid_simple() {
        let ranges = vec![
            ParameterRange {
                rule_index: 0,
                param_name: "period".into(),
                display_name: "SMA Period".into(),
                min: 10.0,
                max: 30.0,
                step: 10.0,
            },
        ];
        let grid = generate_grid(&ranges).unwrap();
        assert_eq!(grid.len(), 3); // 10, 20, 30
        assert_eq!(grid[0], vec![10.0]);
        assert_eq!(grid[1], vec![20.0]);
        assert_eq!(grid[2], vec![30.0]);
    }

    #[test]
    fn test_generate_grid_cartesian() {
        let ranges = vec![
            ParameterRange {
                rule_index: 0,
                param_name: "period".into(),
                display_name: "Fast".into(),
                min: 5.0,
                max: 10.0,
                step: 5.0,
            },
            ParameterRange {
                rule_index: 0,
                param_name: "slow_period".into(),
                display_name: "Slow".into(),
                min: 20.0,
                max: 30.0,
                step: 5.0,
            },
        ];
        let grid = generate_grid(&ranges).unwrap();
        // 2 × 3 = 6 combinations
        assert_eq!(grid.len(), 6);
    }

    #[test]
    fn test_generate_grid_too_many() {
        let ranges = vec![
            ParameterRange {
                rule_index: 0,
                param_name: "period".into(),
                display_name: "P1".into(),
                min: 1.0,
                max: 300.0,
                step: 1.0,
            },
            ParameterRange {
                rule_index: 0,
                param_name: "fast_period".into(),
                display_name: "P2".into(),
                min: 1.0,
                max: 300.0,
                step: 1.0,
            },
        ];
        let result = generate_grid(&ranges);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::TooManyCombinations { count, limit } => {
                assert_eq!(count, 90000);
                assert_eq!(limit, MAX_COMBINATIONS);
            }
            _ => panic!("Expected TooManyCombinations error"),
        }
    }

    #[test]
    fn test_snap_to_step() {
        let range = ParameterRange {
            rule_index: 0,
            param_name: "period".into(),
            display_name: "P".into(),
            min: 5.0,
            max: 25.0,
            step: 5.0,
        };
        assert_eq!(snap_to_step(7.0, &range), 5.0);
        assert_eq!(snap_to_step(8.0, &range), 10.0);
        assert_eq!(snap_to_step(12.5, &range), 15.0);
        assert_eq!(snap_to_step(25.0, &range), 25.0);
    }

    #[test]
    fn test_extract_objective() {
        let metrics = BacktestMetrics {
            final_capital: 0.0,
            total_return_pct: 0.0,
            annualized_return_pct: 0.0,
            monthly_return_avg_pct: 0.0,
            sharpe_ratio: 1.5,
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
            win_rate_pct: 55.0,
            gross_profit: 0.0,
            gross_loss: 0.0,
            net_profit: 1000.0,
            profit_factor: 2.0,
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
        };

        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::TotalProfit), 1000.0);
        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::SharpeRatio), 1.5);
        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::ProfitFactor), 2.0);
        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::WinRate), 55.0);
    }
}
