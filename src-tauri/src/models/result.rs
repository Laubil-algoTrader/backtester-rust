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

/// All performance metrics from a backtest.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    // Stagnation & Ulcer
    pub stagnation_bars: usize,
    pub stagnation_time: String,
    pub ulcer_index_pct: f64,

    // Return / Drawdown ratio
    pub return_dd_ratio: f64,
}

/// Complete results of a backtest run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestResults {
    pub trades: Vec<TradeResult>,
    pub equity_curve: Vec<EquityPoint>,
    pub drawdown_curve: Vec<DrawdownPoint>,
    pub returns: Vec<f64>,
    pub metrics: BacktestMetrics,
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
    pub stagnation_bars: usize,
    pub ulcer_index_pct: f64,
    /// Out-of-Sample results for each OOS period (empty if no OOS configured).
    #[serde(default)]
    pub oos_results: Vec<OosResult>,
}
