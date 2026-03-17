//! Grammar-Based Genetic Programming engine for automated strategy generation.
//!
//! Implements an island-model GP that evolves trading strategies from scratch using
//! a grammar context derived from [`BuilderConfig`]. Each island maintains its own
//! population, with periodic migration between islands using ring topology.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Monotonically-increasing counter used for rule/group/strategy IDs during evolution.
/// Using a simple counter instead of UUID v4 eliminates CSPRNG reads and 36-char String
/// allocations for every rule/group created (~tens of thousands per generation).
/// IDs from the counter are globally unique across all threads for the lifetime of the
/// process, which is sufficient for React list keys and internal bookkeeping.
static BUILDER_ID_SEQ: AtomicU64 = AtomicU64::new(1);

use rand::seq::SliceRandom;
use rand::Rng;
use rayon::prelude::*;
use tracing::{info, warn};

use crate::errors::AppError;
use crate::models::builder::*;
use crate::models::candle::Candle;
use crate::models::config::InstrumentConfig;
use crate::models::result::{BacktestMetrics, EquityPoint};
use crate::models::strategy::*;

use super::executor::{self, SubBarData};
use super::strategy::{max_lookback, IndicatorCache, pre_compute_indicators_with_shared_cache};

// ══════════════════════════════════════════════════════════════
// Progress event
// ══════════════════════════════════════════════════════════════

/// Thread-safe wrapper around `SyncSender` to make it `Sync`.
/// `SyncSender::send(&self, ..)` is inherently thread-safe (it blocks when the
/// buffer is full), but the type doesn't implement `Sync` in the stdlib.
/// This wrapper lets us share `&SyncSender` across rayon's `par_iter_mut`.
struct SyncTx<T>(SyncSender<T>);
unsafe impl<T: Send> Sync for SyncTx<T> {}
impl<T> SyncTx<T> {
    fn send(&self, val: T) -> Result<(), std::sync::mpsc::SendError<T>> {
        self.0.send(val)
    }
}

/// Events emitted during a builder run for UI progress updates.
pub enum BuilderProgressEvent {
    /// Periodic runtime statistics (generation progress, throughput, etc.).
    Stats(BuilderRuntimeStats),
    /// Free-form log message.
    Log(String),
    /// A strategy that passed all filters and was accepted into the databank.
    StrategyFound(BuilderSavedStrategy),
    /// Per-island stats after each generation.
    IslandStats(BuilderIslandStats),
}

// ══════════════════════════════════════════════════════════════
// Internal structs
// ══════════════════════════════════════════════════════════════

/// An individual in the genetic population.
struct BuilderIndividual {
    strategy: Strategy,
    fingerprint: u64,
    fitness: f64,
    metrics: Option<BacktestMetrics>,
    mini_equity_curve: Vec<f64>,
    /// Hashed entry-time strings of every trade, used for behavioral niching
    /// (trade-overlap Jaccard distance). Empty until evaluated.
    trade_bar_hashes: Vec<u32>,
    /// Cached rule count (total across all flat rules and groups).
    /// Updated once at construction time to avoid O(rules) re-walks in the fitness loop.
    rule_count: usize,
}

/// One island in the island-model GP.
struct Island {
    id: usize,
    population: Vec<BuilderIndividual>,
    /// Set of fingerprints for O(1) duplicate detection (kept in sync with `population`).
    fingerprint_set: std::collections::HashSet<u64>,
    generation: usize,
    best_fitness: f64,
    /// Best raw (pre-sharing) fitness, used for stagnation detection.
    /// Sharing-adjusted fitness fluctuates with diversity changes even when the
    /// underlying best strategy hasn't changed, causing false stagnation signals.
    best_raw_fitness: f64,
    stagnation_count: usize,
    /// Effective mutation probability — adapts per-island based on diversity.
    /// Starts at the configured value; increases when diversity drops, decays otherwise.
    effective_mutation_prob: f64,
    /// Per-island grammar: starts as a copy of the global grammar and is
    /// updated in-place each generation by meta-learning.
    grammar: GrammarContext,
}

/// Grammar context derived from [`BuilderConfig`] at startup.
/// Pre-filters enabled indicators with weights for weighted random selection.
/// Each island gets its own clone; meta-learning updates it in-place over time.
#[derive(Clone)]
struct GrammarContext {
    direction: BuilderDirection,
    min_entry_rules: usize,
    max_entry_rules: usize,
    min_exit_rules: usize,
    max_exit_rules: usize,
    max_lookback: usize,
    period_min: usize,
    period_max: usize,
    period_step: usize,
    /// Stop loss: enabled = can be generated; required = always included.
    sl_enabled: bool,
    sl_required: bool,
    sl_type: BuilderSLType,
    sl_coeff_range: (f64, f64),
    sl_coeff_step: f64,
    sl_atr_period_range: (usize, usize),
    sl_atr_period_step: usize,
    /// Take profit: enabled = can be generated; required = always included.
    tp_enabled: bool,
    tp_required: bool,
    tp_type: BuilderTPType,
    tp_coeff_range: (f64, f64),
    tp_coeff_step: f64,
    tp_atr_period_range: (usize, usize),
    tp_atr_period_step: usize,
    /// Trailing stop: enabled = can be generated; required = always included.
    ts_enabled: bool,
    ts_required: bool,
    /// Entry order types enabled by building blocks (Market is always included as fallback).
    enabled_order_types: Vec<OrderType>,
    /// Exit after N bars: enabled/required flags.
    exit_after_bars_enabled: bool,
    exit_after_bars_required: bool,
    /// Move SL to breakeven: enabled/required flags.
    move_sl_be_enabled: bool,
    move_sl_be_required: bool,
    enabled_indicators: Vec<(IndicatorType, usize)>,
    /// Per-indicator period override: min, max, step. Falls back to global if absent.
    indicator_period_overrides: std::collections::HashMap<IndicatorType, (usize, usize, usize)>,
    comparators: Vec<Comparator>,
    price_fields: Vec<PriceField>,
    position_sizing: PositionSizing,
    trading_costs: TradingCosts,
    trading_hours: Option<TradingHours>,
    close_trades_at: Option<CloseTradesAt>,
    max_daily_trades: Option<u32>,
    trade_direction: TradeDirection,
    /// Enable exploration→exploitation phase shift based on generation progress.
    phase_based: bool,
    /// Enabled order-price indicators with weights for random selection.
    enabled_order_price_indicators: Vec<(IndicatorType, usize)>,
    order_price_base_stop: PriceField,
    order_price_base_limit: PriceField,
    order_price_multiplier_range: (f64, f64),
    order_price_multiplier_step: f64,
    // ── Meta-learning state (per-island copy, updated each generation) ──
    /// Indicator weights adapted by meta-learning. Starts as a copy of
    /// `enabled_indicators`; each island updates its own copy independently.
    adapted_indicators: Vec<(IndicatorType, usize)>,
    /// Observed mean period per indicator type from top performers.
    /// Used to bias `random_indicator_params` toward learned-good periods.
    period_means: std::collections::HashMap<IndicatorType, f64>,
    /// Number of meta-learning updates performed on this island's grammar.
    meta_updates: usize,
}

// ══════════════════════════════════════════════════════════════
// GrammarContext construction
// ══════════════════════════════════════════════════════════════

impl GrammarContext {
    /// Build grammar context from the full builder configuration.
    fn from_config(config: &BuilderConfig) -> Self {
        let wtb = &config.what_to_build;

        // Filter enabled indicators and collect weights
        let enabled_indicators: Vec<(IndicatorType, usize)> = config
            .building_blocks
            .indicators
            .iter()
            .filter(|b| b.enabled)
            .map(|b| (b.indicator_type, (b.weight * 100.0).max(1.0) as usize))
            .collect();

        // ── Exit type flags from building blocks ─────────────────────────────
        let find_exit = |t: BuilderExitType| -> (bool, bool) {
            config.building_blocks.exit_types.iter()
                .find(|e| e.exit_type == t)
                .map(|e| (e.enabled, e.required))
                .unwrap_or((true, false)) // default: enabled, not required
        };
        let (sl_bb_enabled, sl_bb_required)  = find_exit(BuilderExitType::StopLoss);
        let (tp_bb_enabled, tp_bb_required)  = find_exit(BuilderExitType::ProfitTarget);
        let (ts_bb_enabled, ts_bb_required)  = find_exit(BuilderExitType::TrailingStop);
        let (er_bb_enabled, er_bb_required)  = find_exit(BuilderExitType::ExitRule);
        let (exit_after_bars_enabled, exit_after_bars_required) = find_exit(BuilderExitType::ExitAfterBars);
        let (move_sl_be_enabled, move_sl_be_required) = find_exit(BuilderExitType::MoveSlBe);

        // ── Order types from building blocks ─────────────────────────────────
        // Market is always available; Limit/Stop are added if enabled.
        let mut enabled_order_types = vec![OrderType::Market];
        for ot_block in &config.building_blocks.order_types {
            if ot_block.enabled {
                match ot_block.order_type {
                    BuilderOrderType::Limit => {
                        if !enabled_order_types.contains(&OrderType::Limit) {
                            enabled_order_types.push(OrderType::Limit);
                        }
                    }
                    BuilderOrderType::Stop => {
                        if !enabled_order_types.contains(&OrderType::Stop) {
                            enabled_order_types.push(OrderType::Stop);
                        }
                    }
                    BuilderOrderType::Market => {} // Market already added by default
                }
            }
        }
        // Combine wtb required flags with building blocks required flags (OR)
        let sl_required = (wtb.sl_required || sl_bb_required) && sl_bb_enabled;
        let tp_required = (wtb.tp_required || tp_bb_required) && tp_bb_enabled;
        // Exit rules: apply building-blocks enable/required on top of wtb min/max
        let (min_exit_rules, max_exit_rules) = if !er_bb_enabled {
            (0, 0)
        } else if er_bb_required {
            (wtb.min_exit_rules.max(1), wtb.max_exit_rules.max(1))
        } else {
            (wtb.min_exit_rules, wtb.max_exit_rules)
        };

        // ── Order price indicators from building blocks ───────────────────────
        let enabled_order_price_indicators: Vec<(IndicatorType, usize)> = config
            .building_blocks
            .order_price_indicators
            .iter()
            .filter(|b| b.enabled)
            .map(|b| (b.indicator_type, (b.weight * 100.0).max(1.0) as usize))
            .collect();

        let parse_price_field = |s: &str| -> PriceField {
            match s {
                "open"  => PriceField::Open,
                "high"  => PriceField::High,
                "low"   => PriceField::Low,
                _       => PriceField::Close,
            }
        };
        let order_price_base_stop  = parse_price_field(&config.building_blocks.order_price_base_stop);
        let order_price_base_limit = parse_price_field(&config.building_blocks.order_price_base_limit);

        // Use the first enabled block's multiplier range as representative, or defaults
        let (order_price_multiplier_range, order_price_multiplier_step) = config
            .building_blocks
            .order_price_indicators
            .iter()
            .find(|b| b.enabled)
            .map(|b| ((b.multiplier_min, b.multiplier_max), b.multiplier_step.max(0.1)))
            .unwrap_or(((0.5, 2.0), 0.25));

        // Per-indicator period overrides (only for indicators that have a custom range set)
        let indicator_period_overrides: std::collections::HashMap<IndicatorType, (usize, usize, usize)> =
            config.building_blocks.indicators.iter()
                .filter(|b| b.period_min.is_some())
                .map(|b| {
                    let pmin = b.period_min.unwrap_or(2).max(2);
                    let pmax = b.period_max.unwrap_or(pmin + 1).max(pmin + 1);
                    let pstep = b.period_step.unwrap_or(1).max(1);
                    (b.indicator_type, (pmin, pmax, pstep))
                })
                .collect();

        let comparators = vec![
            Comparator::GreaterThan,
            Comparator::LessThan,
            Comparator::GreaterOrEqual,
            Comparator::LessOrEqual,
            Comparator::CrossAbove,
            Comparator::CrossBelow,
        ];

        let price_fields = vec![
            PriceField::Open,
            PriceField::High,
            PriceField::Low,
            PriceField::Close,
        ];

        // Map direction
        let trade_direction = match wtb.direction {
            BuilderDirection::LongOnly => TradeDirection::Long,
            BuilderDirection::ShortOnly => TradeDirection::Short,
            BuilderDirection::BothSymmetric | BuilderDirection::BothAsymmetric => {
                TradeDirection::Both
            }
        };

        // Map money management to position sizing
        let mm = &config.money_management;
        let position_sizing = match mm.method {
            BuilderMMMethod::FixedSize => PositionSizing {
                sizing_type: PositionSizingType::FixedLots,
                value: mm.size_if_no_mm,
                decrease_factor: 0.9,
            },
            BuilderMMMethod::RiskFixedBalance | BuilderMMMethod::RiskFixedAccount => {
                PositionSizing {
                    sizing_type: PositionSizingType::RiskBased,
                    value: mm.risked_money,
                    decrease_factor: 0.9,
                }
            }
            BuilderMMMethod::FixedAmount => PositionSizing {
                sizing_type: PositionSizingType::FixedAmount,
                value: mm.risked_money,
                decrease_factor: 0.9,
            },
            BuilderMMMethod::CryptoByPrice | BuilderMMMethod::StocksByPrice => PositionSizing {
                sizing_type: PositionSizingType::FixedAmount,
                value: mm.risked_money,
                decrease_factor: 0.9,
            },
            // No true Martingale (increase-after-loss) implementation exists yet.
            // Use FixedLots as a neutral fallback to avoid inverting user intent.
            BuilderMMMethod::SimpleMartingale => PositionSizing {
                sizing_type: PositionSizingType::FixedLots,
                value: mm.risked_money.max(0.01),
                decrease_factor: 1.0,
            },
        };

        // Trading costs from data config
        let trading_costs = TradingCosts {
            spread_pips: config.data_config.spread_pips,
            commission_type: CommissionType::FixedPerLot,
            commission_value: 0.0,
            slippage_pips: config.data_config.slippage_pips,
            slippage_random: false,
        };

        // Trading hours from trading options
        let trading_hours = if config.trading_options.limit_time_range {
            let (sh, sm) =
                parse_hhmm(&config.trading_options.time_range_from).unwrap_or((0, 0));
            let (eh, em) =
                parse_hhmm(&config.trading_options.time_range_to).unwrap_or((23, 59));
            Some(TradingHours {
                start_hour: sh,
                start_minute: sm,
                end_hour: eh,
                end_minute: em,
            })
        } else {
            None
        };

        // Close trades at end of day
        let close_trades_at = if config.trading_options.exit_at_end_of_day {
            let (h, m) =
                parse_hhmm(&config.trading_options.end_of_day_exit_time).unwrap_or((23, 59));
            Some(CloseTradesAt {
                hour: h,
                minute: m,
            })
        } else {
            None
        };

        let max_daily_trades = if config.trading_options.max_trades_per_day > 0 {
            Some(config.trading_options.max_trades_per_day as u32)
        } else {
            None
        };

        GrammarContext {
            direction: wtb.direction,
            min_entry_rules: wtb.min_entry_rules.max(1),
            max_entry_rules: wtb.max_entry_rules.max(1),
            min_exit_rules,
            max_exit_rules,
            max_lookback: wtb.max_lookback,
            period_min: wtb.indicator_period_min.max(2),
            period_max: wtb.indicator_period_max.max(3),
            period_step: wtb.indicator_period_step.max(1),
            sl_enabled: sl_bb_enabled,
            sl_required,
            sl_type: wtb.sl_type,
            sl_coeff_range: (wtb.sl_coeff_min, wtb.sl_coeff_max),
            sl_coeff_step: wtb.sl_coeff_step.max(0.01),
            sl_atr_period_range: (wtb.sl_atr_period_min.max(2), wtb.sl_atr_period_max.max(3)),
            sl_atr_period_step: wtb.sl_atr_period_step.max(1),
            tp_enabled: tp_bb_enabled,
            tp_required,
            tp_type: wtb.tp_type,
            tp_coeff_range: (wtb.tp_coeff_min, wtb.tp_coeff_max),
            tp_coeff_step: wtb.tp_coeff_step.max(0.01),
            tp_atr_period_range: (wtb.tp_atr_period_min.max(2), wtb.tp_atr_period_max.max(3)),
            tp_atr_period_step: wtb.tp_atr_period_step.max(1),
            ts_enabled: ts_bb_enabled,
            ts_required: ts_bb_required,
            enabled_order_types,
            exit_after_bars_enabled,
            exit_after_bars_required,
            move_sl_be_enabled,
            move_sl_be_required,
            enabled_order_price_indicators,
            order_price_base_stop,
            order_price_base_limit,
            order_price_multiplier_range,
            order_price_multiplier_step,
            adapted_indicators: enabled_indicators.clone(),
            enabled_indicators,
            indicator_period_overrides,
            comparators,
            price_fields,
            position_sizing,
            trading_costs,
            trading_hours,
            close_trades_at,
            max_daily_trades,
            trade_direction,
            phase_based: config.genetic_options.phase_based_adaptation,
            period_means: std::collections::HashMap::new(),
            meta_updates: 0,
        }
    }

    /// Create a minimal probe strategy for cache warm-up purposes.
    /// The strategy contains a single rule that references the given indicator,
    /// which is enough to trigger `pre_compute_indicators_with_shared_cache`
    /// to populate the persistent cache for that (indicator_type, period) pair.
    pub(crate) fn make_single_indicator_probe(
        &self,
        ind_type: IndicatorType,
        period: usize,
    ) -> Option<Strategy> {
        let ind_cfg = IndicatorConfig {
            indicator_type: ind_type,
            params: IndicatorParams {
                period: Some(period),
                ..Default::default()
            },
            output_field: None,
            cached_hash: 0,
        };
        let left = Operand {
            operand_type: OperandType::Indicator,
            indicator: Some(ind_cfg),
            price_field: None,
            constant_value: None,
            time_field: None,
            candle_pattern: None,
            offset: None,
            compound_left: None,
            compound_op: None,
            compound_right: None,
        };
        let right = Operand {
            operand_type: OperandType::Constant,
            indicator: None,
            price_field: None,
            constant_value: Some(0.0),
            time_field: None,
            candle_pattern: None,
            offset: None,
            compound_left: None,
            compound_op: None,
            compound_right: None,
        };
        let rule = Rule {
            id: "probe".to_string(),
            left_operand: left,
            comparator: Comparator::GreaterThan,
            right_operand: right,
            logical_operator: Some(LogicalOperator::And),
        };
        let strat = Strategy {
            id: "probe".to_string(),
            name: "probe".to_string(),
            created_at: String::new(),
            updated_at: String::new(),
            long_entry_rules: vec![rule],
            short_entry_rules: vec![],
            long_exit_rules: vec![],
            short_exit_rules: vec![],
            long_entry_groups: vec![],
            short_entry_groups: vec![],
            long_exit_groups: vec![],
            short_exit_groups: vec![],
            position_sizing: PositionSizing {
                sizing_type: PositionSizingType::FixedLots,
                value: 0.01,
                decrease_factor: 1.0,
            },
            stop_loss: None,
            take_profit: None,
            trailing_stop: None,
            trading_costs: self.trading_costs.clone(),
            trade_direction: TradeDirection::Long,
            trading_hours: None,
            max_daily_trades: None,
            close_trades_at: None,
            entry_order: OrderType::Market,
            entry_order_offset_pips: 0.0,
            close_after_bars: None,
            move_sl_to_be: false,
            entry_order_indicator: None,
        };
        Some(strat)
    }
}

// ══════════════════════════════════════════════════════════════
// Helper utilities
// ══════════════════════════════════════════════════════════════

/// Parse "HH:MM" to (hour, minute). Returns None on failure.
fn parse_hhmm(s: &str) -> Option<(u8, u8)> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() >= 2 {
        let h = parts[0].parse::<u8>().ok()?;
        let m = parts[1].parse::<u8>().ok()?;
        Some((h, m))
    } else {
        None
    }
}

