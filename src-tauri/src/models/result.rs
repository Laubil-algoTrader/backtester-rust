use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::strategy::BacktestConfig;
use super::trade::TradeResult;

/// A point on the equity curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityPoint {
    pub timestamp: String,
    pub equity: f64,
}

/// A point on the drawdown curve.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawdownPoint {
    pub timestamp: String,
    pub drawdown_pct: f64,
}

/// Monthly return entry for year × month breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyReturn {
    pub year: i32,
    pub month: u32,
    pub return_pct: f64,
}

/// Configuration for a Monte Carlo simulation run.
///
/// When both `use_resampling` and `use_skip_trades` are enabled, each simulation
/// applies them sequentially: first bootstrap-resample the trades, then randomly
/// skip some from the resampled set. This is the same behaviour as StrategyQuant X
/// when both methods are checked simultaneously.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloConfig {
    pub n_simulations: usize,
    /// Apply bootstrap resampling (draw N trades with replacement from the historical pool).
    #[serde(default)]
    pub use_resampling: bool,
    /// Randomly skip each trade with `skip_probability`. Models missed executions.
    #[serde(default)]
    pub use_skip_trades: bool,
    /// Probability of skipping each trade (0.0–1.0). Only used when `use_skip_trades` is true.
    #[serde(default)]
    pub skip_probability: f64,
    /// Ruin threshold as a percentage loss of initial capital (0–100).
    /// A simulation is considered "ruined" when equity drops below
    /// `initial_capital * (1 - ruin_threshold_pct / 100)`.
    /// Default 20.0 means: losing 20 % of capital = ruin.
    #[serde(default = "default_ruin_threshold")]
    pub ruin_threshold_pct: f64,
}

fn default_ruin_threshold() -> f64 {
    20.0
}

/// One row in the confidence-level table returned by Monte Carlo simulation.
///
/// A confidence level of C % means: "there is only (100 − C)% chance that the
/// metric will be worse than the value shown here."  For profit-like metrics
/// (net_profit, ret_dd_ratio, expectancy) the displayed value is the pessimistic
/// tail; for drawdown it is the worst-case tail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloConfidenceRow {
    /// Confidence level in percent (e.g. 95.0).
    pub level: f64,
    /// Net profit at this confidence level ($).
    pub net_profit: f64,
    /// Maximum drawdown at this confidence level ($, absolute peak-to-trough).
    pub max_drawdown_abs: f64,
    /// Return / Drawdown ratio at this confidence level.
    pub ret_dd_ratio: f64,
    /// Average profit per executed trade ($) at this confidence level.
    pub expectancy: f64,
}

/// Results of a Monte Carlo simulation run on historical trades.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonteCarloResult {
    pub n_simulations: usize,

    /// Fraction of simulations (0–1) where equity fell below initial capital at any point.
    pub ruin_probability: f64,

    // ── Original strategy metrics (comparison row in table) ──────────────────
    pub original_net_profit: f64,
    pub original_max_drawdown_abs: f64,
    pub original_ret_dd_ratio: f64,
    pub original_expectancy: f64,
    pub original_return_pct: f64,
    pub original_max_drawdown_pct: f64,

    /// Confidence table rows at levels [50, 60, 70, 80, 90, 92, 95, 97, 98].
    pub confidence_table: Vec<MonteCarloConfidenceRow>,

    /// Sampled simulation equity curves for visualization (max 200, each ≤300 points).
    pub sim_equity_curves: Vec<Vec<f64>>,
    /// Original (historical) equity curve, downsampled to the same resolution.
    pub original_equity_curve: Vec<f64>,
}

/// All performance metrics from a backtest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BacktestMetrics {
    // Returns
    pub final_capital: f64,
    pub total_return_pct: f64,
    pub annualized_return_pct: f64,
    pub monthly_return_avg_pct: f64,

    // Risk-adjusted
    pub sharpe_ratio: f64,
    pub sortino_ratio: f64,
    pub calmar_ratio: f64,

    // Drawdown
    pub max_drawdown_pct: f64,
    pub max_drawdown_abs: f64,
    pub max_drawdown_duration_bars: usize,
    pub max_drawdown_duration_time: String,
    pub avg_drawdown_pct: f64,
    pub recovery_factor: f64,

    // Trades
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub breakeven_trades: usize,
    pub win_rate_pct: f64,

    // P&L
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub net_profit: f64,
    pub profit_factor: f64,
    pub avg_trade: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub largest_win: f64,
    pub largest_loss: f64,
    pub expectancy: f64,

    // Consistency
    pub max_consecutive_wins: usize,
    pub max_consecutive_losses: usize,
    pub avg_consecutive_wins: f64,
    pub avg_consecutive_losses: f64,

    // Time
    pub avg_trade_duration: String,
    pub avg_bars_in_trade: f64,
    pub avg_winner_duration: String,
    pub avg_loser_duration: String,

    // Risk
    pub mae_avg: f64,
    pub mae_max: f64,
    pub mfe_avg: f64,
    pub mfe_max: f64,

    // Costs breakdown
    pub total_swap_charged: f64,
    pub total_commission_charged: f64,

    // Stagnation & Ulcer
    pub stagnation_bars: usize,
    pub stagnation_time: String,
    pub ulcer_index_pct: f64,

    // Return / Drawdown ratio
    pub return_dd_ratio: f64,

    // Additional metrics (P4.4)
    pub k_ratio: f64,
    pub omega_ratio: f64,
    pub monthly_returns: Vec<MonthlyReturn>,

    /// Temporal consistency: mean / (std + 1) of per-period Sharpe proxies.
    /// Computed by splitting trades into 3 chronological thirds.
    /// High value → strategy is profitable and stable across different time windows.
    #[serde(default)]
    pub temporal_consistency: f64,
}

