use serde::{Deserialize, Serialize};

fn bool_true() -> bool { true }
fn default_const_min_exp() -> f64 { -1.0 }
fn default_const_max_exp() -> f64 { 2.5 }
fn default_stagnation_window() -> usize { 10 }
fn default_atr_period() -> usize { 14 }
fn default_bloat_threshold() -> usize { 20 }
fn default_bloat_multiplier() -> f64 { 0.8 }
fn default_num_islands() -> usize { 1 }
fn default_migration_interval() -> usize { 10 }
fn default_migration_rate() -> f64 { 0.10 }
fn default_fresh_blood_interval() -> usize { 20 }
fn default_fresh_blood_pct() -> f64 { 0.10 }

/// Custom weights for the multi-objective scalar fitness function used by CMA-ES.
/// All values should be non-negative. Defaults match the hardcoded values previously used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalarWeights {
    #[serde(default = "default_w_sharpe")]
    pub sharpe: f64,
    #[serde(default = "default_w_profit_factor")]
    pub profit_factor: f64,
    #[serde(default = "default_w_temporal")]
    pub temporal_consistency: f64,
    #[serde(default = "default_w_drawdown")]
    pub neg_max_drawdown: f64,
    #[serde(default = "default_w_expectancy")]
    pub expectancy_ratio: f64,
}
fn default_w_sharpe() -> f64 { 0.30 }
fn default_w_profit_factor() -> f64 { 0.20 }
fn default_w_temporal() -> f64 { 0.25 }
fn default_w_drawdown() -> f64 { 0.15 }
fn default_w_expectancy() -> f64 { 0.10 }
impl Default for ScalarWeights {
    fn default() -> Self {
        Self {
            sharpe: 0.30,
            profit_factor: 0.20,
            temporal_consistency: 0.25,
            neg_max_drawdown: 0.15,
            expectancy_ratio: 0.10,
        }
    }
}

use crate::models::config::Timeframe;
use crate::models::result::BacktestMetrics;
use crate::models::strategy::{
    CloseTradesAt, IndicatorConfig, PositionSizing, StopLoss, TakeProfit, TrailingStop,
    TradingCosts, TradingHours, TradeDirection,
};

// ── Tree Op Types ─────────────────────────────────────────────────────────────

/// Binary arithmetic operation for SR formula trees.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BinaryOpType {
    Add,
    Sub,
    Mul,
    /// Protected division — returns 0.0 when |denominator| < 1e-10.
    ProtectedDiv,
}

/// Unary mathematical operation for SR formula trees.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum UnaryOpType {
    /// sqrt(|x|)
    Sqrt,
    /// |x|
    Abs,
    /// ln(|x| + 1e-10)
    Log,
    /// -x
    Neg,
}

// ── Tree Node ─────────────────────────────────────────────────────────────────

/// A node in a Symbolic Regression formula tree.
/// Maximum depth is controlled by [`SrConfig::max_depth`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SrNode {
    /// A numeric constant — ephemeral during evolution; CMA-ES refines values post-evolution.
    Constant(f64),
    /// An indicator value leaf.
    /// `buffer_index` selects the output slot: 0=primary, 1=secondary, 2=tertiary.
    IndicatorLeaf {
        config: IndicatorConfig,
        buffer_index: usize,
    },
    BinaryOp {
        op: BinaryOpType,
        left: Box<SrNode>,
        right: Box<SrNode>,
    },
    UnaryOp {
        op: UnaryOpType,
        child: Box<SrNode>,
    },
}

// ── Pool ──────────────────────────────────────────────────────────────────────

/// A single (indicator_config, buffer_index) combination available as a leaf in SR trees.
/// The user selects which indicators and buffers SR can discover.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolLeaf {
    pub config: IndicatorConfig,
    /// Buffer slot: 0=primary, 1=secondary (e.g. MACD signal), 2=tertiary (e.g. MACD histogram).
    pub buffer_index: usize,
    /// Optional period range — if set, this entry is expanded into multiple pool
    /// entries (one per period in `[period_min, period_max]` stepping by `period_step`)
    /// during SR builder initialization. Only applies to simple single-period indicators.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_min: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_max: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_step: Option<usize>,
}

