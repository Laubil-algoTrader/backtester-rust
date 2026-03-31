/// NSGA-II multi-objective optimization for SR individuals.
///
/// Implements: non-dominated sort, crowding distance, binary tournament
/// selection, and a full generation step (selection → crossover → mutation).

use rand::Rng;

use crate::models::result::BacktestMetrics;
use crate::models::strategy::{StopLossType, TakeProfitType};
use crate::models::sr_result::{PoolLeaf, SrAtrRange, SrObjectives, SrStrategy};

use super::tree;

// ── Individual ────────────────────────────────────────────────────────────────

/// One individual in the NSGA-II population.
#[derive(Debug, Clone)]
pub struct SrIndividual {
    pub strategy: SrStrategy,
    pub objectives: Option<SrObjectives>,
    /// Cached BacktestMetrics from the last evaluation — reused by `build_front_item`
    /// to avoid re-running the backtest just to get complete metrics.
    pub metrics: Option<BacktestMetrics>,
    /// Pareto front rank (0 = best front). `usize::MAX` = not yet ranked.
    pub rank: usize,
    /// Crowding distance within the front. Higher = more isolated = preferred.
    pub crowding: f64,
}

impl SrIndividual {
    pub fn new(strategy: SrStrategy) -> Self {
        Self {
            strategy,
            objectives: None,
            metrics: None,
            rank: usize::MAX,
            crowding: 0.0,
        }
    }

    pub fn is_evaluated(&self) -> bool {
        self.objectives.is_some()
    }

    /// Returns `true` if this individual dominates `other`.
    ///
    /// An evaluated individual (has objectives) always dominates an unevaluated one
    /// (produced no trades). This prevents unevaluated individuals from polluting
    /// rank-0 and crowding out valid strategies.
    pub fn dominates(&self, other: &Self) -> bool {
        match (&self.objectives, &other.objectives) {
            (Some(a), Some(b)) => a.dominates(b),
            (Some(_), None) => true,  // evaluated beats unevaluated
            _ => false,
        }
    }

    /// Scalar fitness used for quick comparison (higher = better).
    pub fn scalar_fitness(&self) -> f64 {
        self.objectives.as_ref().map(|o| o.scalar()).unwrap_or(f64::NEG_INFINITY)
    }
}

// ── Non-Dominated Sort ────────────────────────────────────────────────────────

/// Assign Pareto rank to every individual.
///
/// Front 0 = non-dominated; front 1 = dominated only by front-0 individuals, etc.
/// Returns fronts as `Vec<Vec<usize>>` (indices into `pop`).
pub fn non_dominated_sort(pop: &mut Vec<SrIndividual>) -> Vec<Vec<usize>> {
    let n = pop.len();
    let mut dominated_by: Vec<Vec<usize>> = vec![vec![]; n]; // who dominates i
    let mut domination_count: Vec<usize> = vec![0; n]; // how many dominate i
    let mut fronts: Vec<Vec<usize>> = vec![vec![]];

    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            if pop[i].dominates(&pop[j]) {
                dominated_by[i].push(j);
            } else if pop[j].dominates(&pop[i]) {
                domination_count[i] += 1;
            }
        }
        if domination_count[i] == 0 {
            pop[i].rank = 0;
            fronts[0].push(i);
        }
    }

    let mut fi = 0;
    while !fronts[fi].is_empty() {
        let mut next_front: Vec<usize> = Vec::new();
        for &i in &fronts[fi] {
            for &j in &dominated_by[i] {
                domination_count[j] -= 1;
                if domination_count[j] == 0 {
                    pop[j].rank = fi + 1;
                    next_front.push(j);
                }
            }
        }
        fi += 1;
        fronts.push(next_front);
    }
    fronts.pop(); // last one is always empty
    fronts
}

// ── Crowding Distance ─────────────────────────────────────────────────────────

