use serde::{Deserialize, Serialize};

use super::config::Timeframe;
use super::strategy::{BacktestPrecision, IndicatorType};

// ══════════════════════════════════════════════════════════════
// Enums
// ══════════════════════════════════════════════════════════════

/// Direction constraint for strategy generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderDirection {
    LongOnly,
    ShortOnly,
    BothSymmetric,
    BothAsymmetric,
}

/// Build algorithm to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderBuildMode {
    GeneticEvolution,
}

/// Stop-loss type used during strategy generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderSLType {
    Atr,
    Pips,
    Percentage,
}

/// Take-profit type used during strategy generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderTPType {
    Atr,
    Pips,
    Rr,
}

/// Which data sample triggers stagnation restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderStagnationSample {
    InSample,
    OutOfSample,
    Full,
}

/// Money-management method for the builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderMMMethod {
    FixedSize,
    RiskFixedBalance,
    RiskFixedAccount,
    FixedAmount,
    CryptoByPrice,
    StocksByPrice,
    SimpleMartingale,
}

/// Which data segment to compute fitness on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderFitnessSource {
    MainData,
    InSample,
    OutOfSample,
    Full,
}

/// Metric used to compute the primary fitness score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderComputeFrom {
    NetProfit,
    ReturnDd,
    RExpectancy,
    AnnualMaxDd,
    WeightedFitness,
}

/// When to stop the builder run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderStopWhen {
    Never,
    Totally,
    DatabankFull,
    AfterTime,
}

/// Which order types to close at session boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderOrderTypesToClose {
    All,
    Market,
    Stop,
    Limit,
}

/// Data-range part type: in-sample or out-of-sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderDataRangePartType {
    Is,
    Oos,
}

/// Comparison operator for filter conditions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BuilderFilterOperator {
    #[serde(rename = ">=")]
    Gte,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = "<=")]
    Lte,
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = "==")]
    Eq,
}

/// Optimization direction for a weighted criterion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderCriterionType {
    Minimize,
    Maximize,
}

/// Order type used in builder building blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderOrderType {
    Stop,
    Limit,
    Market,
}

/// Exit type used in builder building blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuilderExitType {
    ExitAfterBars,
    MoveSlBe,
    ProfitTarget,
    StopLoss,
    TrailingStop,
    ExitRule,
}

// ══════════════════════════════════════════════════════════════
// Sub-config structs
// ══════════════════════════════════════════════════════════════

/// A single filter condition (e.g. "Net Profit >= 100").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderFilterCondition {
    pub id: String,
    pub left_value: String,
    pub operator: BuilderFilterOperator,
    pub right_value: f64,
}

/// A weighted criterion for multi-objective fitness.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderWeightedCriterion {
    pub id: String,
    pub criterium: String,
    #[serde(rename = "type")]
    pub criterion_type: BuilderCriterionType,
    pub weight: f64,
    pub target: f64,
}

/// A segment of the data range (in-sample or out-of-sample).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderDataRangePart {
    pub id: String,
    #[serde(rename = "type")]
    pub part_type: BuilderDataRangePartType,
    pub percent: f64,
}

/// An indicator toggle in the building-blocks palette.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderIndicatorBlock {
    pub indicator_type: IndicatorType,
    pub enabled: bool,
    pub weight: f64,
    /// Per-indicator period range override. None = use the global range from WhatToBuildConfig.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_min: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_max: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_step: Option<usize>,
}

/// An order-type toggle in the building-blocks palette.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderOrderTypeBlock {
    pub order_type: BuilderOrderType,
    pub enabled: bool,
    pub weight: f64,
}

/// An exit-type toggle in the building-blocks palette.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderExitTypeBlock {
    pub exit_type: BuilderExitType,
    pub enabled: bool,
    pub required: bool,
}

/// An order-price indicator block in the building-blocks palette.
/// The primary output of the indicator is used as the price offset for Stop/Limit orders.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderOrderPriceBlock {
    pub indicator_type: IndicatorType,
    pub enabled: bool,
    pub weight: f64,
    pub multiplier_min: f64,
    pub multiplier_max: f64,
    pub multiplier_step: f64,
}

// ══════════════════════════════════════════════════════════════
// Settings-tab configs
// ══════════════════════════════════════════════════════════════

/// "What to Build" tab — strategy shape constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderWhatToBuild {
    pub direction: BuilderDirection,
    pub build_mode: BuilderBuildMode,
    pub min_entry_rules: usize,
    pub max_entry_rules: usize,
    pub min_exit_rules: usize,
    pub max_exit_rules: usize,
    pub max_lookback: usize,
    pub indicator_period_min: usize,
    pub indicator_period_max: usize,
    pub indicator_period_step: usize,
    pub sl_required: bool,
    pub sl_type: BuilderSLType,
    pub sl_coeff_min: f64,
    pub sl_coeff_max: f64,
    pub sl_coeff_step: f64,
    pub sl_atr_period_min: usize,
    pub sl_atr_period_max: usize,
    pub sl_atr_period_step: usize,
    pub tp_required: bool,
    pub tp_type: BuilderTPType,
    pub tp_coeff_min: f64,
    pub tp_coeff_max: f64,
    pub tp_coeff_step: f64,
    pub tp_atr_period_min: usize,
    pub tp_atr_period_max: usize,
    pub tp_atr_period_step: usize,
}