// ── SR Strategy ───────────────────────────────────────────────────────────────

/// A Symbolic Regression strategy: 3 formula trees + trading configuration.
///
/// Entry logic:
///   - Long:  `evaluate(entry_long) > long_threshold` → open long
///   - Short: `evaluate(entry_short) < short_threshold` → open short
///   - Exit:  sign change in `evaluate(exit)` → close position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrStrategy {
    pub entry_long: SrNode,
    pub long_threshold: f64,
    pub entry_short: SrNode,
    pub short_threshold: f64,
    pub exit: SrNode,
    pub stop_loss: Option<StopLoss>,
    pub take_profit: Option<TakeProfit>,
    pub trailing_stop: Option<TrailingStop>,
    pub position_sizing: PositionSizing,
    pub trading_costs: TradingCosts,
    pub trade_direction: TradeDirection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trading_hours: Option<TradingHours>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_trades_at: Option<CloseTradesAt>,
    /// Max trade entries per calendar day. Propagated from SrConfig so the
    /// constraint is preserved when the strategy is saved and later re-backtested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_trades_per_day: Option<u32>,
    /// When false, the exit formula sign-change is ignored — positions close only
    /// via SL/TP/trailing-stop/time rules. Default: true.
    #[serde(default = "bool_true")]
    pub use_exit_formula: bool,
    /// Dead zone around zero for exit signal detection. Exit only triggers when the
    /// signal crosses from > dead_zone to < -dead_zone (or vice versa), preventing
    /// whipsaw exits from noisy indicators oscillating near zero. Default: 0.0.
    #[serde(default)]
    pub exit_dead_zone: f64,
    /// Close a trade forcibly after this many bars. `None` = no time limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bars_open: Option<usize>,
    /// Minimum bars to wait after closing a trade before a new entry is allowed.
    /// Prevents same-bar re-entry and whipsaw churn. `None` = no cooldown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_bars_between_trades: Option<usize>,
    /// Move SL to break-even (entry price) once profit ≥ SL distance. Default: false.
    #[serde(default)]
    pub move_sl_to_be: bool,
}

// ── ATR Range ─────────────────────────────────────────────────────────────────

/// Search range for ATR-based SL or TP parameters during SR evolution.
/// When set, the builder samples `atr_period` and the multiplier independently
/// for each individual instead of using fixed values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrAtrRange {
    pub period_min: usize,
    pub period_max: usize,
    pub mult_min: f64,
    pub mult_max: f64,
}

// ── SR Configuration ──────────────────────────────────────────────────────────