/// Compute and assign crowding distance within a single front.
pub fn crowding_distance(front: &[usize], pop: &mut Vec<SrIndividual>) {
    let n = front.len();
    if n <= 2 {
        for &i in front {
            pop[i].crowding = f64::INFINITY;
        }
        return;
    }

    // Reset
    for &i in front {
        pop[i].crowding = 0.0;
    }

    let num_obj = 5;
    for obj in 0..num_obj {
        // Sort front by this objective (ascending)
        let mut sorted = front.to_vec();
        sorted.sort_by(|&a, &b| {
            obj_val(&pop[a], obj)
                .partial_cmp(&obj_val(&pop[b], obj))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Boundary individuals get infinite crowding
        pop[sorted[0]].crowding = f64::INFINITY;
        pop[sorted[n - 1]].crowding = f64::INFINITY;

        let min_v = obj_val(&pop[sorted[0]], obj);
        let max_v = obj_val(&pop[sorted[n - 1]], obj);
        let range = max_v - min_v;
        if range < 1e-12 {
            continue;
        }
        for k in 1..n - 1 {
            let delta =
                (obj_val(&pop[sorted[k + 1]], obj) - obj_val(&pop[sorted[k - 1]], obj)) / range;
            if pop[sorted[k]].crowding.is_finite() {
                pop[sorted[k]].crowding += delta;
            }
        }
    }
}

fn obj_val(ind: &SrIndividual, obj: usize) -> f64 {
    match &ind.objectives {
        None => f64::NEG_INFINITY,
        Some(o) => match obj {
            0 => o.sharpe,
            1 => o.profit_factor,
            2 => o.temporal_consistency,
            3 => o.neg_max_drawdown,
            _ => o.expectancy_ratio,
        },
    }
}

// ── Tournament Selection ──────────────────────────────────────────────────────

/// Binary tournament: pick 2 distinct random individuals, prefer lower rank, break ties by higher crowding.
pub fn tournament_select<'a, R: Rng>(pop: &'a [SrIndividual], rng: &mut R) -> &'a SrIndividual {
    let a = rng.gen_range(0..pop.len());
    // Ensure b != a so the individual never competes against itself.
    let b = {
        let mut c = rng.gen_range(0..pop.len() - 1);
        if c >= a { c += 1; }
        c
    };
    let ia = &pop[a];
    let ib = &pop[b];
    if ia.rank < ib.rank || (ia.rank == ib.rank && ia.crowding > ib.crowding) {
        ia
    } else {
        ib
    }
}

// ── Generation Step ───────────────────────────────────────────────────────────

/// Produce `population_size` offspring from the current sorted population.
///
/// Uses tournament selection → subtree crossover (with probability `crossover_rate`)
/// → point mutation (with probability `mutation_rate`) on each of the 3 trees.
pub fn make_offspring<R: Rng>(
    pop: &[SrIndividual],
    pool: &[PoolLeaf],
    max_depth: usize,
    crossover_rate: f64,
    mutation_rate: f64,
    sl_range: Option<&SrAtrRange>,
    tp_range: Option<&SrAtrRange>,
    rng: &mut R,
) -> Vec<SrIndividual> {
    let target = pop.len();
    let mut offspring = Vec::with_capacity(target);

    while offspring.len() < target {
        let parent_a = tournament_select(pop, rng);
        let child_strategy = if rng.gen::<f64>() < crossover_rate {
            let parent_b = tournament_select(pop, rng);
            crossover_strategies(parent_a, parent_b, max_depth, pool, mutation_rate, sl_range, tp_range, rng)
        } else {
            mutate_strategy(parent_a, max_depth, pool, mutation_rate, sl_range, tp_range, rng)
        };
        offspring.push(SrIndividual::new(child_strategy));
    }

    offspring
}

fn crossover_strategies<R: Rng>(
    a: &SrIndividual,
    b: &SrIndividual,
    max_depth: usize,
    pool: &[PoolLeaf],
    mutation_rate: f64,
    sl_range: Option<&SrAtrRange>,
    tp_range: Option<&SrAtrRange>,
    rng: &mut R,
) -> SrStrategy {
    let mut s = a.strategy.clone();
    // Crossover each tree independently with 50% chance
    if rng.gen::<f64>() < 0.5 {
        s.entry_long = tree::subtree_crossover(&a.strategy.entry_long, &b.strategy.entry_long, rng);
    }
    if rng.gen::<f64>() < 0.5 {
        s.entry_short =
            tree::subtree_crossover(&a.strategy.entry_short, &b.strategy.entry_short, rng);
    }
    if rng.gen::<f64>() < 0.5 {
        s.exit = tree::subtree_crossover(&a.strategy.exit, &b.strategy.exit, rng);
    }
    // Threshold crossover
    if rng.gen::<f64>() < 0.3 {
        s.long_threshold = b.strategy.long_threshold;
    }
    if rng.gen::<f64>() < 0.3 {
        s.short_threshold = b.strategy.short_threshold;
    }
    // ATR SL/TP crossover: swap period + multiplier from parent B with 30% chance
    if rng.gen::<f64>() < 0.3 {
        if let (Some(sl_a), Some(sl_b)) = (&mut s.stop_loss, &b.strategy.stop_loss) {
            if sl_range.is_some() && sl_a.sl_type == StopLossType::ATR {
                sl_a.atr_period = sl_b.atr_period;
                sl_a.value = sl_b.value;
            }
        }
    }
    if rng.gen::<f64>() < 0.3 {
        if let (Some(tp_a), Some(tp_b)) = (&mut s.take_profit, &b.strategy.take_profit) {
            if tp_range.is_some() && tp_a.tp_type == TakeProfitType::ATR {
                tp_a.atr_period = tp_b.atr_period;
                tp_a.value = tp_b.value;
            }
        }
    }
    // Then maybe mutate
    apply_mutation(&mut s, max_depth, pool, mutation_rate, sl_range, tp_range, rng);
    s
}