/// Generate a unique ID string for use during evolution.
/// Uses an AtomicU64 counter — no CSPRNG, no heap format overhead.
/// IDs are globally unique for the lifetime of the process (sufficient for React keys).
fn gen_id() -> String {
    BUILDER_ID_SEQ.fetch_add(1, Ordering::Relaxed).to_string()
}

/// Get the current UTC datetime as an ISO-8601 string.
fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Pick a random value from a weighted list of `(item, weight)` pairs.
fn weighted_choice<'a, T>(
    items: &'a [(T, usize)],
    rng: &mut impl Rng,
) -> &'a T {
    let total: usize = items.iter().map(|(_, w)| w).sum();
    if total == 0 {
        return &items[0].0;
    }
    let mut pick = rng.gen_range(0..total);
    for (item, weight) in items {
        if pick < *weight {
            return item;
        }
        pick -= weight;
    }
    &items[items.len() - 1].0
}

/// Generate a random f64 in a range.
fn rand_range(rng: &mut impl Rng, min: f64, max: f64) -> f64 {
    if (max - min).abs() < f64::EPSILON {
        return min;
    }
    rng.gen_range(min..=max)
}

/// Generate a random usize in [min, max].
fn rand_period(rng: &mut impl Rng, min: usize, max: usize) -> usize {
    if min >= max {
        return min;
    }
    rng.gen_range(min..=max)
}

fn rand_period_step(rng: &mut impl Rng, min: usize, max: usize, step: usize) -> usize {
    if step <= 1 || min >= max {
        return rand_period(rng, min, max);
    }
    let count = (max - min) / step;
    min + rng.gen_range(0..=count) * step
}

/// Snap `value` to the nearest step-aligned grid point within [min, max].
fn snap_to_step_usize(value: usize, min: usize, max: usize, step: usize) -> usize {
    if step <= 1 {
        return value.clamp(min, max);
    }
    let steps = (value.saturating_sub(min) + step / 2) / step;
    (min + steps * step).min(max)
}

/// Snap `value` to the nearest step-aligned grid point within [min, max].
fn snap_to_step_f64(value: f64, min: f64, max: f64, step: f64) -> f64 {
    if step < f64::EPSILON {
        return value.clamp(min, max);
    }
    let steps = ((value - min) / step).round() as i64;
    (min + steps as f64 * step).clamp(min, max)
}

// ══════════════════════════════════════════════════════════════
// Fingerprint
// ══════════════════════════════════════════════════════════════


/// Combined fingerprint + rule count in a single tree traversal.
fn fingerprint_and_count(strategy: &Strategy) -> (u64, usize) {
    let mut hasher = DefaultHasher::new();
    let mut count = 0usize;

    macro_rules! hash_rules_vec {
        ($rules:expr, $sep:expr) => {
            for rule in $rules { hash_rule(rule, &mut hasher); count += 1; }
            hasher.write_u8($sep);
        };
    }
    macro_rules! hash_groups_vec {
        ($groups:expr, $sep:expr) => {
            for g in $groups {
                g.internal.hash(&mut hasher);
                for rule in &g.rules { hash_rule(rule, &mut hasher); count += 1; }
            }
            hasher.write_u8($sep);
        };
    }

    hash_rules_vec!(&strategy.long_entry_rules,  0xFF);
    hash_rules_vec!(&strategy.short_entry_rules, 0xFE);
    hash_rules_vec!(&strategy.long_exit_rules,   0xFD);
    hash_rules_vec!(&strategy.short_exit_rules,  0xFC);
    hash_groups_vec!(&strategy.long_entry_groups,  0xFB);
    hash_groups_vec!(&strategy.short_entry_groups, 0xFA);
    hash_groups_vec!(&strategy.long_exit_groups,   0xF9);
    hash_groups_vec!(&strategy.short_exit_groups,  0xF8);

    if let Some(ref sl) = strategy.stop_loss {
        sl.sl_type.hash(&mut hasher);
        (sl.value * 1000.0).round().to_bits().hash(&mut hasher);
    }
    if let Some(ref tp) = strategy.take_profit {
        tp.tp_type.hash(&mut hasher);
        (tp.value * 1000.0).round().to_bits().hash(&mut hasher);
    }

    (hasher.finish(), count)
}

fn hash_rule(rule: &Rule, hasher: &mut DefaultHasher) {
    hash_operand(&rule.left_operand, hasher);
    rule.comparator.hash(hasher);
    hash_operand(&rule.right_operand, hasher);
    if let Some(lo) = rule.logical_operator {
        lo.hash(hasher);
    }
}

fn hash_operand(op: &Operand, hasher: &mut DefaultHasher) {
    op.operand_type.hash(hasher);
    if let Some(ref ind) = op.indicator {
        ind.indicator_type.hash(hasher);
        if let Some(p) = ind.params.period {
            p.hash(hasher);
        }
        if let Some(p) = ind.params.fast_period {
            p.hash(hasher);
        }
        if let Some(p) = ind.params.slow_period {
            p.hash(hasher);
        }
        if let Some(ref of_) = ind.output_field {
            of_.hash(hasher);
        }
    }
    if let Some(pf) = op.price_field {
        pf.hash(hasher);
    }
    if let Some(cv) = op.constant_value {
        (cv * 10000.0).round().to_bits().hash(hasher);
    }
    if let Some(off) = op.offset {
        off.hash(hasher);
    }
    // Recurse into compound sub-operands so different compound structures get different fingerprints.
    if let Some(ref left) = op.compound_left {
        hash_operand(left, hasher);
    }
    if let Some(ref cop) = op.compound_op {
        cop.hash(hasher);
    }
    if let Some(ref right) = op.compound_right {
        hash_operand(right, hasher);
    }
}

// ══════════════════════════════════════════════════════════════
// Comparator mirroring
// ══════════════════════════════════════════════════════════════

/// Mirror a comparator for symmetric long/short rule conversion.
fn mirror_comparator(c: Comparator) -> Comparator {
    match c {
        Comparator::GreaterThan => Comparator::LessThan,
        Comparator::LessThan => Comparator::GreaterThan,
        Comparator::GreaterOrEqual => Comparator::LessOrEqual,
        Comparator::LessOrEqual => Comparator::GreaterOrEqual,
        Comparator::CrossAbove => Comparator::CrossBelow,
        Comparator::CrossBelow => Comparator::CrossAbove,
        Comparator::Equal => Comparator::Equal,
    }
}

// ══════════════════════════════════════════════════════════════
// Random operand generation
// ══════════════════════════════════════════════════════════════

// ══════════════════════════════════════════════════════════════
// Phase-adaptive helpers
// ══════════════════════════════════════════════════════════════

/// Returns (indicator_weight, price_weight, constant_weight) for the given phase.
/// phase 0.0 = first generation (exploration), 1.0 = last generation (exploitation).
/// Early: more constants for diversity. Late: more indicators for fine-tuning.
fn operand_weights_for_phase(phase: f64) -> (u32, u32, u32) {
    let ind = (55.0 + phase * 25.0).round() as u32; // 55 → 80
    let con = (25.0 - phase * 20.0).round() as u32; // 25 → 5
    let pri = 100u32.saturating_sub(ind + con);      // 20 → 15
    (ind, pri, con)
}

/// Returns the AND probability (0–100) for the given phase.
/// Early: 50% AND/OR mix (exploration). Late: 80% AND (exploitation/precision).
fn and_prob_for_phase(phase: f64) -> u32 {
    (50.0 + phase * 30.0).round() as u32 // 50 → 80
}

/// Generate a random indicator operand with appropriate parameters.
fn random_indicator_operand(grammar: &GrammarContext, rng: &mut impl Rng) -> Operand {
    let indicator_type = if grammar.adapted_indicators.is_empty() {
        IndicatorType::SMA
    } else {
        *weighted_choice(&grammar.adapted_indicators, rng)
    };

    let (params, output_field) = random_indicator_params(indicator_type, grammar, rng);

    let offset = if grammar.max_lookback > 0 && rng.gen_bool(0.3) {
        Some(rng.gen_range(1..=grammar.max_lookback))
    } else {
        None
    };

    Operand {
        operand_type: OperandType::Indicator,
        indicator: Some(IndicatorConfig {
            indicator_type,
            params,
            output_field,
            cached_hash: 0,
        }),
        price_field: None,
        constant_value: None,
        time_field: None,
        candle_pattern: None,
        offset,
        compound_left: None,
        compound_op: None,
        compound_right: None,
    }
}

/// Generate a random compound operand: `left OP right` where both sub-operands
/// are non-compound (depth = 1 constraint).
fn random_compound_operand(grammar: &GrammarContext, rng: &mut impl Rng, phase: f64) -> Operand {
    let left = random_simple_operand(grammar, rng, phase);
    let right = random_simple_operand(grammar, rng, phase);
    let op = match rng.gen_range(0..4u32) {
        0 => ArithmeticOp::Add,
        1 => ArithmeticOp::Sub,
        2 => ArithmeticOp::Mul,
        _ => ArithmeticOp::Div,
    };
    Operand {
        operand_type: OperandType::Compound,
        indicator: None,
        price_field: None,
        constant_value: None,
        time_field: None,
        candle_pattern: None,
        offset: None,
        compound_left: Some(Box::new(left)),
        compound_op: Some(op),
        compound_right: Some(Box::new(right)),
    }
}

/// Generate a simple (non-compound) operand — used inside compound operands to prevent nesting.
fn random_simple_operand(grammar: &GrammarContext, rng: &mut impl Rng, phase: f64) -> Operand {
    let (ind_w, pri_w, _) = if grammar.phase_based {
        operand_weights_for_phase(phase)
    } else {
        (70, 20, 10)
    };
    let roll = rng.gen_range(0..100u32);
    if roll < ind_w {
        random_indicator_operand(grammar, rng)
    } else if roll < ind_w + pri_w {
        let pf = grammar.price_fields.choose(rng).copied().unwrap_or(PriceField::Close);
        Operand {
            operand_type: OperandType::Price,
            indicator: None,
            price_field: Some(pf),
            constant_value: None,
            time_field: None,
            candle_pattern: None,
            offset: None,
            compound_left: None,
            compound_op: None,
            compound_right: None,
        }
    } else {
        let value = if rng.gen_bool(0.6) {
            rand_range(rng, 0.0, 100.0)
        } else {
            rand_range(rng, -100.0, 100.0)
        };
        Operand {
            operand_type: OperandType::Constant,
            indicator: None,
            price_field: None,
            constant_value: Some((value * 100.0).round() / 100.0),
            time_field: None,
            candle_pattern: None,
            offset: None,
            compound_left: None,
            compound_op: None,
            compound_right: None,
        }
    }
}

/// Generate a period biased toward a canonical (industry-standard) value.
/// With 60% probability picks from [canonical-spread, canonical+spread] ∩ [pmin, pmax].
/// With 40% probability picks uniformly from [pmin, pmax].
fn biased_period(rng: &mut impl Rng, canonical: usize, spread: usize, pmin: usize, pmax: usize) -> usize {
    if rng.gen_bool(0.6) {
        let lo = canonical.saturating_sub(spread).max(pmin);
        let hi = (canonical + spread).min(pmax);
        if lo >= hi { return rng.gen_range(pmin..=pmax.max(pmin)); }
        rng.gen_range(lo..=hi)
    } else {
        rng.gen_range(pmin..=pmax.max(pmin))
    }
}

/// Generate appropriate parameters for a given indicator type.
fn random_indicator_params(
    indicator_type: IndicatorType,
    grammar: &GrammarContext,
    rng: &mut impl Rng,
) -> (IndicatorParams, Option<String>) {
    // Use per-indicator override if set, otherwise fall back to global range.
    let (pmin, pmax, pstep) = grammar.indicator_period_overrides
        .get(&indicator_type)
        .copied()
        .unwrap_or((grammar.period_min, grammar.period_max, grammar.period_step));

    // If meta-learning has observed a mean period for this indicator type,
    // use it as the canonical value for biased sampling (overrides the static default).
    let learned = grammar.period_means.get(&indicator_type).copied();
    let canonical = |default: usize| -> usize {
        learned.map(|m| (m.round() as usize).clamp(pmin, pmax)).unwrap_or(default)
    };

    match indicator_type {
        // Biased single-period: favor industry-standard values (60% near canonical, 40% full range)
        IndicatorType::SMA | IndicatorType::EMA | IndicatorType::HullMA | IndicatorType::LinearRegression => {
            let period = biased_period(rng, canonical(20), 10, pmin, pmax);
            (IndicatorParams { period: Some(period), ..Default::default() }, None)
        }
        IndicatorType::RSI | IndicatorType::DeMarker | IndicatorType::LaguerreRSI => {
            let period = biased_period(rng, canonical(14), 4, pmin, pmax);
            (IndicatorParams { period: Some(period), ..Default::default() }, None)
        }
        IndicatorType::ATR | IndicatorType::TrueRange | IndicatorType::UlcerIndex => {
            let period = biased_period(rng, canonical(14), 5, pmin, pmax);
            (IndicatorParams { period: Some(period), ..Default::default() }, None)
        }
        IndicatorType::CCI => {
            let period = biased_period(rng, canonical(20), 7, pmin, pmax);
            (IndicatorParams { period: Some(period), ..Default::default() }, None)
        }

        // Other single-period indicators: use learned canonical if available, else uniform
        IndicatorType::ROC
        | IndicatorType::WilliamsR
        | IndicatorType::Momentum
        | IndicatorType::StdDev
        | IndicatorType::BarRange
        | IndicatorType::BiggestRange
        | IndicatorType::SmallestRange
        | IndicatorType::HighestInRange
        | IndicatorType::LowestInRange
        | IndicatorType::Reflex
        | IndicatorType::BearsPower
        | IndicatorType::BullsPower
        | IndicatorType::Fractal
        | IndicatorType::Fibonacci => {
            let period = if learned.is_some() {
                biased_period(rng, canonical(pmin + (pmax - pmin) / 2), 5, pmin, pmax)
            } else {
                rand_period_step(rng, pmin, pmax, pstep)
            };
            (
                IndicatorParams {
                    period: Some(period),
                    ..Default::default()
                },
                None,
            )
        }

        // ADX: single period, single output
        IndicatorType::ADX => {
            let period = biased_period(rng, canonical(14), 5, pmin, pmax);
            (
                IndicatorParams {
                    period: Some(period),
                    ..Default::default()
                },
                None,
            )
        }

        // MACD: fast_period < slow_period + signal_period, multi-output
        IndicatorType::MACD => {
            // Biased toward MACD(12, 26, 9) standard values
            let fast = biased_period(rng, 12, 4, pmin, pmax.min(pmin + 10));
            let slow_min = (fast + 2).max(pmin);
            let slow = biased_period(rng, 26, 6, slow_min, (fast + 20).min(pmax + 10).max(slow_min));
            let signal = biased_period(rng, 9, 3, 3, 15);
            // Use canonical field names recognized by get_indicator_value:
            // None = MACD line (primary), "signal" = signal line, "histogram" = histogram
            let output_field: Option<&str> = match rng.gen_range(0..3) {
                0 => None,
                1 => Some("signal"),
                _ => Some("histogram"),
            };
            (
                IndicatorParams {
                    fast_period: Some(fast),
                    slow_period: Some(slow),
                    signal_period: Some(signal),
                    ..Default::default()
                },
                output_field.map(|s| s.to_string()),
            )
        }

        // Bollinger Bands: period + std_dev, multi-output (biased toward BB(20, 2.0))
        IndicatorType::BollingerBands => {
            let period = biased_period(rng, 20, 5, pmin, pmax);
            // Biased std_dev: 60% near 2.0 ± 0.5, 40% full [1.0, 3.5]
            let std_dev = if rng.gen_bool(0.6) {
                rand_range(rng, 1.5f64.max(1.0), 2.5f64.min(3.5))
            } else {
                rand_range(rng, 1.0, 3.5)
            };
            let output_field = match rng.gen_range(0..3) {
                0 => "upper",
                1 => "middle",
                _ => "lower",
            };
            (
                IndicatorParams {
                    period: Some(period),
                    std_dev: Some((std_dev * 10.0).round() / 10.0),
                    ..Default::default()
                },
                Some(output_field.to_string()),
            )
        }

        // Stochastic: k_period + d_period, dual output (biased toward k=14)
        IndicatorType::Stochastic => {
            let k = biased_period(rng, 14, 5, pmin.max(5), pmax.min(21));
            let d = rand_period(rng, 3, 7);
            // None = %K (primary), "d" = %D (secondary)
            let output_field: Option<&str> = match rng.gen_range(0..2) {
                0 => None,
                _ => Some("d"),
            };
            (
                IndicatorParams {
                    k_period: Some(k),
                    d_period: Some(d),
                    ..Default::default()
                },
                output_field.map(|s| s.to_string()),
            )
        }

        // Parabolic SAR: acceleration_factor + maximum_factor
        IndicatorType::ParabolicSAR => {
            let af = rand_range(rng, 0.01, 0.05);
            let mf = rand_range(rng, 0.1, 0.3);
            (
                IndicatorParams {
                    acceleration_factor: Some((af * 1000.0).round() / 1000.0),
                    maximum_factor: Some((mf * 100.0).round() / 100.0),
                    ..Default::default()
                },
                None,
            )
        }

        // VWAP: no params
        IndicatorType::VWAP => (IndicatorParams::default(), None),

        // Aroon: period + dual output
        IndicatorType::Aroon => {
            let period = rand_period_step(rng, pmin, pmax, pstep);
            // None = Aroon Up (primary), "aroon_down" = Aroon Down (secondary)
            let output_field: Option<&str> = match rng.gen_range(0..2) {
                0 => None,
                _ => Some("aroon_down"),
            };
            (
                IndicatorParams {
                    period: Some(period),
                    ..Default::default()
                },
                output_field.map(|s| s.to_string()),
            )
        }

        // Vortex: period + dual output
        IndicatorType::Vortex => {
            let period = rand_period_step(rng, pmin, pmax, pstep);
            // None = VI+ (primary), "vi_minus" = VI- (secondary)
            let output_field: Option<&str> = match rng.gen_range(0..2) {
                0 => None,
                _ => Some("vi_minus"),
            };
            (
                IndicatorParams {
                    period: Some(period),
                    ..Default::default()
                },
                output_field.map(|s| s.to_string()),
            )
        }

        // Ichimoku: tenkan/kijun/senkou_b periods with classic 1:3:6 ratios.
        // Output field must match the extra-map keys returned by ichimoku():
        // "tenkan", "kijun", "senkou_a", "senkou_b", "chikou"
        IndicatorType::Ichimoku => {
            let tenkan = rand_period_step(rng, pmin.max(7), pmax.min(26), pstep);
            let kijun = (tenkan * 3).max(tenkan + 1);
            let senkou_b = (tenkan * 6).max(kijun + 1);
            let output_field: Option<&str> = match rng.gen_range(0..5) {
                0 => Some("tenkan"),
                1 => Some("kijun"),
                2 => Some("senkou_a"),
                3 => Some("senkou_b"),
                _ => Some("chikou"),
            };
            (
                IndicatorParams {
                    fast_period: Some(tenkan),
                    slow_period: Some(kijun),
                    signal_period: Some(senkou_b),
                    ..Default::default()
                },
                output_field.map(|s| s.to_string()),
            )
        }

        // Keltner Channel: period + multiplier, triple output
        IndicatorType::KeltnerChannel => {
            let period = rand_period_step(rng, pmin, pmax, pstep);
            let multiplier = rand_range(rng, 1.0, 3.0);
            let output_field = match rng.gen_range(0..3) {
                0 => "upper",
                1 => "middle",
                _ => "lower",
            };
            (
                IndicatorParams {
                    period: Some(period),
                    multiplier: Some((multiplier * 10.0).round() / 10.0),
                    ..Default::default()
                },
                Some(output_field.to_string()),
            )
        }

        // GannHiLo: period, single output only (no secondary)
        IndicatorType::GannHiLo => {
            let period = rand_period_step(rng, pmin, pmax, pstep);
            (
                IndicatorParams {
                    period: Some(period),
                    ..Default::default()
                },
                None,
            )
        }

        // Heiken Ashi: no params. None = ha_close (primary), "ha_open" = ha_open (secondary)
        IndicatorType::HeikenAshi => {
            let output_field: Option<&str> = match rng.gen_range(0..2) {
                0 => None,
                _ => Some("ha_open"),
            };
            (
                IndicatorParams::default(),
                output_field.map(|s| s.to_string()),
            )
        }

        // SuperTrend: period + multiplier
        IndicatorType::SuperTrend => {
            let period = rand_period_step(rng, pmin.max(7), pmax.min(30), pstep);
            let multiplier = rand_range(rng, 1.5, 4.0);
            (
                IndicatorParams {
                    period: Some(period),
                    multiplier: Some((multiplier * 10.0).round() / 10.0),
                    ..Default::default()
                },
                None,
            )
        }

        // Pivots: no period param (uses all candles). Output selects from extra-map keys:
        // "pp", "r1", "r2", "r3", "s1", "s2", "s3"
        IndicatorType::Pivots => {
            let output_field: Option<&str> = match rng.gen_range(0..7) {
                0 => Some("pp"),
                1 => Some("r1"),
                2 => Some("r2"),
                3 => Some("r3"),
                4 => Some("s1"),
                5 => Some("s2"),
                _ => Some("s3"),
            };
            (
                IndicatorParams::default(),
                output_field.map(|s| s.to_string()),
            )
        }

        // Awesome Oscillator: fast_period + slow_period
        IndicatorType::AwesomeOscillator => {
            let fast = rand_period(rng, 3, 7);
            let slow = rand_period(rng, fast + 5, (fast + 30).min(50));
            (
                IndicatorParams {
                    fast_period: Some(fast),
                    slow_period: Some(slow),
                    ..Default::default()
                },
                None,
            )
        }
    }
}