/// Full SR builder configuration sent from the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrConfig {
    /// The indicator/buffer leaf pool available to tree nodes.
    pub pool: Vec<PoolLeaf>,
    /// Number of individuals in the NSGA-II population. Default: 300.
    pub population_size: usize,
    /// Number of NSGA-II generations. Default: 50.
    pub generations: usize,
    /// Maximum tree depth. Default: 6.
    pub max_depth: usize,
    /// Minimum trade count for a neutral `trade_score`. Default: 20.
    pub min_trades: usize,
    /// How many Pareto-front individuals to refine with CMA-ES. Default: 10.
    pub cmaes_top_k: usize,
    /// Max function evaluations per CMA-ES individual refinement. Default: 200.
    pub cmaes_iterations: usize,
    pub crossover_rate: f64,
    pub mutation_rate: f64,
    // ── Backtest configuration ──────────────────────────────────────────────
    pub symbol_id: String,
    pub timeframe: Timeframe,
    pub start_date: String,
    pub end_date: String,
    /// Backtest precision for SL/TP resolution during evolution.
    /// Higher precision loads sub-bar data (M1 or tick) for intra-bar SL/TP checks.
    /// Default: SelectedTfOnly (bar-level OHLC only).
    #[serde(default)]
    pub precision: super::strategy::BacktestPrecision,
    pub initial_capital: f64,
    pub leverage: f64,
    pub position_sizing: PositionSizing,
    pub stop_loss: Option<StopLoss>,
    pub take_profit: Option<TakeProfit>,
    pub trailing_stop: Option<TrailingStop>,
    pub trading_costs: TradingCosts,
    pub trade_direction: TradeDirection,
    /// Stop Phase 1 when this many unique strategies are collected in the databank. Default: 50.
    pub databank_limit: usize,
    /// Max trade entries per calendar day. `None` = no limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_trades_per_day: Option<u32>,
    /// Only open new trades within this time window. `None` = no restriction.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trading_hours: Option<TradingHours>,
    /// Force-close any open position at this time each day. `None` = disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_trades_at: Option<CloseTradesAt>,

    // ── Initial strategy filters (Phase 1 databank acceptance) ─────────────
    /// Minimum Sharpe ratio to accept a strategy into the databank. `None` = no filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_min_sharpe: Option<f64>,
    /// Minimum Profit Factor to accept a strategy into the databank. `None` = no filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_min_profit_factor: Option<f64>,
    /// Maximum drawdown % allowed during Phase 1 acceptance. `None` = no filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_max_drawdown_pct: Option<f64>,

    // ── Final strategy filters (applied to Pareto front before returning to UI) ──
    /// Minimum Sharpe ratio for the final front. `None` = no filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_min_sharpe: Option<f64>,
    /// Minimum Profit Factor for the final front. `None` = no filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_min_profit_factor: Option<f64>,
    /// Minimum trade count for the final front. `None` = no filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_min_trades: Option<usize>,
    /// Maximum drawdown % allowed in the final front. `None` = no filter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_max_drawdown_pct: Option<f64>,
    /// When false, SR does not use the exit formula sign-change to close positions.
    /// Positions close only via SL/TP/trailing-stop/time rules. Default: true.
    #[serde(default = "bool_true")]
    pub use_exit_formula: bool,
    /// ATR period + multiplier search range for SL (only used when `stop_loss.sl_type == ATR`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sl_atr_range: Option<SrAtrRange>,
    /// ATR period + multiplier search range for TP (only used when `take_profit.tp_type == ATR`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tp_atr_range: Option<SrAtrRange>,

    // ── New tuning parameters (all have serde defaults for backward compat) ──

    /// Dead zone around zero for exit signal sign-change detection.
    /// Prevents whipsaw exits when indicators oscillate near zero. Default: 0.0.
    #[serde(default)]
    pub exit_dead_zone: f64,

    /// Custom scalar weights for the CMA-ES fitness function. Uses built-in defaults when `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scalar_weights: Option<ScalarWeights>,

    /// Log₁₀ lower bound for initial random constant magnitudes (default: -1.0 → 0.1).
    #[serde(default = "default_const_min_exp")]
    pub constant_min_exp: f64,

    /// Log₁₀ upper bound for initial random constant magnitudes (default: 2.5 → ~316).
    #[serde(default = "default_const_max_exp")]
    pub constant_max_exp: f64,

    /// Node count threshold above which CMA-ES applies a parsimony penalty (default: 20).
    #[serde(default = "default_bloat_threshold")]
    pub cmaes_bloat_threshold: usize,

    /// Multiplier applied to scalar fitness when node count exceeds `cmaes_bloat_threshold` (default: 0.8).
    #[serde(default = "default_bloat_multiplier")]
    pub cmaes_bloat_multiplier: f64,

    /// Optional RNG seed for reproducible runs. `None` = non-deterministic (default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,

    /// Number of consecutive generations with no improvement before emitting a stagnation warning (default: 10).
    #[serde(default = "default_stagnation_window")]
    pub stagnation_window: usize,

    /// ATR period used for SL/TP distance calculation (default: 14).
    #[serde(default = "default_atr_period")]
    pub atr_period: usize,

    /// Fraction of candles [0.0, 0.5) reserved as out-of-sample for final validation. `None` = no split.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oos_pct: Option<f64>,

    // ── Island model (parallel subpopulations) ──────────────────────────────
    /// Number of independent subpopulations (islands). Default: 1 (single population, same as before).
    /// With N > 1, the total population is split evenly across islands and each evolves
    /// independently. Every `migration_interval` generations the top `migration_rate`
    /// fraction migrates in a ring topology (island 0 → 1 → … → N-1 → 0).
    #[serde(default = "default_num_islands")]
    pub num_islands: usize,
    /// Generations between island migration events. Ignored when `num_islands == 1`. Default: 10.
    #[serde(default = "default_migration_interval")]
    pub migration_interval: usize,
    /// Fraction of each island that migrates to the next. Default: 0.10.
    #[serde(default = "default_migration_rate")]
    pub migration_rate: f64,

    // ── Fresh blood (diversity maintenance) ─────────────────────────────────
    /// Every this many generations, the weakest `fresh_blood_pct` fraction of each island
    /// is replaced with new random individuals to prevent premature convergence. Default: 20.
    #[serde(default = "default_fresh_blood_interval")]
    pub fresh_blood_interval: usize,
    /// Fraction of each island replaced with fresh random individuals. Default: 0.10.
    #[serde(default = "default_fresh_blood_pct")]
    pub fresh_blood_pct: f64,

    // ── Per-trade constraints ────────────────────────────────────────────────
    /// Force-close a trade after this many bars. Propagated into each `SrStrategy`. `None` = no limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bars_open: Option<usize>,
    /// Minimum bars to wait after closing a trade before opening a new one.
    /// Prevents same-bar re-entry and consecutive whipsaw trades. `None` = no cooldown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_bars_between_trades: Option<usize>,
    /// Move SL to break-even (entry price) once profit ≥ SL distance. Default: false.
    #[serde(default)]
    pub move_sl_to_be: bool,
}