/// Complete results of a backtest run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResults {
    pub trades: Vec<TradeResult>,
    pub equity_curve: Vec<EquityPoint>,
    pub drawdown_curve: Vec<DrawdownPoint>,
    pub returns: Vec<f64>,
    pub metrics: BacktestMetrics,
    /// The backtest configuration used to generate these results (capital, timeframe, dates, etc.)
    pub backtest_config: BacktestConfig,
    /// Metrics computed from long trades only. None when there are no long trades.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_metrics: Option<BacktestMetrics>,
    /// Metrics computed from short trades only. None when there are no short trades.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub short_metrics: Option<BacktestMetrics>,
    /// Warnings about backtest configuration that may affect accuracy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

// ══════════════════════════════════════════════════════════════
// Optimization types
// ══════════════════════════════════════════════════════════════

/// Optimization method.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OptimizationMethod {
    GridSearch,
    GeneticAlgorithm,
}

/// Objective function for optimization.
/// "Maximize" objectives: higher is better.
/// "Minimize" objectives (MinStagnation, MinUlcerIndex): lower is better — internally negated.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ObjectiveFunction {
    TotalProfit,
    SharpeRatio,
    ProfitFactor,
    WinRate,
    ReturnDdRatio,
    MinStagnation,
    MinUlcerIndex,
}

/// A parameter range to optimize over.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterRange {
    pub rule_index: i32,
    pub param_name: String,
    pub display_name: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// Which operand of the rule contains the indicator: "left" or "right".
    pub operand_side: String,
    /// Source of the parameter: "indicator", "stop_loss", "take_profit", "trailing_stop".
    #[serde(default = "default_param_source")]
    pub param_source: String,
}

fn default_param_source() -> String {
    "indicator".into()
}

/// Configuration for the genetic algorithm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneticAlgorithmConfig {
    pub population_size: usize,
    pub generations: usize,
    pub mutation_rate: f64,
    pub crossover_rate: f64,
    /// Stop early if the best fitness has not improved for this many consecutive generations.
    /// `None` means no early stopping (run all generations).
    #[serde(default)]
    pub patience: Option<usize>,
}

/// A date range for Out-of-Sample testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OosPeriod {
    pub label: String,
    pub start_date: String,
    pub end_date: String,
}

/// Results of evaluating a parameter set on an OOS period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OosResult {
    pub label: String,
    pub total_return_pct: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown_pct: f64,
    pub profit_factor: f64,
    pub total_trades: usize,
}

/// Full optimization configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationConfig {
    pub method: OptimizationMethod,
    pub parameter_ranges: Vec<ParameterRange>,
    /// One or more objectives. First is primary (used for GA fitness).
    pub objectives: Vec<ObjectiveFunction>,
    pub backtest_config: BacktestConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ga_config: Option<GeneticAlgorithmConfig>,
    /// Out-of-Sample periods for validation (optional).
    #[serde(default)]
    pub oos_periods: Vec<OosPeriod>,
}

/// A single result from an optimization run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
    pub params: HashMap<String, f64>,
    /// Primary objective value (first objective).
    pub objective_value: f64,
    /// Composite score when multiple objectives are used (normalized average).
    pub composite_score: f64,
    pub total_return_pct: f64,
    pub sharpe_ratio: f64,
    pub max_drawdown_pct: f64,
    pub total_trades: usize,
    pub profit_factor: f64,
    pub return_dd_ratio: f64,
    pub win_rate_pct: f64,
    pub stagnation_bars: usize,
    pub ulcer_index_pct: f64,
    /// Out-of-Sample results for each OOS period (empty if no OOS configured).
    #[serde(default)]
    pub oos_results: Vec<OosResult>,
    /// Downsampled equity curve for sparkline visualization (max ~60 points).
    #[serde(default)]
    pub equity_curve: Vec<EquityPoint>,
}

// ══════════════════════════════════════════════════════════════
// Walk-Forward Analysis types
// ══════════════════════════════════════════════════════════════

/// Configuration for Walk-Forward Analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardConfig {
    /// Number of sequential windows to divide the data into.
    pub num_windows: usize,
    /// Fraction of each window used for in-sample optimization (0.1–0.9).
    pub in_sample_pct: f64,
    /// Optimization configuration used on the in-sample portion of each window.
    pub optimization_config: OptimizationConfig,
}

/// Per-window results from a Walk-Forward Analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardWindowResult {
    pub window_index: usize,
    pub in_sample_start: String,
    pub in_sample_end: String,
    pub out_of_sample_start: String,
    pub out_of_sample_end: String,
    pub best_params: HashMap<String, f64>,
    pub in_sample_metrics: BacktestMetrics,
    pub out_of_sample_metrics: BacktestMetrics,
}

/// Aggregated results of a complete Walk-Forward Analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkForwardResult {
    pub windows: Vec<WalkForwardWindowResult>,
    pub combined_out_of_sample_metrics: BacktestMetrics,
    /// OOS net profit / IS net profit. Values ≥ 0.5 suggest a robust strategy.
    pub efficiency_ratio: f64,
}