/// Generate a random operand: Compound (15%), Indicator (~59%), Price (~17%), Constant (~9%).
fn random_operand(grammar: &GrammarContext, rng: &mut impl Rng, phase: f64) -> Operand {
    // Reserve 15% for compound; scale remaining 85% by existing phase weights.
    const COMPOUND_PCT: u32 = 15;
    let roll = rng.gen_range(0..100u32);
    if roll < COMPOUND_PCT {
        return random_compound_operand(grammar, rng, phase);
    }
    // Re-roll within the remaining 85%
    let (ind_w, pri_w, _con_w) = if grammar.phase_based {
        operand_weights_for_phase(phase)
    } else {
        (70, 20, 10)
    };
    let roll2 = rng.gen_range(0..100u32);
    if roll2 < ind_w {
        random_indicator_operand(grammar, rng)
    } else if roll2 < ind_w + pri_w {
        // Price field
        let pf = grammar.price_fields.choose(rng).copied().unwrap_or(PriceField::Close);
        let offset = if grammar.max_lookback > 0 && rng.gen_bool(0.2) {
            Some(rng.gen_range(1..=grammar.max_lookback))
        } else {
            None
        };
        Operand {
            operand_type: OperandType::Price,
            indicator: None,
            price_field: Some(pf),
            constant_value: None,
            time_field: None,
            candle_pattern: None,
            offset,
            compound_left: None,
            compound_op: None,
            compound_right: None,
        }
    } else {
        // Constant — biased toward oscillator-useful ranges.
        let value = if rng.gen_bool(0.6) {
            rand_range(rng, 0.0, 100.0)
        } else {
            rand_range(rng, -100.0, 100.0)
        };
        Operand {
            operand_type: OperandType::Constant,
            indicator: None,
            price_field: None,
            constant_value: Some((value * 100.0).round() / 100.0),
            time_field: None,
            candle_pattern: None,
            offset: None,
            compound_left: None,
            compound_op: None,
            compound_right: None,
        }
    }
}

// ══════════════════════════════════════════════════════════════
// Random rule/strategy generation
// ══════════════════════════════════════════════════════════════

/// Generate a single random trading rule.
fn random_rule(grammar: &GrammarContext, rng: &mut impl Rng, last: bool, phase: f64) -> Rule {
    let left = random_operand(grammar, rng, phase);
    let comparator = *grammar
        .comparators
        .choose(rng)
        .unwrap_or(&Comparator::GreaterThan);
    let right = random_operand(grammar, rng, phase);

    let and_prob = if grammar.phase_based { and_prob_for_phase(phase) } else { 70 };
    let logical_operator = if last {
        None
    } else if rng.gen_range(0..100) < and_prob {
        Some(LogicalOperator::And)
    } else {
        Some(LogicalOperator::Or)
    };

    Rule {
        id: gen_id(),
        left_operand: left,
        comparator,
        right_operand: right,
        logical_operator,
    }
}

/// Mirror a set of rules for symmetric long/short generation.
fn mirror_rules(rules: &[Rule]) -> Vec<Rule> {
    rules
        .iter()
        .map(|r| Rule {
            id: gen_id(),
            left_operand: r.left_operand.clone(),
            comparator: mirror_comparator(r.comparator),
            right_operand: r.right_operand.clone(),
            logical_operator: r.logical_operator,
        })
        .collect()
}

/// Pick a random value from a range snapped to the given step.
fn rand_range_stepped(rng: &mut impl Rng, min: f64, max: f64, step: f64) -> f64 {
    if (max - min).abs() < f64::EPSILON || step <= 0.0 {
        return min;
    }
    let steps = ((max - min) / step).floor() as usize;
    if steps == 0 {
        return min;
    }
    let pick = rng.gen_range(0..=steps);
    let raw = min + pick as f64 * step;
    // Round to avoid floating-point drift
    let precision = (1.0 / step).log10().ceil().max(0.0) as u32;
    let factor = 10f64.powi(precision as i32);
    (raw * factor).round() / factor
}

/// Generate a random stop-loss configuration.
fn random_stop_loss(grammar: &GrammarContext, rng: &mut impl Rng) -> Option<StopLoss> {
    if !grammar.sl_enabled { return None; }
    if !grammar.sl_required && !rng.gen_bool(0.65) { return None; }
    let value = rand_range_stepped(
        rng,
        grammar.sl_coeff_range.0,
        grammar.sl_coeff_range.1,
        grammar.sl_coeff_step,
    ).max(grammar.sl_coeff_step.max(f64::EPSILON));
    let (sl_type, atr_period) = match grammar.sl_type {
        BuilderSLType::Atr => (
            StopLossType::ATR,
            Some(rand_period_step(rng, grammar.sl_atr_period_range.0, grammar.sl_atr_period_range.1, grammar.sl_atr_period_step)),
        ),
        BuilderSLType::Pips => (StopLossType::Pips, None),
        BuilderSLType::Percentage => (StopLossType::Percentage, None),
    };
    Some(StopLoss {
        sl_type,
        value,
        atr_period,
    })
}

/// Generate a random take-profit configuration.
fn random_take_profit(grammar: &GrammarContext, rng: &mut impl Rng) -> Option<TakeProfit> {
    if !grammar.tp_enabled { return None; }
    if !grammar.tp_required && !rng.gen_bool(0.65) { return None; }
    let value = rand_range_stepped(
        rng,
        grammar.tp_coeff_range.0,
        grammar.tp_coeff_range.1,
        grammar.tp_coeff_step,
    ).max(grammar.tp_coeff_step.max(f64::EPSILON));
    let (tp_type, atr_period) = match grammar.tp_type {
        BuilderTPType::Atr => (
            TakeProfitType::ATR,
            Some(rand_period_step(rng, grammar.tp_atr_period_range.0, grammar.tp_atr_period_range.1, grammar.tp_atr_period_step)),
        ),
        BuilderTPType::Pips => (TakeProfitType::Pips, None),
        BuilderTPType::Rr => (TakeProfitType::RiskReward, None),
    };
    Some(TakeProfit {
        tp_type,
        value,
        atr_period,
    })
}

/// Generate a random trailing stop configuration.
fn random_trailing_stop(grammar: &GrammarContext, rng: &mut impl Rng) -> Option<TrailingStop> {
    if !grammar.ts_enabled { return None; }
    if !grammar.ts_required && !rng.gen_bool(0.35) { return None; }
    let value = rand_range_stepped(
        rng,
        grammar.sl_coeff_range.0,
        grammar.sl_coeff_range.1,
        grammar.sl_coeff_step,
    ).max(grammar.sl_coeff_step.max(f64::EPSILON));
    let atr_period = Some(rand_period_step(
        rng,
        grammar.sl_atr_period_range.0,
        grammar.sl_atr_period_range.1,
        grammar.sl_atr_period_step,
    ));
    Some(TrailingStop {
        ts_type: TrailingStopType::ATR,
        value,
        atr_period,
    })
}

/// Pick a random entry order type from the enabled list.
fn random_entry_order(grammar: &GrammarContext, rng: &mut impl Rng) -> OrderType {
    if grammar.enabled_order_types.len() <= 1 {
        return OrderType::Market;
    }
    // 60% Market, remainder split among Limit/Stop
    let non_market: Vec<OrderType> = grammar.enabled_order_types.iter()
        .copied()
        .filter(|t| *t != OrderType::Market)
        .collect();
    if rng.gen_bool(0.60) || non_market.is_empty() {
        OrderType::Market
    } else {
        non_market[rng.gen_range(0..non_market.len())]
    }
}

/// Generate a random close_after_bars value when exit_after_bars is enabled.
fn random_close_after_bars(grammar: &GrammarContext, rng: &mut impl Rng) -> Option<u32> {
    if !grammar.exit_after_bars_enabled { return None; }
    if !grammar.exit_after_bars_required && !rng.gen_bool(0.30) { return None; }
    // Random bar count: 5 to 50 in steps of 5
    let value = rand_range_stepped(rng, 5.0, 50.0, 5.0) as u32;
    Some(value.max(1))
}

/// Generate a random move_sl_to_be flag when move_sl_be is enabled.
fn random_move_sl_be(grammar: &GrammarContext, rng: &mut impl Rng) -> bool {
    if !grammar.move_sl_be_enabled { return false; }
    if grammar.move_sl_be_required { return true; }
    rng.gen_bool(0.40)
}

/// Generate a random order-price indicator config when order price indicators are enabled.
fn random_order_price_indicator(grammar: &GrammarContext, rng: &mut impl Rng) -> Option<crate::models::strategy::OrderPriceConfig> {
    // Only useful when Limit or Stop orders are enabled
    let has_pending = grammar.enabled_order_types.iter().any(|t| *t != OrderType::Market);
    if !has_pending { return None; }
    if grammar.enabled_order_price_indicators.is_empty() { return None; }
    if !rng.gen_bool(0.50) { return None; }

    let indicator_type = {
        let total: usize = grammar.enabled_order_price_indicators.iter().map(|(_, w)| w).sum();
        let mut pick = rng.gen_range(0..total.max(1));
        let mut chosen = grammar.enabled_order_price_indicators[0].0;
        for (it, w) in &grammar.enabled_order_price_indicators {
            if pick < *w { chosen = *it; break; }
            pick -= w;
        }
        chosen
    };

    let (params, output_field) = random_indicator_params(indicator_type, grammar, rng);
    let multiplier = rand_range_stepped(
        rng,
        grammar.order_price_multiplier_range.0,
        grammar.order_price_multiplier_range.1,
        grammar.order_price_multiplier_step,
    );
    Some(crate::models::strategy::OrderPriceConfig {
        indicator: IndicatorConfig {
            indicator_type,
            params,
            output_field,
            cached_hash: 0,
        },
        multiplier,
        base_price_stop: grammar.order_price_base_stop,
        base_price_limit: grammar.order_price_base_limit,
    })
}

/// Generate a complete random strategy from the grammar context.
fn generate_random_strategy(
    grammar: &GrammarContext,
    rng: &mut impl Rng,
    name: String,
    phase: f64,
) -> Strategy {
    // Leave timestamps empty during evolution — updated_at/created_at are only
    // meaningful when the strategy is actually saved to the databank.
    let now = String::new();

    // Phase-adaptive rule count: early generations use fewer rules (exploration),
    // later generations fill up to the grammar maximum (exploitation).
    let phase_max_entry = if grammar.phase_based {
        // Scale from min_entry_rules (exploration) to max_entry_rules (exploitation)
        grammar.min_entry_rules
            + ((grammar.max_entry_rules - grammar.min_entry_rules) as f64 * phase).round() as usize
    } else {
        grammar.max_entry_rules
    };
    let phase_max_entry = phase_max_entry.max(grammar.min_entry_rules);

    let n_entry = rand_period(rng, grammar.min_entry_rules, phase_max_entry);
    let n_exit = if grammar.max_exit_rules > 0 {
        rand_period(rng, grammar.min_exit_rules.max(1), grammar.max_exit_rules)
    } else {
        0
    };

    // Generate entry and exit rules based on direction
    let (long_entry, short_entry, long_exit, short_exit) = match grammar.direction {
        BuilderDirection::LongOnly => {
            let entry: Vec<Rule> = (0..n_entry)
                .map(|i| random_rule(grammar, rng, i == n_entry - 1, phase))
                .collect();
            let exit: Vec<Rule> = (0..n_exit)
                .map(|i| random_rule(grammar, rng, i == n_exit - 1, phase))
                .collect();
            (entry, vec![], exit, vec![])
        }
        BuilderDirection::ShortOnly => {
            let entry: Vec<Rule> = (0..n_entry)
                .map(|i| random_rule(grammar, rng, i == n_entry - 1, phase))
                .collect();
            let exit: Vec<Rule> = (0..n_exit)
                .map(|i| random_rule(grammar, rng, i == n_exit - 1, phase))
                .collect();
            (vec![], entry, vec![], exit)
        }
        BuilderDirection::BothSymmetric => {
            let long_e: Vec<Rule> = (0..n_entry)
                .map(|i| random_rule(grammar, rng, i == n_entry - 1, phase))
                .collect();
            let long_x: Vec<Rule> = (0..n_exit)
                .map(|i| random_rule(grammar, rng, i == n_exit - 1, phase))
                .collect();
            let short_e = mirror_rules(&long_e);
            let short_x = mirror_rules(&long_x);
            (long_e, short_e, long_x, short_x)
        }
        BuilderDirection::BothAsymmetric => {
            let long_e: Vec<Rule> = (0..n_entry)
                .map(|i| random_rule(grammar, rng, i == n_entry - 1, phase))
                .collect();
            let long_x: Vec<Rule> = (0..n_exit)
                .map(|i| random_rule(grammar, rng, i == n_exit - 1, phase))
                .collect();
            let n_entry_s = rand_period(rng, grammar.min_entry_rules, phase_max_entry);
            let n_exit_s = if grammar.max_exit_rules > 0 {
                rand_period(rng, grammar.min_exit_rules.max(1), grammar.max_exit_rules)
            } else {
                0
            };
            let short_e: Vec<Rule> = (0..n_entry_s)
                .map(|i| random_rule(grammar, rng, i == n_entry_s - 1, phase))
                .collect();
            let short_x: Vec<Rule> = (0..n_exit_s)
                .map(|i| random_rule(grammar, rng, i == n_exit_s - 1, phase))
                .collect();
            (long_e, short_e, long_x, short_x)
        }
    };

    // Wrap rules into a single group per side (groups take precedence in executor)
    let long_entry_groups = make_groups_from_rules(long_entry, rng);
    let short_entry_groups = make_groups_from_rules(short_entry, rng);
    let long_exit_groups = make_groups_from_rules(long_exit, rng);
    let short_exit_groups = make_groups_from_rules(short_exit, rng);

    Strategy {
        id: gen_id(),
        name,
        created_at: now.clone(),
        updated_at: now,
        // Flat rules stay empty — groups take precedence
        long_entry_rules: vec![],
        short_entry_rules: vec![],
        long_exit_rules: vec![],
        short_exit_rules: vec![],
        long_entry_groups,
        short_entry_groups,
        long_exit_groups,
        short_exit_groups,
        position_sizing: grammar.position_sizing.clone(),
        stop_loss: random_stop_loss(grammar, rng),
        take_profit: random_take_profit(grammar, rng),
        trailing_stop: random_trailing_stop(grammar, rng),
        trading_costs: grammar.trading_costs.clone(),
        trade_direction: grammar.trade_direction,
        trading_hours: grammar.trading_hours.clone(),
        max_daily_trades: grammar.max_daily_trades,
        close_trades_at: grammar.close_trades_at.clone(),
        entry_order: random_entry_order(grammar, rng),
        entry_order_offset_pips: rand_range_stepped(rng, 5.0, 30.0, 5.0),
        close_after_bars: random_close_after_bars(grammar, rng),
        move_sl_to_be: random_move_sl_be(grammar, rng),
        entry_order_indicator: random_order_price_indicator(grammar, rng),
    }
}

/// Wrap a Vec<Rule> into a single RuleGroup with a random internal operator.
/// Returns an empty Vec if rules is empty.
fn make_groups_from_rules(rules: Vec<Rule>, rng: &mut impl Rng) -> Vec<RuleGroup> {
    if rules.is_empty() {
        return vec![];
    }
    let internal = if rng.gen_bool(0.7) { LogicalOperator::And } else { LogicalOperator::Or };
    vec![RuleGroup {
        id: gen_id(),
        rules,
        internal,
        join: None,
    }]
}

/// Mirror a Vec<RuleGroup> for the symmetric short side.
fn mirror_groups(groups: &[RuleGroup]) -> Vec<RuleGroup> {
    groups.iter().map(|g| RuleGroup {
        id: gen_id(),
        rules: mirror_rules(&g.rules),
        internal: g.internal,
        join: g.join,
    }).collect()
}

/// Fix the join operator: last group's join must be None.
fn fix_last_group_join(groups: &mut Vec<RuleGroup>) {
    if let Some(last) = groups.last_mut() {
        last.join = None;
    }
}

/// Trim rules from the last groups (starting from the last group, then backwards)
/// until the total rule count across all groups is at most `max_rules`.
/// Preserves at least 1 rule per group and never leaves empty groups.
fn trim_groups_to_max_rules(groups: &mut Vec<RuleGroup>, max_rules: usize) {
    if max_rules == 0 {
        return;
    }
    let total: usize = groups.iter().map(|g| g.rules.len()).sum();
    if total <= max_rules {
        return;
    }
    let excess = total - max_rules;
    let mut to_remove = excess;
    // Trim from the last group backwards, preserving at least 1 rule per group
    let mut g_idx = groups.len();
    while to_remove > 0 && g_idx > 0 {
        g_idx -= 1;
        let available = groups[g_idx].rules.len().saturating_sub(1);
        let remove_here = available.min(to_remove);
        if remove_here > 0 {
            let new_len = groups[g_idx].rules.len() - remove_here;
            groups[g_idx].rules.truncate(new_len);
            to_remove -= remove_here;
        }
    }
    fix_last_group_join(groups);
}

// ══════════════════════════════════════════════════════════════
// Crossover
// ══════════════════════════════════════════════════════════════