fn default_fitness_sharing_alpha() -> f64 { 1.0 }
fn default_meta_learning_top_pct() -> f64 { 0.25 }

/// Distance metric used for behavioral niching (fitness sharing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehavioralNichingMode {
    /// Jaccard distance on the set of indicator types used (fast, always available).
    Structural,
    /// 1 − Pearson correlation of the downsampled equity curves.
    /// Penalizes strategies that profit and lose at the same time.
    EquityCurve,
    /// Jaccard distance on trade entry timestamps.
    /// Penalizes strategies that enter the market at the same bars.
    TradeOverlap,
    /// Average of EquityCurve and TradeOverlap distances.
    Combined,
}

impl Default for BehavioralNichingMode {
    fn default() -> Self { Self::Structural }
}

/// Genetic-algorithm tuning knobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderGeneticOptions {
    pub max_generations: usize,
    pub population_per_island: usize,
    pub crossover_probability: f64,
    pub mutation_probability: f64,
    pub islands: usize,
    pub migrate_every_n: usize,
    pub migration_rate: f64,
    pub initial_population_size: usize,
    pub use_from_databank: bool,
    pub decimation_coefficient: f64,
    pub initial_filters: Vec<BuilderFilterCondition>,
    pub fresh_blood_detect_duplicates: bool,
    pub fresh_blood_replace_percent: f64,
    pub fresh_blood_replace_every: usize,
    pub show_last_generation: bool,
    pub start_again_when_finished: bool,
    pub restart_on_stagnation: bool,
    pub stagnation_sample: BuilderStagnationSample,
    pub stagnation_generations: usize,
    /// Fraction of fitness data to use for pre-filter quick backtest (0.0 = disabled).
    /// A strategy must produce at least `prefilter_min_trades` in this window to proceed.
    #[serde(default)]
    pub prefilter_window_pct: f64,
    /// Minimum number of trades required in the pre-filter window.
    #[serde(default)]
    pub prefilter_min_trades: usize,
    /// Enable exploration→exploitation phase shift based on generation progress.
    #[serde(default)]
    pub phase_based_adaptation: bool,
    /// Niche radius for fitness sharing (0.0 = disabled). Strategies closer than sigma
    /// in indicator-type Jaccard distance get their fitness penalized.
    #[serde(default)]
    pub fitness_sharing_sigma: f64,
    /// Shape exponent for fitness sharing function (default 1.0 = linear decay).
    #[serde(default = "default_fitness_sharing_alpha")]
    pub fitness_sharing_alpha: f64,
    /// Distance metric used when fitness_sharing_sigma > 0.
    /// Defaults to `Structural` (Jaccard on indicator types).
    #[serde(default)]
    pub niching_mode: BehavioralNichingMode,
    /// EMA learning rate for per-island grammar adaptation (0.0 = disabled).
    /// After each generation the top `meta_learning_top_pct` individuals are
    /// observed; indicator weights and period means are updated toward what
    /// high-fitness strategies actually use. Good starting value: 0.1.
    #[serde(default)]
    pub meta_learning_rate: f64,
    /// Fraction of the population (top by fitness) observed each generation
    /// for meta-learning. Default: 0.25 (top 25%).
    #[serde(default = "default_meta_learning_top_pct")]
    pub meta_learning_top_pct: f64,
}

/// Data source and range configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderDataConfig {
    pub symbol_id: Option<String>,
    pub timeframe: Timeframe,
    pub start_date: String,
    pub end_date: String,
    pub precision: BacktestPrecision,
    pub spread_pips: f64,
    pub slippage_pips: f64,
    pub min_distance_pips: f64,
    pub data_range_parts: Vec<BuilderDataRangePart>,
}

/// Session/time-based trading restrictions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderTradingOptions {
    pub dont_trade_weekends: bool,
    pub friday_close_time: String,
    pub sunday_open_time: String,
    pub exit_at_end_of_day: bool,
    pub end_of_day_exit_time: String,
    pub exit_on_friday: bool,
    pub friday_exit_time: String,
    pub limit_time_range: bool,
    pub time_range_from: String,
    pub time_range_to: String,
    pub exit_at_end_of_range: bool,
    pub order_types_to_close: BuilderOrderTypesToClose,
    pub max_distance_from_market: bool,
    pub max_distance_percent: f64,
    pub max_trades_per_day: usize,
    #[serde(rename = "minimumSL")]
    pub minimum_sl: f64,
    #[serde(rename = "maximumSL")]
    pub maximum_sl: f64,
    #[serde(rename = "minimumPT")]
    pub minimum_pt: f64,
    #[serde(rename = "maximumPT")]
    pub maximum_pt: f64,
}