// ── NSGA-II Objectives ────────────────────────────────────────────────────────

/// The 6 Pareto objectives — all expressed as "maximize".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrObjectives {
    pub sharpe: f64,
    pub profit_factor: f64,
    /// Mean / (std + 1) of per-period Sharpe proxies across 3 chronological thirds.
    /// High → strategy is profitable and consistent over time.
    pub temporal_consistency: f64,
    /// `-max_drawdown_pct`.
    pub neg_max_drawdown: f64,
    /// `expectancy / avg_loss_abs` — gain per unit of average loss.
    /// Captures edge quality independently of trade frequency.
    pub expectancy_ratio: f64,
    /// `-(total_node_count)` — negative complexity. Higher (less negative) = simpler formula.
    /// Acts as Pareto pressure against bloat during NSGA-II, replacing the hard penalty.
    #[serde(default)]
    pub neg_complexity: f64,
}

impl SrObjectives {
    /// Returns `true` if `self` Pareto-dominates `other`
    /// (better or equal on all objectives, strictly better on at least one).
    /// Returns `false` immediately if `self` has any NaN/Inf objective.
    pub fn dominates(&self, other: &Self) -> bool {
        if !self.is_valid() { return false; }
        let s = self.as_arr();
        let o = other.as_arr();
        s.iter().zip(o.iter()).all(|(a, b)| a >= b)
            && s.iter().zip(o.iter()).any(|(a, b)| a > b)
    }

    fn as_arr(&self) -> [f64; 6] {
        [
            self.sharpe,
            self.profit_factor,
            self.temporal_consistency,
            self.neg_max_drawdown,
            self.expectancy_ratio,
            self.neg_complexity,
        ]
    }

