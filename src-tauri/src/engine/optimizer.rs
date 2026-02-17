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
use crate::models::strategy::{
    BacktestConfig, CloseTradesAt, IndicatorParams, Strategy, TradingHours,
};

use super::executor::{self, SubBarData};

/// Maximum allowed combinations for Grid Search.
const MAX_COMBINATIONS: usize = 500_000;

/// Maximum results to return from optimization.
const MAX_RESULTS: usize = 50;

// ══════════════════════════════════════════════════════════════
// Shared helpers
// ══════════════════════════════════════════════════════════════

/// Apply parameter values to a strategy, returning a modified clone.
///
/// Each `ParameterRange` specifies a `param_source` to identify which rule group:
/// - `"long_entry"`, `"short_entry"`, `"long_exit"`, `"short_exit"` for indicator params
/// - `"indicator"` for backward compat (uses long_entry + long_exit concatenation)
/// - `"stop_loss"`, `"take_profit"`, `"trailing_stop"` for risk management params
/// - `"trading_hours"` for start/end hour/minute
/// - `"close_trades_at"` for force-close hour/minute
///
/// `rule_index` is 0-based within the specified rule group.
pub fn apply_params(strategy: &Strategy, ranges: &[ParameterRange], values: &[f64]) -> Strategy {
    let mut s = strategy.clone();

    for (range, &val) in ranges.iter().zip(values.iter()) {
        match range.param_source.as_str() {
            "stop_loss" => {
                if let Some(ref mut sl) = s.stop_loss {
                    match range.param_name.as_str() {
                        "value" => sl.value = val,
                        "atr_period" => sl.atr_period = Some(val.round() as usize),
                        _ => {}
                    }
                }
            }
            "take_profit" => {
                if let Some(ref mut tp) = s.take_profit {
                    match range.param_name.as_str() {
                        "value" => tp.value = val,
                        "atr_period" => tp.atr_period = Some(val.round() as usize),
                        _ => {}
                    }
                }
            }
            "trailing_stop" => {
                if let Some(ref mut ts) = s.trailing_stop {
                    match range.param_name.as_str() {
                        "value" => ts.value = val,
                        "atr_period" => ts.atr_period = Some(val.round() as usize),
                        _ => {}
                    }
                }
            }
            "trading_hours" => {
                let th = s.trading_hours.get_or_insert(TradingHours {
                    start_hour: 0, start_minute: 0, end_hour: 23, end_minute: 59,
                });
                let v = val.round() as u8;
                match range.param_name.as_str() {
                    "start_hour" => th.start_hour = v.min(23),
                    "start_minute" => th.start_minute = v.min(59),
                    "end_hour" => th.end_hour = v.min(23),
                    "end_minute" => th.end_minute = v.min(59),
                    _ => {}
                }
            }
            "close_trades_at" => {
                let ct = s.close_trades_at.get_or_insert(CloseTradesAt {
                    hour: 16, minute: 0,
                });
                let v = val.round() as u8;
                match range.param_name.as_str() {
                    "hour" => ct.hour = v.min(23),
                    "minute" => ct.minute = v.min(59),
                    _ => {}
                }
            }
            source => {
                // Indicator parameter — find the rule in the correct group
                let idx = range.rule_index as usize;
                let rule = match source {
                    "long_entry" => s.long_entry_rules.get_mut(idx),
                    "short_entry" => s.short_entry_rules.get_mut(idx),
                    "long_exit" => s.long_exit_rules.get_mut(idx),
                    "short_exit" => s.short_exit_rules.get_mut(idx),
                    _ => {
                        // Backward compat: "indicator" uses long_entry + long_exit
                        let le_len = s.long_entry_rules.len();
                        if idx < le_len {
                            s.long_entry_rules.get_mut(idx)
                        } else {
                            s.long_exit_rules.get_mut(idx - le_len)
                        }
                    }
                };

                if let Some(rule) = rule {
                    let updated = match range.operand_side.as_str() {
                        "left" => set_indicator_param(&mut rule.left_operand.indicator, &range.param_name, val),
                        "right" => set_indicator_param(&mut rule.right_operand.indicator, &range.param_name, val),
                        _ => {
                            set_indicator_param(&mut rule.left_operand.indicator, &range.param_name, val)
                                || set_indicator_param(&mut rule.right_operand.indicator, &range.param_name, val)
                        }
                    };

                    if !updated {
                        tracing::warn!(
                            "Could not apply param '{}' to rule index {} in group '{}'",
                            range.param_name,
                            range.rule_index,
                            source
                        );
                    }
                } else {
                    tracing::warn!(
                        "Rule index {} out of bounds for group '{}'",
                        range.rule_index,
                        source
                    );
                }
            }
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
/// For "minimize" objectives, the value is negated so that higher = better universally.
fn extract_objective(metrics: &BacktestMetrics, objective: &ObjectiveFunction) -> f64 {
    match objective {
        ObjectiveFunction::TotalProfit => metrics.net_profit,
        ObjectiveFunction::SharpeRatio => metrics.sharpe_ratio,
        ObjectiveFunction::ProfitFactor => metrics.profit_factor,
        ObjectiveFunction::WinRate => metrics.win_rate_pct,
        ObjectiveFunction::ReturnDdRatio => metrics.return_dd_ratio,
        ObjectiveFunction::MinStagnation => -(metrics.stagnation_bars as f64),
        ObjectiveFunction::MinUlcerIndex => -metrics.ulcer_index_pct,
    }
}

/// Build an OptimizationResult from backtest metrics and parameter values.
/// Uses the first objective as the primary `objective_value`.
fn build_result(
    ranges: &[ParameterRange],
    values: &[f64],
    metrics: &BacktestMetrics,
    objectives: &[ObjectiveFunction],
) -> OptimizationResult {
    let mut params = HashMap::new();
    for (range, &val) in ranges.iter().zip(values.iter()) {
        params.insert(range.display_name.clone(), val);
    }
    let primary = objectives.first().copied().unwrap_or(ObjectiveFunction::SharpeRatio);
    OptimizationResult {
        params,
        objective_value: extract_objective(metrics, &primary),
        composite_score: 0.0, // computed after all results are collected
        total_return_pct: metrics.total_return_pct,
        sharpe_ratio: metrics.sharpe_ratio,
        max_drawdown_pct: metrics.max_drawdown_pct,
        total_trades: metrics.total_trades,
        profit_factor: metrics.profit_factor,
        return_dd_ratio: metrics.return_dd_ratio,
        stagnation_bars: metrics.stagnation_bars,
        ulcer_index_pct: metrics.ulcer_index_pct,
        oos_results: Vec::new(),
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
    sub_bars: &SubBarData,
    strategy: &Strategy,
    config: &BacktestConfig,
    instrument: &InstrumentConfig,
    ranges: &[ParameterRange],
    objectives: &[ObjectiveFunction],
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
                sub_bars,
                &modified,
                config,
                instrument,
                cancel_flag,
                |_, _, _| {},
            );

            let current = counter.fetch_add(1, Ordering::Relaxed) + 1;

            match result {
                Ok(bt) => {
                    let opt_result = build_result(ranges, values, &bt.metrics, objectives);

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

    // Compute composite scores for multi-objective
    compute_composite_scores(&mut valid, objectives);

    // Sort by composite_score if multi-objective, otherwise by objective_value
    if objectives.len() > 1 {
        valid.sort_by(|a, b| b.composite_score.partial_cmp(&a.composite_score).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        valid.sort_by(|a, b| b.objective_value.partial_cmp(&a.objective_value).unwrap_or(std::cmp::Ordering::Equal));
    }
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
    sub_bars: &SubBarData,
    strategy: &Strategy,
    config: &BacktestConfig,
    instrument: &InstrumentConfig,
    ranges: &[ParameterRange],
    objectives: &[ObjectiveFunction],
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
                    sub_bars,
                    &modified,
                    config,
                    instrument,
                    cancel_flag,
                    |_, _, _| {},
                );

                match result {
                    Ok(bt) => {
                        let opt_result = build_result(ranges, &ind.genes, &bt.metrics, objectives);
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

    // Compute composite scores for multi-objective
    compute_composite_scores(&mut results, objectives);

    if objectives.len() > 1 {
        results.sort_by(|a, b| b.composite_score.partial_cmp(&a.composite_score).unwrap_or(std::cmp::Ordering::Equal));
    } else {
        results.sort_by(|a, b| b.objective_value.partial_cmp(&a.objective_value).unwrap_or(std::cmp::Ordering::Equal));
    }
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
        composite_score: f64::NEG_INFINITY,
        total_return_pct: 0.0,
        sharpe_ratio: 0.0,
        max_drawdown_pct: 0.0,
        total_trades: 0,
        profit_factor: 0.0,
        return_dd_ratio: 0.0,
        stagnation_bars: 0,
        ulcer_index_pct: 0.0,
        oos_results: Vec::new(),
    }
}

/// Compute composite scores for multi-objective optimization.
/// Normalizes each objective to [0, 1] using min-max across all results, then averages.
/// For single-objective, composite_score == objective_value (normalized to [0, 1]).
fn compute_composite_scores(results: &mut [OptimizationResult], objectives: &[ObjectiveFunction]) {
    if results.is_empty() || objectives.is_empty() {
        return;
    }

    // For single objective, just copy objective_value as composite
    if objectives.len() == 1 {
        for r in results.iter_mut() {
            r.composite_score = r.objective_value;
        }
        return;
    }

    // Extract raw objective values for each objective
    let num_objectives = objectives.len();
    let num_results = results.len();

    // Collect raw values: raw_values[obj_idx][result_idx]
    let mut raw_values: Vec<Vec<f64>> = Vec::with_capacity(num_objectives);
    for obj in objectives {
        let vals: Vec<f64> = results.iter().map(|r| {
            extract_objective_from_result(r, obj)
        }).collect();
        raw_values.push(vals);
    }

    // Normalize each objective to [0, 1] and compute average
    for i in 0..num_results {
        let mut score_sum = 0.0;
        for (j, _obj) in objectives.iter().enumerate() {
            let vals = &raw_values[j];
            let min = vals.iter().copied().fold(f64::INFINITY, f64::min);
            let max = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let range = max - min;
            let normalized = if range > 0.0 {
                (vals[i] - min) / range
            } else {
                0.5
            };
            score_sum += normalized;
        }
        results[i].composite_score = score_sum / num_objectives as f64;
    }
}

/// Extract an objective value directly from an OptimizationResult (without BacktestMetrics).
/// For "minimize" objectives, values are negated so higher = better.
fn extract_objective_from_result(r: &OptimizationResult, obj: &ObjectiveFunction) -> f64 {
    match obj {
        ObjectiveFunction::TotalProfit => r.total_return_pct * 100.0, // use return %
        ObjectiveFunction::SharpeRatio => r.sharpe_ratio,
        ObjectiveFunction::ProfitFactor => r.profit_factor,
        ObjectiveFunction::WinRate => 0.0, // not stored directly, use objective_value if primary
        ObjectiveFunction::ReturnDdRatio => r.return_dd_ratio,
        ObjectiveFunction::MinStagnation => -(r.stagnation_bars as f64),
        ObjectiveFunction::MinUlcerIndex => -r.ulcer_index_pct,
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
                operand_side: "left".into(),
                param_source: "indicator".into(),
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
                operand_side: "left".into(),
                param_source: "indicator".into(),
            },
            ParameterRange {
                rule_index: 0,
                param_name: "slow_period".into(),
                display_name: "Slow".into(),
                min: 20.0,
                max: 30.0,
                step: 5.0,
                operand_side: "left".into(),
                param_source: "indicator".into(),
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
                max: 800.0,
                step: 1.0,
                operand_side: "left".into(),
                param_source: "indicator".into(),
            },
            ParameterRange {
                rule_index: 0,
                param_name: "fast_period".into(),
                display_name: "P2".into(),
                min: 1.0,
                max: 800.0,
                step: 1.0,
                operand_side: "left".into(),
                param_source: "indicator".into(),
            },
        ];
        let result = generate_grid(&ranges);
        assert!(result.is_err());
        match result.unwrap_err() {
            AppError::TooManyCombinations { count, limit } => {
                assert_eq!(count, 640000);
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
            operand_side: "left".into(),
            param_source: "indicator".into(),
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
            stagnation_bars: 100,
            stagnation_time: String::new(),
            ulcer_index_pct: 3.5,
            return_dd_ratio: 2.5,
        };

        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::TotalProfit), 1000.0);
        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::SharpeRatio), 1.5);
        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::ProfitFactor), 2.0);
        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::WinRate), 55.0);
        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::ReturnDdRatio), 2.5);
        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::MinStagnation), -100.0);
        assert_eq!(extract_objective(&metrics, &ObjectiveFunction::MinUlcerIndex), -3.5);
    }
}