fn mutate_strategy<R: Rng>(
    ind: &SrIndividual,
    max_depth: usize,
    pool: &[PoolLeaf],
    mutation_rate: f64,
    sl_range: Option<&SrAtrRange>,
    tp_range: Option<&SrAtrRange>,
    rng: &mut R,
) -> SrStrategy {
    let mut s = ind.strategy.clone();
    apply_mutation(&mut s, max_depth, pool, mutation_rate, sl_range, tp_range, rng);
    s
}

fn apply_mutation<R: Rng>(
    s: &mut SrStrategy,
    max_depth: usize,
    pool: &[PoolLeaf],
    mutation_rate: f64,
    sl_range: Option<&SrAtrRange>,
    tp_range: Option<&SrAtrRange>,
    rng: &mut R,
) {
    if rng.gen::<f64>() < mutation_rate {
        s.entry_long = tree::mutate(&s.entry_long, max_depth, pool, rng);
    }
    if rng.gen::<f64>() < mutation_rate {
        s.entry_short = tree::mutate(&s.entry_short, max_depth, pool, rng);
    }
    if rng.gen::<f64>() < mutation_rate {
        s.exit = tree::mutate(&s.exit, max_depth, pool, rng);
    }
    // Threshold mutation — step proportional to current value so small and large
    // thresholds both get meaningful perturbations.
    if rng.gen::<f64>() < mutation_rate * 0.5 {
        let step = (s.long_threshold.abs() * 0.2 + 1.0) * rng.gen_range(-1.0_f64..1.0);
        s.long_threshold += step;
    }
    if rng.gen::<f64>() < mutation_rate * 0.5 {
        let step = (s.short_threshold.abs() * 0.2 + 1.0) * rng.gen_range(-1.0_f64..1.0);
        s.short_threshold += step;
    }
    // ATR SL parameter mutation
    if let (Some(sl), Some(r)) = (&mut s.stop_loss, sl_range) {
        if sl.sl_type == StopLossType::ATR {
            if rng.gen::<f64>() < mutation_rate * 0.5 {
                let cur = sl.atr_period.unwrap_or(14) as i64;
                let step = rng.gen_range(-3_i64..=3);
                sl.atr_period = Some((cur + step).max(r.period_min as i64).min(r.period_max as i64) as usize);
            }
            if rng.gen::<f64>() < mutation_rate * 0.5 {
                let step = rng.gen_range(-0.3_f64..=0.3);
                sl.value = (sl.value + step).max(r.mult_min).min(r.mult_max);
            }
        }
    }
    // ATR TP parameter mutation
    if let (Some(tp), Some(r)) = (&mut s.take_profit, tp_range) {
        if tp.tp_type == TakeProfitType::ATR {
            if rng.gen::<f64>() < mutation_rate * 0.5 {
                let cur = tp.atr_period.unwrap_or(14) as i64;
                let step = rng.gen_range(-3_i64..=3);
                tp.atr_period = Some((cur + step).max(r.period_min as i64).min(r.period_max as i64) as usize);
            }
            if rng.gen::<f64>() < mutation_rate * 0.5 {
                let step = rng.gen_range(-0.3_f64..=0.3);
                tp.value = (tp.value + step).max(r.mult_min).min(r.mult_max);
            }
        }
    }
}

// ── NSGA-II Merge & Select ────────────────────────────────────────────────────