/// Available building blocks for strategy generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderBuildingBlocks {
    pub indicators: Vec<BuilderIndicatorBlock>,
    pub order_types: Vec<BuilderOrderTypeBlock>,
    pub exit_types: Vec<BuilderExitTypeBlock>,
    pub order_price_indicators: Vec<BuilderOrderPriceBlock>,
    /// Which signal-bar price to use as base for Stop orders ("high", "low", "close", "open").
    pub order_price_base_stop: String,
    /// Which signal-bar price to use as base for Limit orders ("high", "low", "close", "open").
    pub order_price_base_limit: String,
}

/// Money-management settings for generated strategies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderMoneyManagement {
    pub initial_capital: f64,
    pub method: BuilderMMMethod,
    pub risked_money: f64,
    pub size_decimals: usize,
    #[serde(rename = "sizeIfNoMM")]
    pub size_if_no_mm: f64,
    pub maximum_lots: f64,
}

/// Ranking, stopping criteria, and fitness configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderRanking {
    pub max_strategies_to_store: usize,
    pub stop_when: BuilderStopWhen,
    pub stop_totally_count: usize,
    pub stop_after_days: usize,
    pub stop_after_hours: usize,
    pub stop_after_minutes: usize,
    pub fitness_source: BuilderFitnessSource,
    pub compute_from: BuilderComputeFrom,
    pub weighted_criteria: Vec<BuilderWeightedCriterion>,
    pub custom_filters: Vec<BuilderFilterCondition>,
    pub dismiss_similar: bool,
    /// Complexity penalty coefficient (0.0 = disabled).
    /// fitness *= 1 / (1 + alpha * max(0, total_rules - grammar_min_rules))
    #[serde(default)]
    pub complexity_alpha: f64,
}

/// Cross-check / robustness-test toggles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderCrossChecks {
    pub disable_all: bool,
    pub what_if: bool,
    pub monte_carlo: bool,
    pub higher_precision: bool,
    pub additional_markets: bool,
    pub monte_carlo_retest: bool,
    pub sequential_opt: bool,
    pub walk_forward: bool,
    pub walk_forward_matrix: bool,
}

// ══════════════════════════════════════════════════════════════
// Top-level config
// ══════════════════════════════════════════════════════════════

/// Complete builder configuration sent from the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderConfig {
    pub what_to_build: BuilderWhatToBuild,
    pub genetic_options: BuilderGeneticOptions,
    pub data_config: BuilderDataConfig,
    pub trading_options: BuilderTradingOptions,
    pub building_blocks: BuilderBuildingBlocks,
    pub money_management: BuilderMoneyManagement,
    pub ranking: BuilderRanking,
    pub cross_checks: BuilderCrossChecks,
}

// ══════════════════════════════════════════════════════════════
// Runtime outputs
// ══════════════════════════════════════════════════════════════

/// A strategy saved to the results databank by the builder.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderSavedStrategy {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub fitness: f64,
    pub symbol_id: Option<String>,
    pub symbol_name: String,
    pub timeframe: Timeframe,
    // IS metrics
    pub net_profit: f64,
    pub mini_equity_curve: Vec<f64>,
    pub trades: usize,
    pub profit_factor: f64,
    pub sharpe_ratio: f64,
    pub r_expectancy: f64,
    pub annual_return_pct: f64,
    pub max_drawdown_abs: f64,
    pub win_loss_ratio: f64,
    #[serde(rename = "retDDRatio")]
    pub ret_dd_ratio: f64,
    #[serde(rename = "cagrMaxDDPct")]
    pub cagr_max_dd_pct: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub avg_bars_win: f64,
    pub strategy_json: String,
    /// Hash of the strategy structure for fast duplicate detection (no JSON re-parse needed).
    #[serde(default)]
    pub fingerprint: u64,
    // OOS metrics (None when no OOS split is configured or OOS backtest failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oos_net_profit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oos_trades: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oos_profit_factor: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oos_sharpe_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oos_max_drawdown_abs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oos_win_rate_pct: Option<f64>,
}

/// Per-island stats emitted after each generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderIslandStats {
    pub island_id: usize,
    pub generation: usize,
    pub population: usize,
    pub best_fitness: f64,
}

/// Live stats emitted by the builder engine during a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderRuntimeStats {
    pub generated: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub in_databank: usize,
    pub start_time: Option<f64>,
    pub strategies_per_hour: f64,
    pub accepted_per_hour: f64,
    pub time_per_strategy_ms: f64,
    pub generation: usize,
    pub island: usize,
    pub best_fitness: f64,
}