/// Uniform crossover on two rule lists.
/// Each position is independently assigned from parent1 or parent2 with p=0.5.
/// When one parent is longer, the extra rules are randomly assigned to either child.
fn crossover_rules_uniform(
    p1: &[Rule],
    p2: &[Rule],
    rng: &mut impl Rng,
    grammar: &GrammarContext,
    min_rules: usize,
    max_rules: usize,
) -> (Vec<Rule>, Vec<Rule>) {
    let max_len = p1.len().max(p2.len());
    let mut c1: Vec<Rule> = Vec::with_capacity(max_len);
    let mut c2: Vec<Rule> = Vec::with_capacity(max_len);

    for i in 0..max_len {
        match (p1.get(i), p2.get(i)) {
            (Some(a), Some(b)) => {
                if rng.gen_bool(0.5) { c1.push(a.clone()); c2.push(b.clone()); }
                else                 { c1.push(b.clone()); c2.push(a.clone()); }
            }
            (Some(a), None) => { if rng.gen_bool(0.5) { c1.push(a.clone()); } else { c2.push(a.clone()); } }
            (None, Some(b)) => { if rng.gen_bool(0.5) { c2.push(b.clone()); } else { c1.push(b.clone()); } }
            (None, None) => break,
        }
    }

    c1.truncate(max_rules);
    c2.truncate(max_rules);
    while c1.len() < min_rules { c1.push(random_rule(grammar, rng, true, 0.5)); }
    while c2.len() < min_rules { c2.push(random_rule(grammar, rng, true, 0.5)); }

    if let Some(last) = c1.last_mut() { last.logical_operator = None; }
    if let Some(last) = c2.last_mut() { last.logical_operator = None; }
    (c1, c2)
}

/// Uniform crossover on two group lists.
/// Each group position is independently assigned from parent1 or parent2 with p=0.5.
fn crossover_groups_uniform(
    p1: &[RuleGroup],
    p2: &[RuleGroup],
    rng: &mut impl Rng,
    grammar: &GrammarContext,
    min_groups: usize,
    max_rules: usize,
) -> (Vec<RuleGroup>, Vec<RuleGroup>) {
    let max_len = p1.len().max(p2.len());
    let mut c1: Vec<RuleGroup> = Vec::with_capacity(max_len);
    let mut c2: Vec<RuleGroup> = Vec::with_capacity(max_len);

    for i in 0..max_len {
        match (p1.get(i), p2.get(i)) {
            (Some(a), Some(b)) => {
                if rng.gen_bool(0.5) { c1.push(a.clone()); c2.push(b.clone()); }
                else                 { c1.push(b.clone()); c2.push(a.clone()); }
            }
            (Some(a), None) => { if rng.gen_bool(0.5) { c1.push(a.clone()); } else { c2.push(a.clone()); } }
            (None, Some(b)) => { if rng.gen_bool(0.5) { c2.push(b.clone()); } else { c1.push(b.clone()); } }
            (None, None) => break,
        }
    }

    // Pad if below minimum (shouldn't normally happen but be safe)
    let _ = grammar;
    while c1.len() < min_groups {
        c1.push(RuleGroup { id: gen_id(), rules: vec![random_rule(grammar, rng, true, 0.5)], internal: LogicalOperator::And, join: None });
    }
    while c2.len() < min_groups {
        c2.push(RuleGroup { id: gen_id(), rules: vec![random_rule(grammar, rng, true, 0.5)], internal: LogicalOperator::And, join: None });
    }

    trim_groups_to_max_rules(&mut c1, max_rules);
    trim_groups_to_max_rules(&mut c2, max_rules);
    fix_last_group_join(&mut c1);
    fix_last_group_join(&mut c2);
    (c1, c2)
}

/// Uniform crossover on entry/exit rules between two parent strategies.
fn crossover_strategies(
    parent1: &Strategy,
    parent2: &Strategy,
    grammar: &GrammarContext,
    rng: &mut impl Rng,
) -> (Strategy, Strategy) {
    let mut child1 = parent1.clone();
    let mut child2 = parent2.clone();

    child1.id = gen_id();
    child2.id = gen_id();
    // Do NOT call now_iso() here — created_at/updated_at are set in individual_to_saved()
    // once the strategy reaches the databank. Skipping now_iso() here eliminates
    // ~2× chrono::Utc::now() + String allocs per crossover (~tens of thousands per gen).
    child1.created_at = String::new();
    child1.updated_at = String::new();
    child2.created_at = String::new();
    child2.updated_at = String::new();

    // Crossover groups (groups take precedence when non-empty)
    if !parent1.long_entry_groups.is_empty() && !parent2.long_entry_groups.is_empty() {
        let (c1g, c2g) = crossover_groups_uniform(&parent1.long_entry_groups, &parent2.long_entry_groups, rng, grammar, 1, grammar.max_entry_rules);
        child1.long_entry_groups = c1g; child2.long_entry_groups = c2g;
    } else if !parent1.long_entry_rules.is_empty() && !parent2.long_entry_rules.is_empty() {
        let (c1r, c2r) = crossover_rules_uniform(&parent1.long_entry_rules, &parent2.long_entry_rules, rng, grammar, grammar.min_entry_rules, grammar.max_entry_rules);
        child1.long_entry_rules = c1r; child2.long_entry_rules = c2r;
    }

    if !parent1.long_exit_groups.is_empty() && !parent2.long_exit_groups.is_empty() {
        let (c1g, c2g) = crossover_groups_uniform(&parent1.long_exit_groups, &parent2.long_exit_groups, rng, grammar, 0, grammar.max_exit_rules);
        child1.long_exit_groups = c1g; child2.long_exit_groups = c2g;
    } else if grammar.max_exit_rules > 0 && !parent1.long_exit_rules.is_empty() && !parent2.long_exit_rules.is_empty() {
        let (c1r, c2r) = crossover_rules_uniform(&parent1.long_exit_rules, &parent2.long_exit_rules, rng, grammar, grammar.min_exit_rules, grammar.max_exit_rules);
        child1.long_exit_rules = c1r; child2.long_exit_rules = c2r;
    }

    if grammar.direction == BuilderDirection::BothAsymmetric {
        if !parent1.short_entry_groups.is_empty() && !parent2.short_entry_groups.is_empty() {
            let (c1g, c2g) = crossover_groups_uniform(&parent1.short_entry_groups, &parent2.short_entry_groups, rng, grammar, 1, grammar.max_entry_rules);
            child1.short_entry_groups = c1g; child2.short_entry_groups = c2g;
        } else if !parent1.short_entry_rules.is_empty() && !parent2.short_entry_rules.is_empty() {
            let (c1r, c2r) = crossover_rules_uniform(&parent1.short_entry_rules, &parent2.short_entry_rules, rng, grammar, grammar.min_entry_rules, grammar.max_entry_rules);
            child1.short_entry_rules = c1r; child2.short_entry_rules = c2r;
        }
        if !parent1.short_exit_groups.is_empty() && !parent2.short_exit_groups.is_empty() {
            let (c1g, c2g) = crossover_groups_uniform(&parent1.short_exit_groups, &parent2.short_exit_groups, rng, grammar, 0, grammar.max_exit_rules);
            child1.short_exit_groups = c1g; child2.short_exit_groups = c2g;
        } else if grammar.max_exit_rules > 0 && !parent1.short_exit_rules.is_empty() && !parent2.short_exit_rules.is_empty() {
            let (c1r, c2r) = crossover_rules_uniform(&parent1.short_exit_rules, &parent2.short_exit_rules, rng, grammar, grammar.min_exit_rules, grammar.max_exit_rules);
            child1.short_exit_rules = c1r; child2.short_exit_rules = c2r;
        }
    } else if grammar.direction == BuilderDirection::BothSymmetric {
        child1.short_entry_rules = mirror_rules(&child1.long_entry_rules);
        child2.short_entry_rules = mirror_rules(&child2.long_entry_rules);
        child1.short_exit_rules = mirror_rules(&child1.long_exit_rules);
        child2.short_exit_rules = mirror_rules(&child2.long_exit_rules);
        child1.short_entry_groups = mirror_groups(&child1.long_entry_groups);
        child2.short_entry_groups = mirror_groups(&child2.long_entry_groups);
        child1.short_exit_groups = mirror_groups(&child1.long_exit_groups);
        child2.short_exit_groups = mirror_groups(&child2.long_exit_groups);
    }

    // Crossover SL/TP coefficients
    if rng.gen_bool(0.5) { std::mem::swap(&mut child1.stop_loss, &mut child2.stop_loss); }
    if rng.gen_bool(0.5) { std::mem::swap(&mut child1.take_profit, &mut child2.take_profit); }
    if rng.gen_bool(0.5) { std::mem::swap(&mut child1.entry_order, &mut child2.entry_order); }
    if rng.gen_bool(0.5) { std::mem::swap(&mut child1.close_after_bars, &mut child2.close_after_bars); }
    if rng.gen_bool(0.5) { std::mem::swap(&mut child1.move_sl_to_be, &mut child2.move_sl_to_be); }

    (child1, child2)
}

// ══════════════════════════════════════════════════════════════
// Mutation
// ══════════════════════════════════════════════════════════════

/// Apply a random mutation to a strategy.
/// Compute phase-adaptive mutation weights.
///
/// Maps generation progress [0.0, 1.0] to weights for each of the 12 mutation types.
/// Early generations favour structural mutations (exploration); late generations
/// favour parametric mutations (exploitation).
///
/// Index mapping:
/// 0=indicator_type  1=indicator_params  2=comparator   3=add_rule    4=remove_rule
/// 5=logical_op      6=sl_tp_coeff       7=offset        8=add_group   9=remove_group
/// 10=flip_internal  11=flip_join
fn mutation_weights_for_phase(phase: f64) -> [u32; 12] {
    // Both rows sum to 100. Lerp between them; result also sums to 100.
    const EARLY: [f64; 12] = [28.0, 10.0, 13.0, 10.0, 10.0, 8.0,  3.0, 3.0, 6.0, 4.0, 3.0, 2.0];
    const LATE:  [f64; 12] = [12.0, 36.0,  6.0,  5.0,  5.0, 4.0, 12.0, 9.0, 3.0, 2.0, 3.0, 3.0];
    let mut weights = [0u32; 12];
    let mut sum = 0u32;
    for i in 0..12 {
        let w = (EARLY[i] + phase * (LATE[i] - EARLY[i])).round() as u32;
        weights[i] = w;
        sum += w;
    }
    // Absorb rounding drift (max ±6) into the last weight so total stays at 100.
    if sum > 100 {
        weights[11] = weights[11].saturating_sub(sum - 100);
    } else if sum < 100 {
        weights[11] += 100 - sum;
    }
    weights
}

fn mutate_strategy(strategy: &mut Strategy, grammar: &GrammarContext, rng: &mut impl Rng, phase: f64) {
    let weights = if grammar.phase_based {
        mutation_weights_for_phase(phase)
    } else {
        // Fixed baseline weights (same distribution as before).
        [22, 30, 9, 7, 7, 5, 6, 2, 4, 3, 3, 2]
    };
    const TOTAL_WEIGHT: u32 = 100;
    let pick = rng.gen_range(0..TOTAL_WEIGHT);
    let mut acc = 0u32;
    let mutation_type = weights
        .iter()
        .enumerate()
        .find(|(_, &w)| { acc += w; pick < acc })
        .map(|(i, _)| i)
        .unwrap_or(0);
    match mutation_type {
        0 => mutate_indicator_type(strategy, grammar, rng),
        1 => mutate_indicator_params(strategy, grammar, rng),
        2 => mutate_comparator(strategy, grammar, rng),
        3 => mutate_add_rule(strategy, grammar, rng),
        4 => mutate_remove_rule(strategy, grammar, rng),
        5 => mutate_logical_operator(strategy, rng),
        6 => mutate_sl_tp_coeff(strategy, grammar, rng),
        7 => mutate_offset(strategy, grammar, rng),
        8 => mutate_add_group(strategy, grammar, rng),
        9 => mutate_remove_group(strategy, grammar, rng),
        10 => mutate_flip_internal_op(strategy, rng),
        11 => mutate_flip_join_op(strategy, rng),
        _ => {}
    }

    // Do NOT call now_iso() here — updated_at is set in individual_to_saved() once
    // the strategy reaches the databank. Avoiding the syscall+alloc in every mutation
    // saves ~200 K chrono::Utc::now() calls over a 500-generation run.

    // If symmetric, re-mirror short side
    if grammar.direction == BuilderDirection::BothSymmetric {
        strategy.short_entry_rules = mirror_rules(&strategy.long_entry_rules);
        strategy.short_exit_rules = mirror_rules(&strategy.long_exit_rules);
        strategy.short_entry_groups = mirror_groups(&strategy.long_entry_groups);
        strategy.short_exit_groups = mirror_groups(&strategy.long_exit_groups);
    }
}

/// Pick a random rule from the strategy — searches groups first, then flat rules.
fn pick_random_rule_mut<'a>(
    strategy: &'a mut Strategy,
    rng: &mut impl Rng,
) -> Option<&'a mut Rule> {
    // Count rules in groups
    let groups_total: usize =
        strategy.long_entry_groups.iter().map(|g| g.rules.len()).sum::<usize>()
        + strategy.short_entry_groups.iter().map(|g| g.rules.len()).sum::<usize>()
        + strategy.long_exit_groups.iter().map(|g| g.rules.len()).sum::<usize>()
        + strategy.short_exit_groups.iter().map(|g| g.rules.len()).sum::<usize>();

    // Count rules in flat lists
    let flat_total: usize =
        strategy.long_entry_rules.len()
        + strategy.short_entry_rules.len()
        + strategy.long_exit_rules.len()
        + strategy.short_exit_rules.len();

    let total = groups_total + flat_total;
    if total == 0 {
        return None;
    }

    let pick = rng.gen_range(0..total);

    if pick < groups_total {
        // Find rule in groups using two-pass (immutable count + mutable index)
        let mut acc = 0usize;
        let mut found: Option<(usize, usize, usize)> = None; // (set_idx, group_idx, rule_idx)

        'outer: for set_idx in 0..4usize {
            let groups: &[RuleGroup] = match set_idx {
                0 => &strategy.long_entry_groups,
                1 => &strategy.short_entry_groups,
                2 => &strategy.long_exit_groups,
                _ => &strategy.short_exit_groups,
            };
            for (g_idx, group) in groups.iter().enumerate() {
                for r_idx in 0..group.rules.len() {
                    if acc == pick {
                        found = Some((set_idx, g_idx, r_idx));
                        break 'outer;
                    }
                    acc += 1;
                }
            }
        }

        if let Some((set_idx, g_idx, r_idx)) = found {
            let groups_mut: &mut Vec<RuleGroup> = match set_idx {
                0 => &mut strategy.long_entry_groups,
                1 => &mut strategy.short_entry_groups,
                2 => &mut strategy.long_exit_groups,
                _ => &mut strategy.short_exit_groups,
            };
            return groups_mut[g_idx].rules.get_mut(r_idx);
        }
        return None;
    }

    // Fall through to flat rules
    let flat_pick = pick - groups_total;
    let counts = [
        strategy.long_entry_rules.len(),
        strategy.short_entry_rules.len(),
        strategy.long_exit_rules.len(),
        strategy.short_exit_rules.len(),
    ];
    let mut offset = 0;
    for i in 0..4 {
        if flat_pick < offset + counts[i] {
            return match i {
                0 => strategy.long_entry_rules.get_mut(flat_pick - offset),
                1 => strategy.short_entry_rules.get_mut(flat_pick - offset),
                2 => strategy.long_exit_rules.get_mut(flat_pick - offset),
                _ => strategy.short_exit_rules.get_mut(flat_pick - offset),
            };
        }
        offset += counts[i];
    }
    None
}

fn mutate_indicator_type_in_operand(operand: &mut Operand, grammar: &GrammarContext, rng: &mut impl Rng) {
    match operand.operand_type {
        OperandType::Indicator => {
            *operand = random_indicator_operand(grammar, rng);
        }
        OperandType::Compound => {
            let go_left = match (&operand.compound_left, &operand.compound_right) {
                (Some(_), Some(_)) => rng.gen_bool(0.5),
                (Some(_), None)    => true,
                _                  => false,
            };
            if go_left {
                if let Some(ref mut left) = operand.compound_left {
                    mutate_indicator_type_in_operand(left, grammar, rng);
                }
            } else if let Some(ref mut right) = operand.compound_right {
                mutate_indicator_type_in_operand(right, grammar, rng);
            }
        }
        _ => {}
    }
}

fn mutate_indicator_type(strategy: &mut Strategy, grammar: &GrammarContext, rng: &mut impl Rng) {
    if let Some(rule) = pick_random_rule_mut(strategy, rng) {
        let operand = if rng.gen_bool(0.5) {
            &mut rule.left_operand
        } else {
            &mut rule.right_operand
        };
        mutate_indicator_type_in_operand(operand, grammar, rng);
    }
}

fn mutate_indicator_params_in_operand(operand: &mut Operand, grammar: &GrammarContext, rng: &mut impl Rng) {
    match operand.operand_type {
        OperandType::Indicator => {
            if let Some(ref mut ind) = operand.indicator {
                // Use per-indicator override if available, otherwise global range.
                let (pmin, pmax, pstep) = grammar.indicator_period_overrides
                    .get(&ind.indicator_type)
                    .copied()
                    .unwrap_or((grammar.period_min, grammar.period_max, grammar.period_step));
                if let Some(ref mut p) = ind.params.period {
                    let steps: i32 = rng.gen_range(-3..=3);
                    let delta = steps * pstep as i32;
                    let raw = (*p as i32 + delta).max(pmin as i32) as usize;
                    *p = snap_to_step_usize(raw.min(pmax), pmin, pmax, pstep);
                }
                if let Some(ref mut fp) = ind.params.fast_period {
                    let delta: i32 = rng.gen_range(-2..=2);
                    *fp = ((*fp as i32 + delta).max(2)) as usize;
                }
                if let Some(ref mut sp) = ind.params.slow_period {
                    let delta: i32 = rng.gen_range(-2..=2);
                    *sp = ((*sp as i32 + delta).max(5)) as usize;
                }
                // Enforce fast_period < slow_period (required by MACD/Ichimoku semantics).
                let fp_copy = ind.params.fast_period;
                if let Some(ref mut sp) = ind.params.slow_period {
                    if let Some(fp) = fp_copy {
                        if fp >= *sp {
                            *sp = (fp + 2).min(pmax);
                        }
                    }
                    *sp = (*sp).clamp(pmin, pmax);
                }
            }
        }
        OperandType::Compound => {
            let go_left = match (&operand.compound_left, &operand.compound_right) {
                (Some(_), Some(_)) => rng.gen_bool(0.5),
                (Some(_), None)    => true,
                _                  => false,
            };
            if go_left {
                if let Some(ref mut left) = operand.compound_left {
                    mutate_indicator_params_in_operand(left, grammar, rng);
                }
            } else if let Some(ref mut right) = operand.compound_right {
                mutate_indicator_params_in_operand(right, grammar, rng);
            }
        }
        _ => {}
    }
}

fn mutate_indicator_params(strategy: &mut Strategy, grammar: &GrammarContext, rng: &mut impl Rng) {
    if let Some(rule) = pick_random_rule_mut(strategy, rng) {
        let operand = if rng.gen_bool(0.5) {
            &mut rule.left_operand
        } else {
            &mut rule.right_operand
        };
        mutate_indicator_params_in_operand(operand, grammar, rng);
    }
}

fn mutate_comparator(strategy: &mut Strategy, grammar: &GrammarContext, rng: &mut impl Rng) {
    if let Some(rule) = pick_random_rule_mut(strategy, rng) {
        if let Some(new_cmp) = grammar.comparators.choose(rng) {
            rule.comparator = *new_cmp;
        }
    }
}

