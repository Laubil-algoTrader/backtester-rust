use serde::{Deserialize, Serialize};

use crate::models::config::Timeframe;
use crate::models::result::BacktestMetrics;
use crate::models::strategy::{
    IndicatorConfig, PositionSizing, StopLoss, TakeProfit, TrailingStop, TradingCosts,
    TradeDirection,
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
}

// ── NSGA-II Objectives ────────────────────────────────────────────────────────

/// The 4 Pareto objectives — all expressed as "maximize".
///
/// Removed from previous version:
/// - `trade_score`: replaced by `temporal_consistency` (captures trade frequency implicitly)
/// - `neg_complexity`: moved to a hard penalty in CMA-ES scalar (not a Pareto axis)
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
}

impl SrObjectives {
    /// Returns `true` if `self` Pareto-dominates `other`
    /// (better or equal on all objectives, strictly better on at least one).
    pub fn dominates(&self, other: &Self) -> bool {
        let s = self.as_arr();
        let o = other.as_arr();
        s.iter().zip(o.iter()).all(|(a, b)| a >= b)
            && s.iter().zip(o.iter()).any(|(a, b)| a > b)
    }

    fn as_arr(&self) -> [f64; 5] {
        [
            self.sharpe,
            self.profit_factor,
            self.temporal_consistency,
            self.neg_max_drawdown,
            self.expectancy_ratio,
        ]
    }

    /// Weighted scalar used by the CMA-ES constant-refinement phase.
    /// Complexity penalty is applied externally in cmaes.rs (not here).
    pub fn scalar(&self) -> f64 {
        self.sharpe * 0.30
            + self.profit_factor.min(10.0) * 0.20
            + self.temporal_consistency * 0.25
            + self.neg_max_drawdown * 0.15
            + self.expectancy_ratio.clamp(-5.0, 5.0) * 0.10
    }

    /// Returns `true` if all objective values are finite (no NaN/Inf).
    pub fn is_valid(&self) -> bool {
        self.sharpe.is_finite()
            && self.profit_factor.is_finite()
            && self.temporal_consistency.is_finite()
            && self.neg_max_drawdown.is_finite()
            && self.expectancy_ratio.is_finite()
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
    },
    /// Emitted once per individual during the CMA-ES constant-refinement phase.
    CmaesProgress {
        current: usize,
        total: usize,
    },
    CmaesComplete {
        improved_count: usize,
    },
    Done {
        front: Vec<SrFrontItem>,
    },
}