    /// Weighted scalar used by the CMA-ES constant-refinement phase.
    pub fn scalar(&self) -> f64 {
        self.scalar_with_weights(None)
    }

    /// Same as `scalar()` but uses custom weights when provided.
    /// `neg_complexity` is intentionally excluded from scalar — it acts only
    /// as a Pareto axis, not as a direct fitness component. This keeps CMA-ES
    /// focused on performance while NSGA-II handles the complexity trade-off.
    pub fn scalar_with_weights(&self, weights: Option<&ScalarWeights>) -> f64 {
        let w = weights.cloned().unwrap_or_default();
        self.sharpe * w.sharpe
            + self.profit_factor.min(10.0) * w.profit_factor
            + self.temporal_consistency * w.temporal_consistency
            + self.neg_max_drawdown * w.neg_max_drawdown
            + self.expectancy_ratio.clamp(-5.0, 5.0) * w.expectancy_ratio
    }

    /// Returns `true` if all objective values are finite (no NaN/Inf).
    pub fn is_valid(&self) -> bool {
        self.sharpe.is_finite()
            && self.profit_factor.is_finite()
            && self.temporal_consistency.is_finite()
            && self.neg_max_drawdown.is_finite()
            && self.expectancy_ratio.is_finite()
            && self.neg_complexity.is_finite()
    }
}

// ── Results ───────────────────────────────────────────────────────────────────

/// One item in the Pareto front returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SrFrontItem {
    pub rank: usize,
    pub crowding_distance: f64,
    pub objectives: SrObjectives,
    pub metrics: BacktestMetrics,
    pub formula_long: String,
    pub formula_short: String,
    pub formula_exit: String,
    /// Full strategy (serialized as JSON opaque blob for MQL5 export and detailed backtest).
    pub strategy: SrStrategy,
    /// Out-of-sample metrics (populated when `oos_pct` > 0 in SrConfig).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oos_metrics: Option<BacktestMetrics>,
}

// ── Progress Events ───────────────────────────────────────────────────────────

/// Progress event emitted during SR builder execution via Tauri event `sr-progress`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SrProgressEvent {
    Generation {
        gen: usize,
        /// Max generations safety limit (0 = unlimited — stops via `databank_limit` instead).
        total: usize,
        pareto_size: usize,
        best_sharpe: f64,
        databank_count: usize,
        databank_limit: usize,
        /// Total individuals evaluated across all generations.
        total_evaluated: usize,
        /// Strategies per second (individuals evaluated / elapsed seconds).
        strategies_per_sec: f64,
        /// Average tree depth across all 3 formula trees and all population members.
        avg_depth: f64,
        /// Number of unique formula keys in the current population (measures genetic diversity).
        unique_formula_count: usize,
        /// Mean scalar fitness across all evaluated individuals in the combined population.
        avg_fitness: f64,
        /// Standard deviation of scalar fitness (low = convergent population, high = diverse).
        fitness_std: f64,
        /// Mean crowding distance of rank-0 individuals (higher = more spread Pareto front).
        pareto_diversity: f64,
        /// Count of each operator type across all tree nodes in the population.
        /// Indices: [Add, Sub, Mul, ProtectedDiv, Sqrt, Abs, Log, Neg].
        operator_counts: [usize; 8],
    },
    /// Emitted when the best Sharpe on the Pareto front has not improved for `window` generations.
    Stagnation {
        gen: usize,
        window: usize,
        best_sharpe_in_window: f64,
    },
    /// Emitted once per individual during the CMA-ES constant-refinement phase.
    CmaesProgress {
        current: usize,
        total: usize,
    },
    CmaesComplete {
        improved_count: usize,
    },
    /// Emitted after NSGA-II completes and before CMA-ES refinement starts.
    /// Contains the full Pareto front from the generation phase — used to
    /// populate the "builder" databank on the frontend.
    NsgaDone {
        front: Vec<SrFrontItem>,
    },
    Done {
        front: Vec<SrFrontItem>,
    },
}