fn mutate_add_rule(strategy: &mut Strategy, grammar: &GrammarContext, rng: &mut impl Rng) {
    // Prefer adding to groups; fall back to flat rules if groups are empty.
    let choice = rng.gen_range(0..4u32);
    let (max_rules, is_entry) = match choice {
        0 | 1 => (grammar.max_entry_rules, true),
        _ => (grammar.max_exit_rules, false),
    };
    if max_rules == 0 { return; }

    let groups = match choice {
        0 => &mut strategy.long_entry_groups,
        1 => &mut strategy.short_entry_groups,
        2 => &mut strategy.long_exit_groups,
        _ => &mut strategy.short_exit_groups,
    };

    if !groups.is_empty() {
        // Check total rules across ALL groups in this section before adding
        let total_rules: usize = groups.iter().map(|g| g.rules.len()).sum();
        if total_rules < max_rules {
            let g_idx = rng.gen_range(0..groups.len());
            let g = &mut groups[g_idx];
            // Set logical_operator on current last rule before it becomes second-to-last
            if let Some(last) = g.rules.last_mut() {
                if last.logical_operator.is_none() {
                    last.logical_operator = Some(LogicalOperator::And);
                }
            }
            g.rules.push(random_rule(grammar, rng, is_entry, 0.5));
            // Ensure new last rule has no logical_operator
            if let Some(last) = g.rules.last_mut() {
                last.logical_operator = None;
            }
        }
    } else {
        // Fall back: add to flat rules
        let (rules, flat_max) = match choice {
            0 => (&mut strategy.long_entry_rules, grammar.max_entry_rules),
            1 => (&mut strategy.short_entry_rules, grammar.max_entry_rules),
            2 => (&mut strategy.long_exit_rules, grammar.max_exit_rules),
            _ => (&mut strategy.short_exit_rules, grammar.max_exit_rules),
        };
        if flat_max == 0 || rules.len() >= flat_max { return; }
        if let Some(last) = rules.last_mut() {
            if last.logical_operator.is_none() {
                last.logical_operator = Some(LogicalOperator::And);
            }
        }
        rules.push(random_rule(grammar, rng, is_entry, 0.5));
        if let Some(last) = rules.last_mut() {
            last.logical_operator = None;
        }
    }
}

fn mutate_remove_rule(strategy: &mut Strategy, grammar: &GrammarContext, rng: &mut impl Rng) {
    let choice = rng.gen_range(0..4u32);
    let (min_rules, is_entry) = match choice {
        0 | 1 => (grammar.min_entry_rules, true),
        _ => (grammar.min_exit_rules, false),
    };
    let _ = is_entry;

    let groups = match choice {
        0 => &mut strategy.long_entry_groups,
        1 => &mut strategy.short_entry_groups,
        2 => &mut strategy.long_exit_groups,
        _ => &mut strategy.short_exit_groups,
    };

    if !groups.is_empty() {
        // Pick a random group that has more than min_rules rules
        let candidates: Vec<usize> = groups.iter().enumerate()
            .filter(|(_, g)| g.rules.len() > min_rules.max(1))
            .map(|(i, _)| i)
            .collect();
        if let Some(&g_idx) = candidates.choose(rng) {
            let g = &mut groups[g_idx];
            if g.rules.len() > 1 {
                let r_idx = rng.gen_range(0..g.rules.len());
                g.rules.remove(r_idx);
            }
        }
    } else {
        // Fall back: remove from flat rules
        let (rules, flat_min) = match choice {
            0 => (&mut strategy.long_entry_rules, grammar.min_entry_rules),
            1 => (&mut strategy.short_entry_rules, grammar.min_entry_rules),
            2 => (&mut strategy.long_exit_rules, grammar.min_exit_rules),
            _ => (&mut strategy.short_exit_rules, grammar.min_exit_rules),
        };
        if rules.len() <= flat_min || rules.is_empty() { return; }
        let idx = rng.gen_range(0..rules.len());
        rules.remove(idx);
        if let Some(last) = rules.last_mut() {
            last.logical_operator = None;
        }
    }
}

/// Add a new group to a random rule set by splitting an existing group.
fn mutate_add_group(strategy: &mut Strategy, grammar: &GrammarContext, rng: &mut impl Rng) {
    let choice = rng.gen_range(0..4u32);

    // If the chosen slot has flat rules but no groups, wrap the flat rules in a single group
    // before proceeding. The executor evaluates groups OR flat rules (never both), so mixing
    // them would silently ignore the flat rules.
    {
        let (flat, groups) = match choice {
            0 => (&mut strategy.long_entry_rules,  &mut strategy.long_entry_groups),
            1 => (&mut strategy.short_entry_rules, &mut strategy.short_entry_groups),
            2 => (&mut strategy.long_exit_rules,   &mut strategy.long_exit_groups),
            _ => (&mut strategy.short_exit_rules,  &mut strategy.short_exit_groups),
        };
        if !flat.is_empty() && groups.is_empty() {
            let mut rules = std::mem::take(flat);
            if let Some(last) = rules.last_mut() { last.logical_operator = None; }
            groups.push(RuleGroup {
                id: gen_id(),
                rules,
                internal: LogicalOperator::And,
                join: None,
            });
        }
    }

    let groups = match choice {
        0 => &mut strategy.long_entry_groups,
        1 => &mut strategy.short_entry_groups,
        2 => &mut strategy.long_exit_groups,
        _ => &mut strategy.short_exit_groups,
    };
    // Need at least one group with 2+ rules to split
    let splittable: Vec<usize> = groups.iter().enumerate()
        .filter(|(_, g)| g.rules.len() >= 2)
        .map(|(i, _)| i)
        .collect();
    if let Some(&g_idx) = splittable.choose(rng) {
        let split_at = rng.gen_range(1..groups[g_idx].rules.len());
        let tail_rules = groups[g_idx].rules.split_off(split_at);
        // Set join on the existing (now first) group
        let join = if rng.gen_bool(0.5) { LogicalOperator::And } else { LogicalOperator::Or };
        groups[g_idx].join = Some(join);
        let internal = if rng.gen_bool(0.7) { LogicalOperator::And } else { LogicalOperator::Or };
        let new_group = RuleGroup {
            id: gen_id(),
            rules: tail_rules,
            internal,
            join: None,
        };
        groups.insert(g_idx + 1, new_group);
    } else {
        // No splittable group: create a fresh one-rule group instead
        let (min_rules, max_rules) = match choice {
            0 | 1 => (grammar.min_entry_rules, grammar.max_entry_rules),
            _ => (grammar.min_exit_rules, grammar.max_exit_rules),
        };
        if max_rules == 0 { return; }
        // Check total rule count before adding new group
        let total_rules: usize = groups.iter().map(|g| g.rules.len()).sum();
        if total_rules >= max_rules { return; }
        let n = min_rules.max(1).min(max_rules - total_rules);
        let is_entry = choice < 2;
        let mut new_rules: Vec<Rule> = (0..n).map(|_| random_rule(grammar, rng, is_entry, 0.5)).collect();
        if let Some(last) = new_rules.last_mut() { last.logical_operator = None; }
        let internal = if rng.gen_bool(0.7) { LogicalOperator::And } else { LogicalOperator::Or };
        if let Some(last) = groups.last_mut() {
            last.join = Some(if rng.gen_bool(0.5) { LogicalOperator::And } else { LogicalOperator::Or });
        }
        groups.push(RuleGroup { id: gen_id(), rules: new_rules, internal, join: None });
    }
}

/// Remove a random group from a rule set (only if there are 2+ groups).
fn mutate_remove_group(strategy: &mut Strategy, grammar: &GrammarContext, rng: &mut impl Rng) {
    let _ = grammar;
    let choice = rng.gen_range(0..4u32);
    let groups = match choice {
        0 => &mut strategy.long_entry_groups,
        1 => &mut strategy.short_entry_groups,
        2 => &mut strategy.long_exit_groups,
        _ => &mut strategy.short_exit_groups,
    };
    if groups.len() < 2 { return; }
    let idx = rng.gen_range(0..groups.len());
    groups.remove(idx);
    fix_last_group_join(groups);
}

/// Flip the internal operator of a random group (AND ↔ OR).
fn mutate_flip_internal_op(strategy: &mut Strategy, rng: &mut impl Rng) {
    let all_groups: Vec<usize> = (0..4usize).filter(|&s| {
        let g: &[RuleGroup] = match s {
            0 => &strategy.long_entry_groups,
            1 => &strategy.short_entry_groups,
            2 => &strategy.long_exit_groups,
            _ => &strategy.short_exit_groups,
        };
        !g.is_empty()
    }).collect();
    if let Some(&set_idx) = all_groups.choose(rng) {
        let groups: &mut Vec<RuleGroup> = match set_idx {
            0 => &mut strategy.long_entry_groups,
            1 => &mut strategy.short_entry_groups,
            2 => &mut strategy.long_exit_groups,
            _ => &mut strategy.short_exit_groups,
        };
        let g_idx = rng.gen_range(0..groups.len());
        groups[g_idx].internal = match groups[g_idx].internal {
            LogicalOperator::And => LogicalOperator::Or,
            LogicalOperator::Or => LogicalOperator::And,
        };
    }
}

/// Flip the join operator between two adjacent groups.
fn mutate_flip_join_op(strategy: &mut Strategy, rng: &mut impl Rng) {
    let multi_groups: Vec<usize> = (0..4usize).filter(|&s| {
        let g: &[RuleGroup] = match s {
            0 => &strategy.long_entry_groups,
            1 => &strategy.short_entry_groups,
            2 => &strategy.long_exit_groups,
            _ => &strategy.short_exit_groups,
        };
        g.len() >= 2
    }).collect();
    if let Some(&set_idx) = multi_groups.choose(rng) {
        let groups: &mut Vec<RuleGroup> = match set_idx {
            0 => &mut strategy.long_entry_groups,
            1 => &mut strategy.short_entry_groups,
            2 => &mut strategy.long_exit_groups,
            _ => &mut strategy.short_exit_groups,
        };
        // Pick a non-last group to flip join
        let idx = rng.gen_range(0..groups.len() - 1);
        groups[idx].join = match groups[idx].join {
            Some(LogicalOperator::And) | None => Some(LogicalOperator::Or),
            Some(LogicalOperator::Or) => Some(LogicalOperator::And),
        };
    }
}

fn mutate_logical_operator(strategy: &mut Strategy, rng: &mut impl Rng) {
    if let Some(rule) = pick_random_rule_mut(strategy, rng) {
        if rule.logical_operator.is_some() {
            rule.logical_operator = if rng.gen_bool(0.5) {
                Some(LogicalOperator::And)
            } else {
                Some(LogicalOperator::Or)
            };
        }
    }
}

fn mutate_sl_tp_coeff(strategy: &mut Strategy, grammar: &GrammarContext, rng: &mut impl Rng) {
    if rng.gen_bool(0.5) {
        if let Some(ref mut sl) = strategy.stop_loss {
            let steps: i32 = rng.gen_range(-3..=3);
            let delta = steps as f64 * grammar.sl_coeff_step;
            sl.value = snap_to_step_f64(
                sl.value + delta,
                grammar.sl_coeff_range.0,
                grammar.sl_coeff_range.1,
                grammar.sl_coeff_step,
            );
        }
    } else if let Some(ref mut tp) = strategy.take_profit {
        let steps: i32 = rng.gen_range(-3..=3);
        let delta = steps as f64 * grammar.tp_coeff_step;
        tp.value = snap_to_step_f64(
            tp.value + delta,
            grammar.tp_coeff_range.0,
            grammar.tp_coeff_range.1,
            grammar.tp_coeff_step,
        );
    }
}

fn mutate_offset_in_operand(operand: &mut Operand, grammar: &GrammarContext, rng: &mut impl Rng) {
    match operand.operand_type {
        OperandType::Indicator | OperandType::Price => {
            if grammar.max_lookback > 0 && rng.gen_bool(0.6) {
                operand.offset = Some(rng.gen_range(1..=grammar.max_lookback));
            } else {
                operand.offset = None;
            }
        }
        OperandType::Compound => {
            let go_left = match (&operand.compound_left, &operand.compound_right) {
                (Some(_), Some(_)) => rng.gen_bool(0.5),
                (Some(_), None)    => true,
                _                  => false,
            };
            if go_left {
                if let Some(ref mut left) = operand.compound_left {
                    mutate_offset_in_operand(left, grammar, rng);
                }
            } else if let Some(ref mut right) = operand.compound_right {
                mutate_offset_in_operand(right, grammar, rng);
            }
        }
        _ => {}
    }
}

fn mutate_offset(strategy: &mut Strategy, grammar: &GrammarContext, rng: &mut impl Rng) {
    if let Some(rule) = pick_random_rule_mut(strategy, rng) {
        let operand = if rng.gen_bool(0.5) {
            &mut rule.left_operand
        } else {
            &mut rule.right_operand
        };
        mutate_offset_in_operand(operand, grammar, rng);
    }
}

// ══════════════════════════════════════════════════════════════
// Fitness computation
// ══════════════════════════════════════════════════════════════

/// Compute fitness score from backtest metrics based on the ranking configuration.
fn compute_fitness(metrics: &BacktestMetrics, ranking: &BuilderRanking) -> f64 {
    match ranking.compute_from {
        BuilderComputeFrom::NetProfit => metrics.net_profit,
        BuilderComputeFrom::ReturnDd => {
            if metrics.max_drawdown_pct.abs() < f64::EPSILON {
                if metrics.net_profit > 0.0 {
                    1000.0
                } else {
                    0.0
                }
            } else {
                metrics.total_return_pct / metrics.max_drawdown_pct.abs()
            }
        }
        BuilderComputeFrom::RExpectancy => {
            // R-Expectancy = expectancy / avg_loss (if avg_loss > 0)
            if metrics.avg_loss.abs() < f64::EPSILON {
                metrics.expectancy
            } else {
                metrics.expectancy / metrics.avg_loss.abs()
            }
        }
        BuilderComputeFrom::AnnualMaxDd => {
            if metrics.max_drawdown_pct.abs() < f64::EPSILON {
                if metrics.annualized_return_pct > 0.0 {
                    1000.0
                } else {
                    0.0
                }
            } else {
                metrics.annualized_return_pct / metrics.max_drawdown_pct.abs()
            }
        }
        BuilderComputeFrom::WeightedFitness => {
            compute_weighted_fitness(metrics, &ranking.weighted_criteria)
        }
    }
}

/// Compute weighted fitness from multiple criteria.
fn compute_weighted_fitness(
    metrics: &BacktestMetrics,
    criteria: &[BuilderWeightedCriterion],
) -> f64 {
    if criteria.is_empty() {
        return metrics.net_profit;
    }

    let mut total_weight = 0.0;
    let mut weighted_sum = 0.0;

    for c in criteria {
        let raw = extract_metric_by_name(metrics, &c.criterium);
        let target = c.target;

        // Normalize: how close is the metric to the target?
        let score = match c.criterion_type {
            BuilderCriterionType::Maximize => {
                if target.abs() < f64::EPSILON {
                    raw
                } else {
                    (raw / target).min(2.0) // cap at 2x target
                }
            }
            BuilderCriterionType::Minimize => {
                if raw.abs() < f64::EPSILON {
                    2.0 // perfect minimization
                } else {
                    (target / raw).min(2.0)
                }
            }
        };

        weighted_sum += score * c.weight;
        total_weight += c.weight;
    }

    if total_weight > 0.0 {
        weighted_sum / total_weight
    } else {
        0.0
    }
}

/// Extract a metric value by its string name from BacktestMetrics.
fn extract_metric_by_name(metrics: &BacktestMetrics, name: &str) -> f64 {
    match name {
        "net_profit" | "Net Profit" | "netProfit" => metrics.net_profit,
        "total_return_pct" | "Total Return %" | "totalReturnPct" => metrics.total_return_pct,
        "annualized_return_pct" | "Annual Return %" | "annualizedReturnPct" => {
            metrics.annualized_return_pct
        }
        "sharpe_ratio" | "Sharpe Ratio" | "sharpeRatio" => metrics.sharpe_ratio,
        "sortino_ratio" | "Sortino Ratio" | "sortinoRatio" => metrics.sortino_ratio,
        "calmar_ratio" | "Calmar Ratio" | "calmarRatio" => metrics.calmar_ratio,
        "profit_factor" | "Profit Factor" | "profitFactor" => metrics.profit_factor,
        "max_drawdown_pct" | "Max Drawdown %" | "maxDrawdownPct" => metrics.max_drawdown_pct,
        "win_rate_pct" | "Win Rate %" | "winRatePct" => metrics.win_rate_pct,
        "total_trades" | "Total Trades" | "totalTrades" => metrics.total_trades as f64,
        "avg_trade" | "Avg Trade" | "avgTrade" => metrics.avg_trade,
        "avg_win" | "Avg Win" | "avgWin" => metrics.avg_win,
        "avg_loss" | "Avg Loss" | "avgLoss" => metrics.avg_loss,
        "largest_win" | "Largest Win" | "largestWin" => metrics.largest_win,
        "largest_loss" | "Largest Loss" | "largestLoss" => metrics.largest_loss,
        "expectancy" | "Expectancy" => metrics.expectancy,
        "recovery_factor" | "Recovery Factor" | "recoveryFactor" => metrics.recovery_factor,
        "max_consecutive_wins" | "Max Consec. Wins" | "maxConsecutiveWins" => {
            metrics.max_consecutive_wins as f64
        }
        "max_consecutive_losses" | "Max Consec. Losses" | "maxConsecutiveLosses" => {
            metrics.max_consecutive_losses as f64
        }
        "avg_bars_in_trade" | "Avg Bars" | "avgBarsInTrade" => metrics.avg_bars_in_trade,
        "gross_profit" | "Gross Profit" | "grossProfit" => metrics.gross_profit,
        "gross_loss" | "Gross Loss" | "grossLoss" => metrics.gross_loss,
        "return_dd_ratio" | "Return/DD Ratio" | "returnDdRatio" => metrics.return_dd_ratio,
        "k_ratio" | "K-Ratio" | "kRatio" => metrics.k_ratio,
        "omega_ratio" | "Omega Ratio" | "omegaRatio" => metrics.omega_ratio,
        "stagnation_bars" | "Stagnation Bars" | "stagnationBars" => {
            metrics.stagnation_bars as f64
        }
        "ulcer_index_pct" | "Ulcer Index" | "ulcerIndexPct" => metrics.ulcer_index_pct,
        "final_capital" | "Final Capital" | "finalCapital" => metrics.final_capital,
        "monthly_return_avg_pct" | "Monthly Avg Return" | "monthlyReturnAvgPct" => {
            metrics.monthly_return_avg_pct
        }
        "avg_drawdown_pct" | "Avg Drawdown %" | "avgDrawdownPct" => metrics.avg_drawdown_pct,
        "mae_avg" | "MAE Avg" | "maeAvg" => metrics.mae_avg,
        "mae_max" | "MAE Max" | "maeMax" => metrics.mae_max,
        "mfe_avg" | "MFE Avg" | "mfeAvg" => metrics.mfe_avg,
        "mfe_max" | "MFE Max" | "mfeMax" => metrics.mfe_max,
        _ => {
            warn!("Unknown metric name: {}", name);
            0.0
        }
    }
}

// ══════════════════════════════════════════════════════════════
// Filters
// ══════════════════════════════════════════════════════════════

/// Check whether backtest metrics pass all filter conditions.
fn passes_filters(metrics: &BacktestMetrics, filters: &[BuilderFilterCondition]) -> bool {
    for f in filters {
        let left = extract_metric_by_name(metrics, &f.left_value);
        let right = f.right_value;
        let ok = match f.operator {
            BuilderFilterOperator::Gte => left >= right,
            BuilderFilterOperator::Gt => left > right,
            BuilderFilterOperator::Lte => left <= right,
            BuilderFilterOperator::Lt => left < right,
            BuilderFilterOperator::Eq => (left - right).abs() < f64::EPSILON,
        };
        if !ok {
            return false;
        }
    }
    true
}

// ══════════════════════════════════════════════════════════════
// Selection & Migration
// ══════════════════════════════════════════════════════════════

/// Tournament selection: pick the best individual from k random contestants.
fn tournament_select<'a>(
    population: &'a [BuilderIndividual],
    k: usize,
    rng: &mut impl Rng,
) -> usize {
    if population.is_empty() {
        return 0;
    }
    let k = k.min(population.len());
    let mut best_idx = rng.gen_range(0..population.len());
    let mut best_fit = population[best_idx].fitness;

    for _ in 1..k {
        let idx = rng.gen_range(0..population.len());
        if population[idx].fitness > best_fit {
            best_fit = population[idx].fitness;
            best_idx = idx;
        }
    }
    best_idx
}