/// Merge parent and offspring populations, rank them, then select the best
/// `target_size` individuals to carry forward (elitism via crowded-comparison).
pub fn nsga2_select(
    mut combined: Vec<SrIndividual>,
    target_size: usize,
) -> Vec<SrIndividual> {
    // First pass: replace NaN objectives
    for ind in combined.iter_mut() {
        if let Some(obj) = &ind.objectives {
            if !obj.is_valid() {
                ind.objectives = None;
                ind.rank = usize::MAX;
                ind.crowding = 0.0;
            }
        }
    }

    let fronts = non_dominated_sort(&mut combined);

    // Compute crowding per front
    for front in &fronts {
        crowding_distance(front, &mut combined);
    }

    // Greedy selection: take fronts until we have enough
    let mut selected: Vec<SrIndividual> = Vec::with_capacity(target_size);
    for front in &fronts {
        if selected.len() + front.len() <= target_size {
            for &i in front {
                selected.push(combined[i].clone());
            }
        } else {
            // Last front: sort by crowding descending, take what fits
            let mut partial: Vec<usize> = front.clone();
            partial.sort_by(|&a, &b| {
                combined[b]
                    .crowding
                    .partial_cmp(&combined[a].crowding)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for &i in partial.iter().take(target_size - selected.len()) {
                selected.push(combined[i].clone());
            }
            break;
        }
        if selected.len() >= target_size {
            break;
        }
    }

    selected
}

/// Return the best Sharpe on the current Pareto front (rank 0).
pub fn best_front_sharpe(pop: &[SrIndividual]) -> f64 {
    pop.iter()
        .filter(|i| i.rank == 0)
        .filter_map(|i| i.objectives.as_ref())
        .map(|o| o.sharpe)
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Collect all rank-0 individuals (the Pareto front).
pub fn collect_pareto_front(pop: &[SrIndividual]) -> Vec<&SrIndividual> {
    let mut front: Vec<&SrIndividual> =
        pop.iter().filter(|i| i.rank == 0 && i.is_evaluated()).collect();
    // Sort by scalar fitness descending for a deterministic ordering
    front.sort_by(|a, b| {
        b.scalar_fitness()
            .partial_cmp(&a.scalar_fitness())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    front
}

/// Generate the initial random population.
pub fn random_population<R: Rng>(
    count: usize,
    config: &crate::models::sr_result::SrConfig,
    pool: &[PoolLeaf],
    rng: &mut R,
) -> Vec<SrIndividual> {
    (0..count)
        .map(|_| {
            let strategy = SrStrategy {
                entry_long: tree::generate_random(0, config.max_depth, pool, rng),
                // Wide threshold range: most indicators have values 0-100 or ±200,
                // so sample thresholds from a matching range.
                long_threshold: rng.gen_range(-50.0_f64..50.0),
                entry_short: tree::generate_random(0, config.max_depth, pool, rng),
                short_threshold: rng.gen_range(-50.0_f64..50.0),
                exit: tree::generate_random(0, config.max_depth, pool, rng),
                stop_loss: {
                    let mut sl = config.stop_loss.clone();
                    if let (Some(ref mut s), Some(r)) = (&mut sl, &config.sl_atr_range) {
                        if s.sl_type == StopLossType::ATR {
                            s.atr_period = Some(rng.gen_range(r.period_min..=r.period_max));
                            s.value = rng.gen_range(r.mult_min..=r.mult_max);
                        }
                    }
                    sl
                },
                take_profit: {
                    let mut tp = config.take_profit.clone();
                    if let (Some(ref mut t), Some(r)) = (&mut tp, &config.tp_atr_range) {
                        if t.tp_type == TakeProfitType::ATR {
                            t.atr_period = Some(rng.gen_range(r.period_min..=r.period_max));
                            t.value = rng.gen_range(r.mult_min..=r.mult_max);
                        }
                    }
                    tp
                },
                trailing_stop: config.trailing_stop.clone(),
                position_sizing: config.position_sizing.clone(),
                trading_costs: config.trading_costs.clone(),
                trade_direction: config.trade_direction,
                use_exit_formula: config.use_exit_formula,
                // Populated by evaluate_individual before first backtest run
                trading_hours: None,
                close_trades_at: None,
                max_trades_per_day: None,
            };
            SrIndividual::new(strategy)
        })
        .collect()
}