/// Ring-topology migration: move top individuals from island i to island (i+1) % N.
/// Population is assumed to be sorted descending by fitness (guaranteed by main loop).
/// Updates each target island's fingerprint_set in-place (no full rebuild needed).
fn ring_migration(islands: &mut [Island], migration_rate_pct: f64, rng: &mut impl Rng) {
    let n = islands.len();
    if n < 2 {
        return;
    }

    // Collect migrants from each island — population already sorted descending by fitness.
    let mut migrants: Vec<Vec<BuilderIndividual>> = Vec::with_capacity(n);

    // Track the intended migration count per island for injection limiting.
    let mut migration_counts: Vec<usize> = Vec::with_capacity(n);

    for island in islands.iter() {
        let count = ((island.population.len() as f64 * migration_rate_pct / 100.0).ceil() as usize)
            .max(1)
            .min(island.population.len());

        migration_counts.push(count);

        // Take up to 2× as many candidates so that duplicate skips don't starve migration.
        let candidate_count = (count * 2).min(island.population.len());
        let island_migrants: Vec<BuilderIndividual> = island.population.iter()
            .take(candidate_count)
            .map(|ind| BuilderIndividual {
                strategy: ind.strategy.clone(),
                fingerprint: ind.fingerprint,
                fitness: ind.fitness,
                metrics: ind.metrics.clone(),
                mini_equity_curve: ind.mini_equity_curve.clone(),
                trade_bar_hashes: ind.trade_bar_hashes.clone(),
                rule_count: ind.rule_count,
            })
            .collect();

        migrants.push(island_migrants);
    }

    // Inject migrants into the next island (ring topology).
    // Update fingerprint_set incrementally — no full rebuild after migration.
    for i in 0..n {
        let target = (i + 1) % n;
        let incoming = &migrants[i];
        let max_accepted = migration_counts[i];
        let mut accepted = 0;

        for migrant in incoming {
            if accepted >= max_accepted {
                break;
            }
            if islands[target].population.is_empty() {
                continue;
            }
            // Skip if the target island already contains this exact strategy (same fingerprint).
            if islands[target].fingerprint_set.contains(&migrant.fingerprint) {
                continue;
            }
            // Replace a random weak individual (bottom half)
            let replace_idx = rng.gen_range(
                islands[target].population.len() / 2..islands[target].population.len(),
            );
            let old_fp = islands[target].population[replace_idx].fingerprint;
            islands[target].population[replace_idx] = BuilderIndividual {
                strategy: migrant.strategy.clone(),
                fingerprint: migrant.fingerprint,
                fitness: migrant.fitness,
                metrics: migrant.metrics.clone(),
                mini_equity_curve: migrant.mini_equity_curve.clone(),
                trade_bar_hashes: migrant.trade_bar_hashes.clone(),
                rule_count: migrant.rule_count,
            };
            // Incremental fingerprint_set update — avoids full O(N) rebuild.
            islands[target].fingerprint_set.remove(&old_fp);
            islands[target].fingerprint_set.insert(migrant.fingerprint);
            accepted += 1;
        }
    }
}

// ══════════════════════════════════════════════════════════════
// Equity curve downsampling
// ══════════════════════════════════════════════════════════════

/// Downsample an equity curve to at most `max_points` for sparkline display.
fn downsample_equity_curve(equity: &[EquityPoint], max_points: usize) -> Vec<f64> {
    if equity.is_empty() {
        return vec![];
    }
    if equity.len() <= max_points {
        return equity.iter().map(|e| e.equity).collect();
    }

    let step = equity.len() as f64 / max_points as f64;
    let mut result = Vec::with_capacity(max_points);
    for i in 0..max_points {
        let idx = (i as f64 * step) as usize;
        let idx = idx.min(equity.len() - 1);
        result.push(equity[idx].equity);
    }

    // Always include the last point
    if let Some(last) = equity.last() {
        if let Some(r_last) = result.last_mut() {
            *r_last = last.equity;
        }
    }

    result
}

// ══════════════════════════════════════════════════════════════
// Data splitting (IS/OOS)
// ══════════════════════════════════════════════════════════════

/// Split data for IS/OOS based on data range parts configuration.
struct DataSplit<'a> {
    /// In-sample candles
    is_candles: &'a [Candle],
    /// Out-of-sample candles (may be empty)
    oos_candles: &'a [Candle],
    /// Full data (for Full fitness source)
    full_candles: &'a [Candle],
}

fn split_data<'a>(candles: &'a [Candle], parts: &[BuilderDataRangePart]) -> DataSplit<'a> {
    if parts.is_empty() || candles.is_empty() {
        return DataSplit {
            is_candles: candles,
            oos_candles: &[],
            full_candles: candles,
        };
    }

    // Sum up IS and OOS percentages
    let is_pct: f64 = parts
        .iter()
        .filter(|p| p.part_type == BuilderDataRangePartType::Is)
        .map(|p| p.percent)
        .sum();
    let total_pct: f64 = parts.iter().map(|p| p.percent).sum();

    let is_fraction = if total_pct > 0.0 {
        is_pct / total_pct
    } else {
        1.0
    };

    let is_len = ((candles.len() as f64 * is_fraction).round() as usize)
        .max(1)
        .min(candles.len());

    DataSplit {
        is_candles: &candles[..is_len],
        oos_candles: &candles[is_len..],
        full_candles: candles,
    }
}

// ══════════════════════════════════════════════════════════════
// BuilderSavedStrategy conversion
// ══════════════════════════════════════════════════════════════

fn individual_to_saved(
    ind: &BuilderIndividual,
    config: &BuilderConfig,
    oos_candles: &[Candle],
    instrument: &InstrumentConfig,
    backtest_config: &BacktestConfig,
    shared_cache: &Arc<IndicatorCache>,
    cancel_flag: &AtomicBool,
) -> Option<BuilderSavedStrategy> {
    let metrics = ind.metrics.as_ref()?;

    let strategy_json = serde_json::to_string(&ind.strategy).unwrap_or_default();

    let win_loss_ratio = if metrics.losing_trades > 0 {
        metrics.winning_trades as f64 / metrics.losing_trades as f64
    } else if metrics.winning_trades > 0 {
        999.0
    } else {
        0.0
    };

    let cagr_max_dd = if metrics.max_drawdown_pct.abs() > f64::EPSILON {
        metrics.annualized_return_pct / metrics.max_drawdown_pct.abs()
    } else {
        0.0
    };

    let r_expectancy = if metrics.avg_loss.abs() > f64::EPSILON {
        metrics.expectancy / metrics.avg_loss.abs()
    } else {
        metrics.expectancy
    };

    let avg_bars_win = if metrics.winning_trades > 0 {
        metrics.avg_bars_in_trade // approximate
    } else {
        0.0
    };

    // Compute OOS metrics when OOS data is available
    let (oos_net_profit, oos_trades, oos_profit_factor, oos_sharpe_ratio,
         oos_max_drawdown_abs, oos_win_rate_pct) =
        if !oos_candles.is_empty() && max_lookback(&ind.strategy) < oos_candles.len().saturating_sub(1) {
            match executor::run_backtest_with_cache(
                oos_candles,
                &SubBarData::None,
                &ind.strategy,
                backtest_config,
                instrument,
                cancel_flag,
                |_, _, _| {},
                Arc::clone(shared_cache),
            ) {
                Ok(result) => {
                    let m = &result.metrics;
                    (
                        Some(m.net_profit),
                        Some(m.total_trades),
                        Some(m.profit_factor),
                        Some(m.sharpe_ratio),
                        Some(m.max_drawdown_abs),
                        Some(m.win_rate_pct),
                    )
                }
                Err(_) => (None, None, None, None, None, None),
            }
        } else {
            (None, None, None, None, None, None)
        };

    Some(BuilderSavedStrategy {
        id: gen_id(),
        name: ind.strategy.name.clone(),
        created_at: now_iso(),
        fitness: ind.fitness,
        symbol_id: config.data_config.symbol_id.clone(),
        symbol_name: config
            .data_config
            .symbol_id
            .clone()
            .unwrap_or_else(|| "Unknown".to_string()),
        timeframe: config.data_config.timeframe,
        net_profit: metrics.net_profit,
        mini_equity_curve: ind.mini_equity_curve.clone(),
        trades: metrics.total_trades,
        profit_factor: metrics.profit_factor,
        sharpe_ratio: metrics.sharpe_ratio,
        r_expectancy,
        annual_return_pct: metrics.annualized_return_pct,
        max_drawdown_abs: metrics.max_drawdown_abs,
        win_loss_ratio,
        ret_dd_ratio: metrics.return_dd_ratio,
        cagr_max_dd_pct: cagr_max_dd,
        avg_win: metrics.avg_win,
        avg_loss: metrics.avg_loss,
        avg_bars_win,
        strategy_json,
        fingerprint: ind.fingerprint,
        oos_net_profit,
        oos_trades,
        oos_profit_factor,
        oos_sharpe_ratio,
        oos_max_drawdown_abs,
        oos_win_rate_pct,
    })
}

// ══════════════════════════════════════════════════════════════
// Evaluate individual
// ══════════════════════════════════════════════════════════════

/// Evaluate a single individual by running a backtest.
/// Each rayon thread builds its own local indicator cache — no shared mutex contention.
/// Build a u64 bitmask where bit N is set if the strategy uses `IndicatorType` with
/// discriminant N.  Because there are ≤ 64 indicator variants (u8 cast) this fits in
/// a single word — no heap allocation, no HashSet.
fn compute_indicator_bitmask(s: &Strategy) -> u64 {
    fn add_op(op: &Operand, mask: &mut u64) {
        if op.operand_type == OperandType::Indicator {
            if let Some(ref ind) = op.indicator {
                *mask |= 1u64 << (ind.indicator_type as u8);
            }
        }
        // Recurse into compound sub-operands.
        if let Some(ref left)  = op.compound_left  { add_op(left,  mask); }
        if let Some(ref right) = op.compound_right { add_op(right, mask); }
    }
    let mut mask = 0u64;
    let flat = s.long_entry_rules.iter()
        .chain(s.short_entry_rules.iter())
        .chain(s.long_exit_rules.iter())
        .chain(s.short_exit_rules.iter());
    let groups = s.long_entry_groups.iter()
        .chain(s.short_entry_groups.iter())
        .chain(s.long_exit_groups.iter())
        .chain(s.short_exit_groups.iter())
        .flat_map(|g| g.rules.iter());
    for rule in flat.chain(groups) {
        add_op(&rule.left_operand,  &mut mask);
        add_op(&rule.right_operand, &mut mask);
    }
    mask
}

/// Jaccard distance from two indicator bitmasks — pure integer arithmetic, no allocs.
#[inline]
fn jaccard_from_masks(a: u64, b: u64) -> f64 {
    let inter = (a & b).count_ones();
    let union = (a | b).count_ones();
    if union == 0 { return 0.0; }
    1.0 - inter as f64 / union as f64
}

/// Compute Jaccard distance (0=identical, 1=completely different) between two strategies
/// based on the set of indicator types used across all rule sets.
fn structural_distance(a: &Strategy, b: &Strategy) -> f64 {
    jaccard_from_masks(compute_indicator_bitmask(a), compute_indicator_bitmask(b))
}

// ── Behavioral distance metrics for niching ──────────────────────────────────

/// Pearson-correlation-based distance between two equity curves.
/// Returns a value in [0, 1]: 0 = identical, 1 = perfectly anti-correlated.
/// Two strategies are "close" when they profit and lose at the same bars.
fn equity_curve_distance(a: &[f64], b: &[f64]) -> f64 {
    if a.len() < 2 || b.len() < 2 || a.len() != b.len() {
        return 1.0;
    }
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let cov: f64 = a.iter().zip(b).map(|(x, y)| (x - mean_a) * (y - mean_b)).sum::<f64>() / n;
    let var_a: f64 = a.iter().map(|x| (x - mean_a).powi(2)).sum::<f64>() / n;
    let var_b: f64 = b.iter().map(|y| (y - mean_b).powi(2)).sum::<f64>() / n;
    let denom = var_a.sqrt() * var_b.sqrt();
    if denom < 1e-10 { return 1.0; }
    let r = (cov / denom).clamp(-1.0, 1.0);
    (1.0 - r) * 0.5 // maps [-1, 1] → [0, 1]
}

/// Jaccard distance on trade entry timestamps (hashed to u32).
/// Returns a value in [0, 1]: 0 = identical trade times, 1 = no overlap.
fn trade_overlap_distance(a: &[u32], b: &[u32]) -> f64 {
    if a.is_empty() && b.is_empty() { return 0.0; }
    let set_a: std::collections::HashSet<u32> = a.iter().copied().collect();
    let set_b: std::collections::HashSet<u32> = b.iter().copied().collect();
    let inter = set_a.intersection(&set_b).count();
    let union = set_a.len() + set_b.len() - inter;
    if union == 0 { return 0.0; }
    1.0 - inter as f64 / union as f64
}

/// Dispatch to the configured distance metric.
fn behavioral_distance(a: &BuilderIndividual, b: &BuilderIndividual, mode: BehavioralNichingMode) -> f64 {
    match mode {
        BehavioralNichingMode::Structural =>
            structural_distance(&a.strategy, &b.strategy),
        BehavioralNichingMode::EquityCurve =>
            equity_curve_distance(&a.mini_equity_curve, &b.mini_equity_curve),
        BehavioralNichingMode::TradeOverlap =>
            trade_overlap_distance(&a.trade_bar_hashes, &b.trade_bar_hashes),
        BehavioralNichingMode::Combined => {
            let d_ec = equity_curve_distance(&a.mini_equity_curve, &b.mini_equity_curve);
            let d_to = trade_overlap_distance(&a.trade_bar_hashes, &b.trade_bar_hashes);
            (d_ec + d_to) * 0.5
        }
    }
}

/// Apply fitness sharing (niching) to a population slice.
/// Individuals closer than `sigma` in behavioral distance have their fitness reduced,
/// maintaining population diversity.
///
/// For `Structural` mode uses precomputed u64 indicator bitmasks — pure integer
/// arithmetic, no HashSet allocations — and short-circuits when no pair lies within
/// `sigma` (population already fully diverse, sharing would be a no-op).
///
/// For other modes precomputes the upper triangle of the symmetric distance matrix so
/// each pair (i, j) is evaluated exactly once — 50% fewer distance calls vs the naive
/// O(N²) double loop.
fn apply_fitness_sharing(
    pop: &mut Vec<BuilderIndividual>,
    sigma: f64,
    alpha: f64,
    mode: BehavioralNichingMode,
) {
    let n = pop.len();
    if n < 2 { return; }
    let raw: Vec<f64> = pop.iter().map(|i| i.fitness).collect();

    // Shift all fitnesses to be ≥ 1.0 before dividing so that dividing by denom > 1
    // always penalizes (moves fitness away from zero / the shifted origin), regardless of sign.
    let min_raw = raw.iter()
        .filter(|&&v| v != f64::NEG_INFINITY)
        .cloned()
        .fold(f64::INFINITY, f64::min);
    let shift = if min_raw < 1.0 { 1.0 - min_raw } else { 0.0 };

    // ── Fast path: Structural mode with bitmask arithmetic ────────────────────
    // Pre-compute one u64 per individual (O(N) tree traversal, no alloc per pair).
    // Then check if ANY pair is within sigma before building the full distance matrix.
    // For a highly diverse population this early-exit avoids the entire O(N²) scan.
    if mode == BehavioralNichingMode::Structural {
        let masks: Vec<u64> = pop.iter().map(|ind| compute_indicator_bitmask(&ind.strategy)).collect();

        // O(N²/2) early-exit using pure integer ops (~3 ns per pair).
        let any_close = (0..n).any(|i| {
            if raw[i] == f64::NEG_INFINITY { return false; }
            ((i + 1)..n).any(|j| {
                if raw[j] == f64::NEG_INFINITY { return false; }
                jaccard_from_masks(masks[i], masks[j]) < sigma
            })
        });
        if !any_close { return; }

        // Full O(N²) distance matrix — bit ops only, no heap allocation per pair.
        let mut dist = vec![0.0f64; n * n];
        for i in 0..n {
            if raw[i] == f64::NEG_INFINITY { continue; }
            for j in (i + 1)..n {
                if raw[j] == f64::NEG_INFINITY { continue; }
                let d = jaccard_from_masks(masks[i], masks[j]);
                dist[i * n + j] = d;
                dist[j * n + i] = d;
            }
        }
        for i in 0..n {
            if raw[i] == f64::NEG_INFINITY { continue; }
            let mut denom = 0.0f64;
            for j in 0..n {
                if raw[j] == f64::NEG_INFINITY { continue; }
                let d = dist[i * n + j];
                if d < sigma {
                    denom += 1.0 - (d / sigma).powf(alpha);
                }
            }
            if denom > 0.0 {
                pop[i].fitness = (raw[i] + shift) / denom - shift;
            }
        }
        return;
    }

    // ── Generic path: EquityCurve / TradeOverlap / Combined ──────────────────
    // Precompute upper triangle; fill both (i,j) and (j,i) — distance is symmetric.
    let mut dist = vec![0.0f64; n * n];
    for i in 0..n {
        if raw[i] == f64::NEG_INFINITY { continue; }
        for j in (i + 1)..n {
            if raw[j] == f64::NEG_INFINITY { continue; }
            let d = behavioral_distance(&pop[i], &pop[j], mode);
            dist[i * n + j] = d;
            dist[j * n + i] = d;
        }
    }

    for i in 0..n {
        if raw[i] == f64::NEG_INFINITY { continue; }
        let mut denom = 0.0f64;
        for j in 0..n {
            if raw[j] == f64::NEG_INFINITY { continue; }
            let d = dist[i * n + j];
            if d < sigma {
                denom += 1.0 - (d / sigma).powf(alpha);
            }
        }
        if denom > 0.0 {
            // Operate in shifted space so division always penalises similarity correctly.
            pop[i].fitness = (raw[i] + shift) / denom - shift;
        }
    }
}

// ── Meta-learning ──────────────────────────────────────────────────────────────

/// Update an island's grammar based on the best-performing individuals.
/// Uses EMA blending: new_weight = (1-lr)*old + lr*observed_proportion.
/// Called once per generation after the population has been sorted descending.
fn update_island_grammar(
    grammar: &mut GrammarContext,
    population: &[BuilderIndividual],
    lr: f64,
    top_pct: f64,
) {
    if lr <= 0.0 || population.is_empty() { return; }

    let valid: Vec<&BuilderIndividual> = population
        .iter()
        .filter(|i| i.fitness > f64::NEG_INFINITY)
        .collect();
    if valid.is_empty() { return; }

    let top_n = ((valid.len() as f64 * top_pct).ceil() as usize).max(1);
    let top = &valid[..top_n.min(valid.len())];

    let mut ind_counts: std::collections::HashMap<IndicatorType, usize> =
        std::collections::HashMap::new();
    let mut period_sums: std::collections::HashMap<IndicatorType, (f64, usize)> =
        std::collections::HashMap::new();

    for ind in top {
        let s = &ind.strategy;
        let flat = s.long_entry_rules.iter()
            .chain(s.short_entry_rules.iter())
            .chain(s.long_exit_rules.iter())
            .chain(s.short_exit_rules.iter());
        let grouped = s.long_entry_groups.iter().flat_map(|g| g.rules.iter())
            .chain(s.short_entry_groups.iter().flat_map(|g| g.rules.iter()))
            .chain(s.long_exit_groups.iter().flat_map(|g| g.rules.iter()))
            .chain(s.short_exit_groups.iter().flat_map(|g| g.rules.iter()));
        for rule in flat.chain(grouped) {
            for operand in [&rule.left_operand, &rule.right_operand] {
                visit_operand_for_meta(operand, &mut ind_counts, &mut period_sums);
            }
        }
    }

    // EMA-update indicator weights
    let total_obs: usize = ind_counts.values().sum();
    if total_obs > 0 && !grammar.adapted_indicators.is_empty() {
        let n = grammar.adapted_indicators.len() as f64;
        for (ind_type, weight) in grammar.adapted_indicators.iter_mut() {
            let observed = ind_counts.get(ind_type).copied().unwrap_or(0);
            // Scale: normalized so that a "flat" distribution would give each indicator weight 100
            let target = (observed as f64 / total_obs as f64) * n * 100.0;
            let new_w = (1.0 - lr) * *weight as f64 + lr * target;
            // Clamp to [0.5×baseline, 2.0×baseline] to prevent drift
            let baseline = grammar.enabled_indicators.iter()
                .find(|(t, _)| *t == *ind_type)
                .map(|(_, w)| *w as f64)
                .unwrap_or(100.0);
            *weight = new_w
                .max(baseline * 0.5)
                .min(baseline * 2.0)
                .round()
                .max(1.0) as usize;
        }
    }

    // EMA-update period means
    for (ind_type, (sum, count)) in period_sums {
        if count == 0 { continue; }
        let obs_mean = sum / count as f64;
        let entry = grammar.period_means.entry(ind_type).or_insert(obs_mean);
        *entry = (1.0 - lr) * *entry + lr * obs_mean;
    }

    grammar.meta_updates += 1;
}

/// Recursively collect indicator types and primary periods from an operand
/// (handles both plain and Compound operands).
fn visit_operand_for_meta(
    operand: &Operand,
    ind_counts: &mut std::collections::HashMap<IndicatorType, usize>,
    period_sums: &mut std::collections::HashMap<IndicatorType, (f64, usize)>,
) {
    match operand.operand_type {
        OperandType::Indicator => {
            if let Some(ref ic) = operand.indicator {
                *ind_counts.entry(ic.indicator_type).or_insert(0) += 1;
                // Use the "primary" period (first non-None of period / k_period / fast_period)
                let p = ic.params.period.or(ic.params.k_period).or(ic.params.fast_period);
                if let Some(period) = p {
                    let e = period_sums.entry(ic.indicator_type).or_insert((0.0, 0));
                    e.0 += period as f64;
                    e.1 += 1;
                }
            }
        }
        OperandType::Compound => {
            if let Some(ref left) = operand.compound_left {
                visit_operand_for_meta(left, ind_counts, period_sums);
            }
            if let Some(ref right) = operand.compound_right {
                visit_operand_for_meta(right, ind_counts, period_sums);
            }
        }
        _ => {}
    }
}

fn evaluate_individual(
    ind: &mut BuilderIndividual,
    candles: &[Candle],
    instrument: &InstrumentConfig,
    backtest_config: &BacktestConfig,
    cancel_flag: &AtomicBool,
    shared_cache: &Arc<IndicatorCache>,
) {
    // Fast pre-check: if lookback exceeds data, bar loop runs 0 iterations (guaranteed 0 trades)
    if max_lookback(&ind.strategy) >= candles.len().saturating_sub(1) {
        ind.fitness = f64::NEG_INFINITY;
        ind.metrics = None;
        return;
    }

    let result = executor::run_backtest_with_cache(
        candles,
        &SubBarData::None,
        &ind.strategy,
        backtest_config,
        instrument,
        cancel_flag,
        |_, _, _| {},
        Arc::clone(shared_cache),
    );

    match result {
        Ok(bt) => {
            ind.mini_equity_curve = downsample_equity_curve(&bt.equity_curve, 50);
            // Hash each trade's entry_time string to a u32 for O(1) Jaccard distance.
            ind.trade_bar_hashes = bt.trades.iter().map(|t| {
                let mut h = DefaultHasher::new();
                t.entry_time.hash(&mut h);
                h.finish() as u32
            }).collect();
            ind.metrics = Some(bt.metrics);
        }
        Err(e) => {
            // Strategy produced an error (e.g. insufficient data); treat as worst fitness
            if !matches!(e, AppError::BacktestCancelled) {
                ind.fitness = f64::NEG_INFINITY;
                ind.metrics = None;
                ind.mini_equity_curve = vec![];
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════
// Main builder function
// ══════════════════════════════════════════════════════════════

/// Run the Grammar-Based Genetic Programming builder engine.
///
/// This is the main entry point. It evolves trading strategies from scratch using
/// an island-model GP with grammar-guided random generation, crossover, and mutation.
///
/// # Arguments
/// * `candles` - Full OHLCV data for the selected symbol/timeframe
/// * `instrument` - Instrument configuration (pip size, lot size, etc.)
/// * `backtest_config` - Base backtest config (symbol, timeframe, dates, capital)
/// * `config` - Complete builder configuration from the frontend
/// * `cancel_flag` - Atomic flag to request cancellation
/// * `pause_flag` - Atomic flag to pause/resume
/// * `progress_cb` - Callback for progress events
pub fn run_builder(
    candles: &[Candle],
    instrument: &InstrumentConfig,
    backtest_config: &BacktestConfig,
    config: &BuilderConfig,
    cancel_flag: &AtomicBool,
    pause_flag: &AtomicBool,
    tx: SyncSender<BuilderProgressEvent>,
) -> Result<Vec<BuilderSavedStrategy>, AppError> {
    let tx = SyncTx(tx);
    let start_time = Instant::now();

    info!(
        "Builder starting: {} candles, {} islands, {} pop/island, {} max generations",
        candles.len(),
        config.genetic_options.islands,
        config.genetic_options.population_per_island,
        config.genetic_options.max_generations,
    );

    if candles.is_empty() {
        return Err(AppError::BuilderError("No candle data provided".into()));
    }

    // 1. Build grammar context
    let grammar = GrammarContext::from_config(config);

    if grammar.enabled_indicators.is_empty() {
        return Err(AppError::BuilderError(
            "No indicators enabled in building blocks".into(),
        ));
    }

    // 2. Split data into IS/OOS
    let data_split = split_data(candles, &config.data_config.data_range_parts);

    let _ = tx.send(BuilderProgressEvent::Log(format!(
        "Data split: IS={} bars, OOS={} bars",
        data_split.is_candles.len(),
        data_split.oos_candles.len(),
    )));

    if data_split.is_candles.len() < 50 {
        return Err(AppError::BuilderError(
            "In-sample data too short (need at least 50 bars)".into(),
        ));
    }

    // Databank: accepted strategies
    let databank: Arc<Mutex<Vec<BuilderSavedStrategy>>> = Arc::new(Mutex::new(Vec::new()));
    let max_databank_size = config.ranking.max_strategies_to_store;

    // Counters
    let total_generated = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let total_accepted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let total_rejected = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // Determine fitness data source
    let fitness_candles: &[Candle] = match config.ranking.fitness_source {
        BuilderFitnessSource::MainData | BuilderFitnessSource::InSample => {
            data_split.is_candles
        }
        BuilderFitnessSource::OutOfSample => {
            if data_split.oos_candles.is_empty() {
                data_split.is_candles
            } else {
                data_split.oos_candles
            }
        }
        BuilderFitnessSource::Full => data_split.full_candles,
    };

    // Stop conditions from config
    let stop_totally_count = config.ranking.stop_totally_count;
    let stop_when = config.ranking.stop_when;
    let stop_after_seconds = (config.ranking.stop_after_days as u64 * 86400)
        + (config.ranking.stop_after_hours as u64 * 3600)
        + (config.ranking.stop_after_minutes as u64 * 60);

    // Initial filter conditions
    let initial_filters = &config.genetic_options.initial_filters;
    let ranking_filters = &config.ranking.custom_filters;

    let ga = &config.genetic_options;

    // Normalize probabilities: frontend sends 0-100 (percent), engine expects 0.0-1.0
    let crossover_prob = if ga.crossover_probability > 1.0 {
        ga.crossover_probability / 100.0
    } else {
        ga.crossover_probability
    };
    let mutation_prob = if ga.mutation_probability > 1.0 {
        ga.mutation_probability / 100.0
    } else {
        ga.mutation_probability
    };

    let _ = tx.send(BuilderProgressEvent::Log(format!(
        "Config: crossover={:.0}%, mutation={:.0}%, islands={}, pop/island={}, generations={}",
        crossover_prob * 100.0,
        mutation_prob * 100.0,
        ga.islands,
        ga.population_per_island,
        ga.max_generations,
    )));

    // Main loop (may restart if start_again_when_finished)
    'outer: loop {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(AppError::BuilderCancelled);
        }

        // 4. Create N islands with initial populations
        let num_islands = ga.islands.max(1);
        let pop_per_island = ga.population_per_island.max(4);
        // decimation_coefficient: generate pop_per_island × coeff candidates, keep top pop_per_island
        let decimation = ga.decimation_coefficient.max(1.0);
        let initial_gen_size = ((pop_per_island as f64 * decimation).ceil() as usize)
            .max(pop_per_island)
            .max((ga.initial_population_size as f64 / num_islands as f64).ceil() as usize);

        let _ = tx.send(BuilderProgressEvent::Log(format!(
            "Creating {} islands with {} individuals each (generating {} candidates for decimation)",
            num_islands, pop_per_island, initial_gen_size,
        )));

        let mut islands: Vec<Island> = Vec::with_capacity(num_islands);

        // Persistent indicator cache shared across all islands and all generations.
        // Indicator values for (type, params) on the same dataset are immutable — a cache
        // entry computed in generation 1 is valid in generation 500. After the first generation
        // the cache reaches steady state: all subsequent generations serve 100% cache hits,
        // eliminating O(gens × pop × n_indicators × n_bars) recomputation.
        let persistent_cache: Arc<IndicatorCache> =
            Arc::new(IndicatorCache::new());

        // ── Pre-warm indicator cache before gen 0 ────────────────────────────────
        {
            let _ = tx.send(BuilderProgressEvent::Log(
                "Warming up indicator cache\u{2026}".to_string(),
            ));

            let w2b = &config.what_to_build;
            let period_min = w2b.indicator_period_min.max(2);
            let period_max = w2b.indicator_period_max.max(period_min + 2);

            // Sample up to 3 evenly-spaced periods in [period_min, period_max]
            let mut periods = std::collections::BTreeSet::new();
            periods.insert(period_min);
            periods.insert((period_min + period_max) / 2);
            periods.insert(period_max);
            let periods: Vec<usize> = periods.into_iter().collect();

            // Build one probe strategy per enabled indicator x sampled period
            let probes: Vec<Strategy> = config
                .building_blocks
                .indicators
                .iter()
                .filter(|b| b.enabled)
                .flat_map(|b| {
                    periods
                        .iter()
                        .filter_map(|&p| grammar.make_single_indicator_probe(b.indicator_type, p))
                        .collect::<Vec<_>>()
                })
                .collect();

            // Warm the cache in parallel
            probes.par_iter().for_each(|probe| {
                let _ = pre_compute_indicators_with_shared_cache(
                    probe,
                    fitness_candles,
                    &persistent_cache,
                );
            });

            let cache_size = persistent_cache.len();
            let _ = tx.send(BuilderProgressEvent::Log(format!(
                "Cache warm-up complete: {} indicator series pre-computed",
                cache_size
            )));
        }

        for island_id in 0..num_islands {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(AppError::BuilderCancelled);
            }

            // Generate initial population with decimation
            let mut candidates: Vec<BuilderIndividual> = (0..initial_gen_size)
                .into_par_iter()
                .map(|i| {
                    let mut rng = rand::thread_rng();
                    let name = format!("Island{}_{}", island_id, i);
                    let strategy = generate_random_strategy(&grammar, &mut rng, name, 0.0);
                    let (fp, rc) = fingerprint_and_count(&strategy);
                    BuilderIndividual {
                        strategy,
                        fingerprint: fp,
                        fitness: f64::NEG_INFINITY,
                        metrics: None,
                        mini_equity_curve: vec![],
                        trade_bar_hashes: vec![],
                        rule_count: rc,
                    }
                })
                .collect();

            // Evaluate candidates in parallel with the persistent indicator cache.
            let shared_cache = Arc::clone(&persistent_cache);
            candidates.par_iter_mut().for_each(|ind| {
                if cancel_flag.load(Ordering::Relaxed) {
                    return;
                }
                evaluate_individual(
                    ind,
                    fitness_candles,
                    instrument,
                    backtest_config,
                    cancel_flag,
                    &shared_cache,
                );
                if let Some(ref m) = ind.metrics {
                    ind.fitness = compute_fitness(m, &config.ranking);
                    if config.ranking.complexity_alpha > 0.0 {
                        let min_rules = (grammar.min_entry_rules + grammar.min_exit_rules) * 2;
                        let n = ind.rule_count;
                        let extra = n.saturating_sub(min_rules) as f64;
                        ind.fitness /= 1.0 + config.ranking.complexity_alpha * extra;
                    }
                }
            });

            if cancel_flag.load(Ordering::Relaxed) {
                return Err(AppError::BuilderCancelled);
            }

            // Apply initial filters for decimation
            if !initial_filters.is_empty() {
                candidates.retain(|ind| {
                    ind.metrics
                        .as_ref()
                        .map(|m| passes_filters(m, initial_filters))
                        .unwrap_or(false)
                });
            }

            // Apply fitness sharing before selection to maintain diversity
            let sigma = config.genetic_options.fitness_sharing_sigma;
            if sigma > 0.0 {
                apply_fitness_sharing(&mut candidates, sigma, config.genetic_options.fitness_sharing_alpha, config.genetic_options.niching_mode);
            }

            // Sort by fitness descending and keep top pop_per_island
            candidates.sort_by(|a, b| {
                b.fitness
                    .partial_cmp(&a.fitness)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            candidates.truncate(pop_per_island);

            // If we don't have enough survivors, fill with random new ones
            while candidates.len() < pop_per_island {
                let mut rng = rand::thread_rng();
                let name = format!("Island{}_fill_{}", island_id, candidates.len());
                let strategy = generate_random_strategy(&grammar, &mut rng, name, 0.0);
                let (fp, rc) = fingerprint_and_count(&strategy);
                candidates.push(BuilderIndividual {
                    strategy,
                    fingerprint: fp,
                    fitness: f64::NEG_INFINITY,
                    metrics: None,
                    mini_equity_curve: vec![],
                    trade_bar_hashes: vec![],
                    rule_count: rc,
                });
            }

            let best = candidates
                .first()
                .map(|c| c.fitness)
                .unwrap_or(f64::NEG_INFINITY);

            total_generated.fetch_add(initial_gen_size, Ordering::Relaxed);

            let fp_set: std::collections::HashSet<u64> =
                candidates.iter().map(|c| c.fingerprint).collect();
            islands.push(Island {
                id: island_id,
                population: candidates,
                fingerprint_set: fp_set,
                generation: 0,
                best_fitness: best,
                best_raw_fitness: best,
                stagnation_count: 0,
                effective_mutation_prob: mutation_prob,
                grammar: grammar.clone(),
            });

            let _ = tx.send(BuilderProgressEvent::Log(format!(
                "Island {} initialized, best fitness: {:.4}",
                island_id, best,
            )));
        }

        // 4b. Check initial population for strategies that pass ranking filters -> databank
        for island in &islands {
            for ind in &island.population {
                if let Some(ref m) = ind.metrics {
                    if passes_filters(m, ranking_filters) {
                        let is_duplicate = if config.ranking.dismiss_similar {
                            let db = databank.lock().unwrap_or_else(|e| e.into_inner());
                            db.iter().any(|s| s.fingerprint == ind.fingerprint)
                        } else {
                            false
                        };

                        if !is_duplicate {
                            if let Some(saved) = individual_to_saved(ind, config, data_split.oos_candles, instrument, backtest_config, &persistent_cache, cancel_flag) {
                                let mut db = databank.lock().unwrap_or_else(|e| e.into_inner());
                                let insert_pos = db
                                    .iter()
                                    .position(|s| s.fitness < saved.fitness)
                                    .unwrap_or(db.len());
                                db.insert(insert_pos, saved.clone());
                                if db.len() > max_databank_size {
                                    db.truncate(max_databank_size);
                                }
                                total_accepted.fetch_add(1, Ordering::Relaxed);
                                let _ = tx.send(BuilderProgressEvent::StrategyFound(saved));
                            }
                        }
                    }
                }
            }
        }

        let init_db_count = {
            let db = databank.lock().unwrap_or_else(|e| e.into_inner());
            db.len()
        };
        if init_db_count > 0 {
            let _ = tx.send(BuilderProgressEvent::Log(format!(
                "Initial population: {} strategies added to databank",
                init_db_count,
            )));
        }

        // 5. Generational loop
        for gen in 0..ga.max_generations {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err(AppError::BuilderCancelled);
            }

            // Pause loop
            while pause_flag.load(Ordering::Relaxed) {
                if cancel_flag.load(Ordering::Relaxed) {
                    return Err(AppError::BuilderCancelled);
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            // Evolve all islands in parallel. Each island gets a rayon thread and runs its
            // full generation independently. Shared state (databank, counters, tx) uses
            // Arc<Mutex>/AtomicXxx/SyncTx so no data races occur.
            let tx_ref = &tx;
            islands.par_iter_mut().for_each(|island| {
                if cancel_flag.load(Ordering::Relaxed) { return; }

                island.generation = gen;

                let pop_size = island.population.len();
                if pop_size < 2 {
                    return;
                }

                // Create next generation via selection, crossover, mutation
                let mut next_gen: Vec<BuilderIndividual> = Vec::with_capacity(pop_size);

                // Elitism: keep the best individual.
                // Population is always sorted descending by fitness after the previous
                // generation's sort, so the elite is always at index 0 — O(1).
                let elite = &island.population[0];
                next_gen.push(BuilderIndividual {
                    strategy: elite.strategy.clone(),
                    fingerprint: elite.fingerprint,
                    fitness: elite.fitness,
                    metrics: elite.metrics.clone(),
                    mini_equity_curve: elite.mini_equity_curve.clone(),
                    trade_bar_hashes: elite.trade_bar_hashes.clone(),
                    rule_count: elite.rule_count,
                });

                // Fill rest of population
                let mut rng = rand::thread_rng();
                let phase = if ga.max_generations > 0 { gen as f64 / ga.max_generations as f64 } else { 0.0 };

                // OPT-B: Track fingerprints already queued in next_gen so we avoid
                // scheduling a full backtest for structurally identical strategies.
                // When the island converges and unique individuals become hard to
                // generate, the dup_budget counter lets us accept duplicates rather
                // than looping forever.
                let mut next_gen_fp_set: std::collections::HashSet<u64> =
                    std::collections::HashSet::with_capacity(pop_size);
                next_gen_fp_set.insert(elite.fingerprint);
                let mut dup_budget = pop_size; // max extra retries before forcing a duplicate

                while next_gen.len() < pop_size {
                    let p1_idx = tournament_select(&island.population, 3, &mut rng);
                    let p2_idx = tournament_select(&island.population, 3, &mut rng);

                    if rng.gen::<f64>() < crossover_prob {
                        let (mut c1, mut c2) = crossover_strategies(
                            &island.population[p1_idx].strategy,
                            &island.population[p2_idx].strategy,
                            &island.grammar,
                            &mut rng,
                        );

                        if rng.gen::<f64>() < island.effective_mutation_prob {
                            mutate_strategy(&mut c1, &island.grammar, &mut rng, phase);
                        }
                        if rng.gen::<f64>() < island.effective_mutation_prob {
                            mutate_strategy(&mut c2, &island.grammar, &mut rng, phase);
                        }

                        let (fp1, rc1) = fingerprint_and_count(&c1);
                        let (fp2, rc2) = fingerprint_and_count(&c2);

                        // --- child 1 ---
                        if !next_gen_fp_set.contains(&fp1) {
                            next_gen_fp_set.insert(fp1);
                            c1.name = format!("G{}_I{}_{}", gen, island.id, next_gen.len());
                            next_gen.push(BuilderIndividual {
                                strategy: c1,
                                fingerprint: fp1,
                                fitness: f64::NEG_INFINITY,
                                metrics: None,
                                mini_equity_curve: vec![],
                                trade_bar_hashes: vec![],
                                rule_count: rc1,
                            });
                        } else if dup_budget == 0 {
                            // Converged island — accept duplicate to avoid hanging.
                            c1.name = format!("G{}_I{}_{}", gen, island.id, next_gen.len());
                            next_gen.push(BuilderIndividual {
                                strategy: c1,
                                fingerprint: fp1,
                                fitness: f64::NEG_INFINITY,
                                metrics: None,
                                mini_equity_curve: vec![],
                                trade_bar_hashes: vec![],
                                rule_count: rc1,
                            });
                        } else {
                            dup_budget -= 1;
                        }

                        // --- child 2 ---
                        if next_gen.len() < pop_size {
                            if !next_gen_fp_set.contains(&fp2) {
                                next_gen_fp_set.insert(fp2);
                                c2.name = format!("G{}_I{}_{}", gen, island.id, next_gen.len());
                                next_gen.push(BuilderIndividual {
                                    strategy: c2,
                                    fingerprint: fp2,
                                    fitness: f64::NEG_INFINITY,
                                    metrics: None,
                                    mini_equity_curve: vec![],
                                    trade_bar_hashes: vec![],
                                    rule_count: rc2,
                                });
                            } else if dup_budget == 0 {
                                c2.name = format!("G{}_I{}_{}", gen, island.id, next_gen.len());
                                next_gen.push(BuilderIndividual {
                                    strategy: c2,
                                    fingerprint: fp2,
                                    fitness: f64::NEG_INFINITY,
                                    metrics: None,
                                    mini_equity_curve: vec![],
                                    trade_bar_hashes: vec![],
                                    rule_count: rc2,
                                });
                            } else {
                                dup_budget -= 1;
                            }
                        }
                    } else {
                        let mut child = island.population[p1_idx].strategy.clone();
                        if rng.gen::<f64>() < island.effective_mutation_prob {
                            mutate_strategy(&mut child, &island.grammar, &mut rng, phase);
                        }
                        let (fp, rc) = fingerprint_and_count(&child);

                        if !next_gen_fp_set.contains(&fp) {
                            next_gen_fp_set.insert(fp);
                            child.name = format!("G{}_I{}_{}", gen, island.id, next_gen.len());
                            next_gen.push(BuilderIndividual {
                                strategy: child,
                                fingerprint: fp,
                                fitness: f64::NEG_INFINITY,
                                metrics: None,
                                mini_equity_curve: vec![],
                                trade_bar_hashes: vec![],
                                rule_count: rc,
                            });
                        } else if dup_budget == 0 {
                            child.name = format!("G{}_I{}_{}", gen, island.id, next_gen.len());
                            next_gen.push(BuilderIndividual {
                                strategy: child,
                                fingerprint: fp,
                                fitness: f64::NEG_INFINITY,
                                metrics: None,
                                mini_equity_curve: vec![],
                                trade_bar_hashes: vec![],
                                rule_count: rc,
                            });
                        } else {
                            dup_budget -= 1;
                        }
                    }
                }

                // Evaluate new individuals (skip elite which already has fitness).
                // Reuse the persistent cache — no reallocation needed; Rayon handles
                // nested par_iter_mut via work-stealing.
                let shared_cache = Arc::clone(&persistent_cache);
                next_gen[1..].par_iter_mut().for_each(|ind| {
                    if cancel_flag.load(Ordering::Relaxed) {
                        return;
                    }
                    // Pause: spin per individual so pausing takes effect mid-generation
                    // (otherwise the entire generation must complete before the generation-level
                    // pause check at the top of the outer loop is reached).
                    while pause_flag.load(Ordering::Relaxed) {
                        if cancel_flag.load(Ordering::Relaxed) { return; }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    evaluate_individual(
                        ind,
                        fitness_candles,
                        instrument,
                        backtest_config,
                        cancel_flag,
                        &shared_cache,
                    );
                    if let Some(ref m) = ind.metrics {
                        ind.fitness = compute_fitness(m, &config.ranking);
                        if config.ranking.complexity_alpha > 0.0 {
                            let min_rules = (grammar.min_entry_rules + grammar.min_exit_rules) * 2;
                            let n = ind.rule_count;
                            let extra = n.saturating_sub(min_rules) as f64;
                            ind.fitness /= 1.0 + config.ranking.complexity_alpha * extra;
                        }
                    }
                });

                if cancel_flag.load(Ordering::Relaxed) {
                    return; // Cancellation is checked again after the par_iter_mut completes
                }

                total_generated.fetch_add(next_gen.len() - 1, Ordering::Relaxed);

                // Count individuals that failed evaluation (backtest error → metrics = None)
                let eval_errors = next_gen[1..].iter().filter(|i| i.metrics.is_none()).count();
                if eval_errors > 0 {
                    total_rejected.fetch_add(eval_errors, Ordering::Relaxed);
                }

                // Log diagnostic info every generation for island 0
                if island.id == 0 {
                    let with_metrics = next_gen.iter().filter(|i| i.metrics.is_some()).count();
                    let with_trades = next_gen.iter().filter(|i| {
                        i.metrics.as_ref().map(|m| m.total_trades > 0).unwrap_or(false)
                    }).count();
                    let profitable = next_gen.iter().filter(|i| {
                        i.metrics.as_ref().map(|m| m.net_profit > 0.0).unwrap_or(false)
                    }).count();
                    let pass_filters = next_gen.iter().filter(|i| {
                        i.metrics.as_ref().map(|m| passes_filters(m, ranking_filters)).unwrap_or(false)
                    }).count();
                    let _ = tx_ref.send(BuilderProgressEvent::Log(format!(
                        "Gen {} Island 0: {}/{} have metrics, {}/{} have trades, {} profitable, {} pass filters",
                        gen, with_metrics, next_gen.len(), with_trades, next_gen.len(), profitable, pass_filters,
                    )));
                }

                // Check for strategies passing ranking filters -> add to databank.
                // Phase 1: collect passing individuals and count rejections (sequential — fast).
                // Phase 2: run OOS backtests sequentially per island.
                //   NOTE: the island loop itself runs in parallel (islands.par_iter_mut), so
                //   spawning another par_iter here would nest parallel inside parallel, saturating
                //   the rayon thread pool when many strategies pass in early generations.
                //   Sequential per island keeps each island's work bounded and predictable.
                let mut db_candidates: Vec<BuilderSavedStrategy> = vec![];
                for ind in &next_gen {
                    if let Some(ref m) = ind.metrics {
                        if passes_filters(m, ranking_filters) {
                            if let Some(saved) = individual_to_saved(ind, config, data_split.oos_candles, instrument, backtest_config, &persistent_cache, cancel_flag) {
                                db_candidates.push(saved);
                            }
                        } else {
                            total_rejected.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }

                // Phase 3: single lock acquisition for the entire island's batch.
                // Reduces (2 × passing_count) mutex acquisitions to 1 per island per generation.
                if !db_candidates.is_empty() {
                    let mut db = databank.lock().unwrap_or_else(|e| e.into_inner());
                    for saved in db_candidates {
                        let is_dup = config.ranking.dismiss_similar
                            && db.iter().any(|s| s.fingerprint == saved.fingerprint);
                        if !is_dup {
                            let insert_pos = db
                                .iter()
                                .position(|s| s.fitness < saved.fitness)
                                .unwrap_or(db.len());
                            db.insert(insert_pos, saved.clone());
                            if db.len() > max_databank_size {
                                db.truncate(max_databank_size);
                            }
                            total_accepted.fetch_add(1, Ordering::Relaxed);
                            let _ = tx_ref.send(BuilderProgressEvent::StrategyFound(saved));
                        }
                    }
                }

                // Update island population
                // Use next_gen_fp_set directly — it was maintained incrementally during
                // offspring generation, so no O(N) rebuild is needed here.
                let prev_best_raw = island.best_raw_fitness;
                island.population = next_gen;
                island.fingerprint_set = next_gen_fp_set;

                // Capture raw best BEFORE fitness sharing modifies individual fitnesses.
                // Stagnation detection must compare raw values; sharing-adjusted values fluctuate
                // with diversity even when the underlying best strategy hasn't changed.
                // Elite at population[0] carries sharing-adjusted fitness from the previous
                // generation — it would contaminate raw_new_best. Instead, use prev_best_raw as
                // the fold floor (elitism guarantees the raw best can't regress below it).
                let raw_new_best = island
                    .population
                    .iter()
                    .filter(|i| i.fitness != f64::NEG_INFINITY)
                    .map(|i| i.fitness)
                    .fold(prev_best_raw, f64::max);
                island.best_raw_fitness = raw_new_best;

                // Apply fitness sharing before selection to maintain population diversity
                let sigma = config.genetic_options.fitness_sharing_sigma;
                if sigma > 0.0 {
                    apply_fitness_sharing(&mut island.population, sigma, config.genetic_options.fitness_sharing_alpha, config.genetic_options.niching_mode);
                }

                island.population.sort_by(|a, b| {
                    b.fitness
                        .partial_cmp(&a.fitness)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                let new_best = island
                    .population
                    .first()
                    .map(|i| i.fitness)
                    .unwrap_or(f64::NEG_INFINITY);
                island.best_fitness = new_best;

                // Meta-learning: adapt grammar based on top performers
                let meta_lr = config.genetic_options.meta_learning_rate;
                if meta_lr > 0.0 {
                    update_island_grammar(
                        &mut island.grammar,
                        &island.population,
                        meta_lr,
                        config.genetic_options.meta_learning_top_pct,
                    );
                }

                // Stagnation detection uses raw (pre-sharing) fitness.
                let threshold = prev_best_raw.abs() * 1e-4;
                if (raw_new_best - prev_best_raw).abs() < threshold.max(f64::EPSILON) {
                    island.stagnation_count += 1;
                } else {
                    island.stagnation_count = 0;
                }

                // Adaptive mutation: boost when population diversity drops
                let diversity = if island.population.is_empty() {
                    1.0_f64
                } else {
                    island.fingerprint_set.len() as f64 / island.population.len() as f64
                };
                if diversity < 0.5 {
                    let cap = (mutation_prob * 3.0_f64).min(0.95_f64);
                    island.effective_mutation_prob =
                        (island.effective_mutation_prob + 0.05_f64).min(cap);
                } else if diversity > 0.8 {
                    // Decay back toward the configured base value
                    island.effective_mutation_prob = island.effective_mutation_prob
                        .mul_add(0.98_f64, mutation_prob * 0.02_f64);
                }

                // Fresh blood: replace worst N% with random individuals every M generations
                if ga.fresh_blood_replace_every > 0
                    && gen > 0
                    && gen % ga.fresh_blood_replace_every == 0
                {
                    let replace_count = ((pop_size as f64
                        * ga.fresh_blood_replace_percent
                        / 100.0)
                        .ceil() as usize)
                        .max(1)
                        .min(pop_size.saturating_sub(1));

                    let start_idx = pop_size.saturating_sub(replace_count);
                    for i in start_idx..pop_size {
                        let mut rng = rand::thread_rng();
                        let name = format!("Fresh_G{}_I{}_{}", gen, island.id, i);
                        let fresh_phase = if ga.max_generations > 0 { gen as f64 / ga.max_generations as f64 } else { 0.0 };
                        let new_strategy = generate_random_strategy(&island.grammar, &mut rng, name, fresh_phase);
                        let (fp, rc) = fingerprint_and_count(&new_strategy);

                        let already_exists = ga.fresh_blood_detect_duplicates
                            && island.fingerprint_set.contains(&fp);

                        if !already_exists {
                            let old_fp = island.population[i].fingerprint;
                            island.population[i] = BuilderIndividual {
                                strategy: new_strategy,
                                fingerprint: fp,
                                fitness: f64::NEG_INFINITY,
                                metrics: None,
                                mini_equity_curve: vec![],
                                trade_bar_hashes: vec![],
                                rule_count: rc,
                            };
                            // Incremental fingerprint_set update — remove old, add new.
                            island.fingerprint_set.remove(&old_fp);
                            island.fingerprint_set.insert(fp);
                        }
                    }

                    // Evaluate fresh blood using the persistent cache (warm N-bar indicators).
                    let shared_cache = Arc::clone(&persistent_cache);
                    island.population[start_idx..].par_iter_mut().for_each(|ind| {
                        if ind.metrics.is_none() && !cancel_flag.load(Ordering::Relaxed) {
                            evaluate_individual(
                                ind,
                                fitness_candles,
                                instrument,
                                backtest_config,
                                cancel_flag,
                                &shared_cache,
                            );
                            if let Some(ref m) = ind.metrics {
                                ind.fitness = compute_fitness(m, &config.ranking);
                                if config.ranking.complexity_alpha > 0.0 {
                                    let min_rules = (grammar.min_entry_rules + grammar.min_exit_rules) * 2;
                                    let n = ind.rule_count;
                                    let extra = n.saturating_sub(min_rules) as f64;
                                    ind.fitness /= 1.0 + config.ranking.complexity_alpha * extra;
                                }
                            }
                        }
                    });

                    total_generated.fetch_add(replace_count, Ordering::Relaxed);
                    // fingerprint_set was updated incrementally inside the fresh-blood loop.
                }

                // Restart on stagnation
                if ga.restart_on_stagnation
                    && island.stagnation_count >= ga.stagnation_generations
                {
                    let _ = tx_ref.send(BuilderProgressEvent::Log(format!(
                        "Island {} stagnated at gen {} (best: {:.4}), reinitializing",
                        island.id, gen, island.best_fitness,
                    )));

                    for i in 1..island.population.len() {
                        let mut rng = rand::thread_rng();
                        let name = format!("Restag_G{}_I{}_{}", gen, island.id, i);
                        let restag_phase = if ga.max_generations > 0 { gen as f64 / ga.max_generations as f64 } else { 0.0 };
                        let strategy = generate_random_strategy(&island.grammar, &mut rng, name, restag_phase);
                        let (fp, rc) = fingerprint_and_count(&strategy);
                        let old_fp = island.population[i].fingerprint;
                        island.population[i] = BuilderIndividual {
                            strategy,
                            fingerprint: fp,
                            fitness: f64::NEG_INFINITY,
                            metrics: None,
                            mini_equity_curve: vec![],
                            trade_bar_hashes: vec![],
                            rule_count: rc,
                        };
                        // Incremental fingerprint_set update.
                        island.fingerprint_set.remove(&old_fp);
                        island.fingerprint_set.insert(fp);
                    }

                    // Use the persistent cache: new individuals can reuse the N-bar
                    // indicator values already computed for previous generations.
                    let shared_cache = Arc::clone(&persistent_cache);
                    island.population[1..].par_iter_mut().for_each(|ind| {
                        if !cancel_flag.load(Ordering::Relaxed) {
                            evaluate_individual(
                                ind,
                                fitness_candles,
                                instrument,
                                backtest_config,
                                cancel_flag,
                                &shared_cache,
                            );
                            if let Some(ref m) = ind.metrics {
                                ind.fitness = compute_fitness(m, &config.ranking);
                                if config.ranking.complexity_alpha > 0.0 {
                                    let min_rules = (grammar.min_entry_rules + grammar.min_exit_rules) * 2;
                                    let n = ind.rule_count;
                                    let extra = n.saturating_sub(min_rules) as f64;
                                    ind.fitness /= 1.0 + config.ranking.complexity_alpha * extra;
                                }
                            }
                        }
                    });

                    island.stagnation_count = 0;
                    island.effective_mutation_prob = mutation_prob;
                    total_generated.fetch_add(island.population.len() - 1, Ordering::Relaxed);
                    // fingerprint_set updated incrementally inside the stagnation loop.
                }

                // Emit per-island stats
                let _ = tx_ref.send(BuilderProgressEvent::IslandStats(BuilderIslandStats {
                    island_id: island.id,
                    generation: gen,
                    population: island.population.len(),
                    best_fitness: island.best_fitness,
                }));
            }); // end par_iter_mut island loop

            if cancel_flag.load(Ordering::Relaxed) {
                return Err(AppError::BuilderCancelled);
            }

            // Migration
            if ga.migrate_every_n > 0 && gen > 0 && gen % ga.migrate_every_n == 0 {
                let mut rng = rand::thread_rng();
                ring_migration(&mut islands, ga.migration_rate, &mut rng);
                // fingerprint_sets updated incrementally inside ring_migration.
                let _ = tx.send(BuilderProgressEvent::Log(format!(
                    "Migration at generation {}",
                    gen,
                )));
            }

            // Emit progress stats
            let elapsed = start_time.elapsed().as_secs_f64();
            let gen_count = total_generated.load(Ordering::Relaxed);
            let acc_count = total_accepted.load(Ordering::Relaxed);
            let rej_count = total_rejected.load(Ordering::Relaxed);
            let db_count = {
                let db = databank.lock().unwrap_or_else(|e| e.into_inner());
                db.len()
            };

            let strategies_per_hour = if elapsed > 0.0 {
                gen_count as f64 / elapsed * 3600.0
            } else {
                0.0
            };
            let accepted_per_hour = if elapsed > 0.0 {
                acc_count as f64 / elapsed * 3600.0
            } else {
                0.0
            };
            let time_per_strategy_ms = if gen_count > 0 {
                elapsed * 1000.0 / gen_count as f64
            } else {
                0.0
            };

            let global_best = islands
                .iter()
                .map(|i| i.best_fitness)
                .fold(f64::NEG_INFINITY, f64::max);

            let _ = tx.send(BuilderProgressEvent::Stats(BuilderRuntimeStats {
                generated: gen_count,
                accepted: acc_count,
                rejected: rej_count,
                in_databank: db_count,
                start_time: Some(start_time.elapsed().as_secs_f64()),
                strategies_per_hour,
                accepted_per_hour,
                time_per_strategy_ms,
                generation: gen,
                island: islands.len(),
                best_fitness: global_best,
            }));

            // Check stop conditions
            match stop_when {
                BuilderStopWhen::Totally => {
                    if gen_count >= stop_totally_count {
                        let _ = tx.send(BuilderProgressEvent::Log(format!(
                            "Stop condition: {} strategies generated",
                            gen_count,
                        )));
                        break 'outer;
                    }
                }
                BuilderStopWhen::DatabankFull => {
                    if db_count >= max_databank_size {
                        let _ = tx.send(BuilderProgressEvent::Log(
                            "Stop condition: databank full".into(),
                        ));
                        break 'outer;
                    }
                }
                BuilderStopWhen::AfterTime => {
                    if elapsed as u64 >= stop_after_seconds && stop_after_seconds > 0 {
                        let _ = tx.send(BuilderProgressEvent::Log(
                            "Stop condition: time limit reached".into(),
                        ));
                        break 'outer;
                    }
                }
                BuilderStopWhen::Never => {
                    // Only stops via cancel or max_generations
                }
            }
        }

        // If not start_again_when_finished, break
        if !ga.start_again_when_finished {
            break 'outer;
        }

        let _ = tx.send(BuilderProgressEvent::Log(
            "Max generations reached, restarting...".into(),
        ));
    }

    // Return databank
    let db = databank.lock().unwrap_or_else(|e| e.into_inner());
    let result = db.clone();

    info!(
        "Builder finished: {} strategies generated, {} accepted, {} in databank, {:.1}s elapsed",
        total_generated.load(Ordering::Relaxed),
        total_accepted.load(Ordering::Relaxed),
        result.len(),
        start_time.elapsed().as_secs_f64(),
    );

    Ok(result)
}
