use serde::{Deserialize, Serialize};

use super::config::Timeframe;

// ── Indicators ──

/// All supported indicator types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IndicatorType {
    SMA,
    EMA,
    RSI,
    MACD,
    BollingerBands,
    ATR,
    Stochastic,
    ADX,
    CCI,
    ROC,
    WilliamsR,
    ParabolicSAR,
    // New indicators
    Aroon,
    AwesomeOscillator,
    BarRange,
    BiggestRange,
    HighestInRange,
    LowestInRange,
    SmallestRange,
    BearsPower,
    BullsPower,
    DeMarker,
    Fibonacci,
    Fractal,
    GannHiLo,
    HeikenAshi,
    HullMA,
    Ichimoku,
    KeltnerChannel,
    LaguerreRSI,
    LinearRegression,
    Momentum,
    SuperTrend,
    TrueRange,
    StdDev,
    Reflex,
    Pivots,
    UlcerIndex,
    Vortex,
}

/// Parameters for indicator calculation. Each indicator uses the fields relevant to it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IndicatorParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fast_period: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slow_period: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_period: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub std_dev: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k_period: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d_period: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceleration_factor: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_factor: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gamma: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiplier: Option<f64>,
}

/// Configuration for a single indicator instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorConfig {
    pub indicator_type: IndicatorType,
    pub params: IndicatorParams,
    /// For multi-output indicators (e.g. "upper"/"middle"/"lower" for Bollinger Bands).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_field: Option<String>,
    /// Pre-computed cache key hash — set once by `init_strategy_hashes()` before the bar loop.
    /// Not serialized; recomputed after deserialization via `init_strategy_hashes`.
    #[serde(skip, default)]
    pub cached_hash: u64,
}

impl IndicatorConfig {
    /// Fast allocation-free cache key using a u64 hash.
    /// Matches the same field set as `cache_key()`. Use this for `IndicatorCache` lookups.
    pub fn cache_key_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        let mut h = DefaultHasher::new();
        self.indicator_type.hash(&mut h);
        self.params.period.hash(&mut h);
        self.params.fast_period.hash(&mut h);
        self.params.slow_period.hash(&mut h);
        self.params.signal_period.hash(&mut h);
        self.params.k_period.hash(&mut h);
        self.params.d_period.hash(&mut h);
        // Float params: round to match precision used in cache_key()
        self.params.std_dev.map(|v| (v * 100.0).round() as i64).hash(&mut h);
        self.params.acceleration_factor.map(|v| (v * 10000.0).round() as i64).hash(&mut h);
        self.params.maximum_factor.map(|v| (v * 10000.0).round() as i64).hash(&mut h);
        self.params.gamma.map(|v| (v * 10000.0).round() as i64).hash(&mut h);
        self.params.multiplier.map(|v| (v * 100.0).round() as i64).hash(&mut h);
        h.finish()
    }

    /// Generate a unique cache key for this indicator configuration.
    /// Used for StreamingStateMap lookups (String-keyed). Prefer `cache_key_hash()` for IndicatorCache.
    pub fn cache_key(&self) -> String {
        let mut key = format!("{:?}", self.indicator_type);
        if let Some(p) = self.params.period {
            key.push_str(&format!("_p{}", p));
        }
        if let Some(p) = self.params.fast_period {
            key.push_str(&format!("_fp{}", p));
        }
        if let Some(p) = self.params.slow_period {
            key.push_str(&format!("_sp{}", p));
        }
        if let Some(p) = self.params.signal_period {
            key.push_str(&format!("_sig{}", p));
        }
        if let Some(s) = self.params.std_dev {
            key.push_str(&format!("_sd{:.2}", s));
        }
        if let Some(p) = self.params.k_period {
            key.push_str(&format!("_kp{}", p));
        }
        if let Some(p) = self.params.d_period {
            key.push_str(&format!("_dp{}", p));
        }
        if let Some(a) = self.params.acceleration_factor {
            key.push_str(&format!("_af{:.4}", a));
        }
        if let Some(m) = self.params.maximum_factor {
            key.push_str(&format!("_mf{:.4}", m));
        }
        if let Some(g) = self.params.gamma {
            key.push_str(&format!("_g{:.4}", g));
        }
        if let Some(m) = self.params.multiplier {
            key.push_str(&format!("_mul{:.2}", m));
        }
        key
    }
}

// ── Rules ──

/// Comparison operators for rule evaluation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Comparator {
    GreaterThan,
    LessThan,
    GreaterOrEqual,
    LessOrEqual,
    Equal,
    CrossAbove,
    CrossBelow,
}

/// Logical connectors between rules.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum LogicalOperator {
    #[serde(rename = "AND")]
    And,
    #[serde(rename = "OR")]
    Or,
}

/// Type discriminator for operands.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum OperandType {
    Indicator,
    Price,
    Constant,
    BarTime,
    CandlePattern,
    /// Arithmetic combination of two simple (non-Compound) operands: left OP right.
    Compound,
}

/// Arithmetic operator for compound operands.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ArithmeticOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// Time/bar fields for the BarTime operand type.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TimeField {
    CurrentBar,
    BarTimeValue,
    BarHour,
    BarMinute,
    BarDayOfWeek,
    CurrentTime,
    CurrentHour,
    CurrentMinute,
    CurrentDayOfWeek,
    CurrentMonth,
}

/// Which price field to use as an operand.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PriceField {
    Open,
    High,
    Low,
    Close,
    DailyOpen,
    DailyHigh,
    DailyLow,
    DailyClose,
}

/// Candle pattern types for the CandlePattern operand.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CandlePatternType {
    Doji,
    Hammer,
    ShootingStar,
    BearishEngulfing,
    BullishEngulfing,
    DarkCloud,
    PiercingLine,
}

/// One side of a rule comparison. Flat struct matching the TypeScript interface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operand {
    pub operand_type: OperandType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indicator: Option<IndicatorConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_field: Option<PriceField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constant_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_field: Option<TimeField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candle_pattern: Option<CandlePatternType>,
    /// Look back N bars for the operand value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
    // ── Compound operand fields (only used when operand_type == Compound) ──
    /// Left sub-operand. Must be a non-Compound operand (depth = 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compound_left: Option<Box<Operand>>,
    /// Arithmetic operation between left and right sub-operands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compound_op: Option<ArithmeticOp>,
    /// Right sub-operand. Must be a non-Compound operand (depth = 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compound_right: Option<Box<Operand>>,
}

/// A group of rules combined with a shared logical operator.
/// Groups are joined to adjacent groups by their `join` operator.
/// Enables expressions like (A AND B) OR (C AND D) using 2 groups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleGroup {
    pub id: String,
    /// Rules within this group, combined using `internal`.
    pub rules: Vec<Rule>,
    /// How rules within this group are combined (AND = all must pass, OR = any must pass).
    pub internal: LogicalOperator,
    /// How this group connects to the NEXT group (None on the last group).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<LogicalOperator>,
}

/// A single rule: left [comparator] right.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub left_operand: Operand,
    pub comparator: Comparator,
    pub right_operand: Operand,
    /// Logical connector to the next rule in the list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_operator: Option<LogicalOperator>,
}

// ── Position Sizing ──

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PositionSizingType {
    FixedLots,
    FixedAmount,
    PercentEquity,
    RiskBased,
    /// Anti-martingale: reduce lot size proportionally after consecutive losses.
    /// Base size is risk-based (like RiskBased). After each consecutive loss the
    /// effective size is multiplied by `decrease_factor^n_losses` (< 1 = reduction).
    AntiMartingale,
}

fn default_decrease_factor() -> f64 {
    0.9
}

fn default_one() -> f64 { 1.0 }
fn default_price_high() -> PriceField { PriceField::High }
fn default_price_low() -> PriceField { PriceField::Low }

/// Configuration for computing a Stop/Limit order target price using an indicator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderPriceConfig {
    /// The indicator whose primary output value is used as the price offset.
    pub indicator: IndicatorConfig,
    /// Scale factor applied to the indicator value before using as offset.
    #[serde(default = "default_one")]
    pub multiplier: f64,
    /// Which signal-bar price to use as base when placing a Stop order.
    #[serde(default = "default_price_high")]
    pub base_price_stop: PriceField,
    /// Which signal-bar price to use as base when placing a Limit order.
    #[serde(default = "default_price_low")]
    pub base_price_limit: PriceField,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionSizing {
    pub sizing_type: PositionSizingType,
    pub value: f64,
    /// Used by `AntiMartingale` only. Multiplier per consecutive losing trade.
    /// Must be in (0.0, 1.0]; e.g. 0.9 = reduce by 10% per loss.
    #[serde(default = "default_decrease_factor")]
    pub decrease_factor: f64,
}

// ── Stop Loss ──

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum StopLossType {
    Pips,
    Percentage,
    ATR,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopLoss {
    pub sl_type: StopLossType,
    pub value: f64,
    /// ATR period used when sl_type is ATR.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atr_period: Option<usize>,
}

// ── Take Profit ──

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TakeProfitType {
    Pips,
    RiskReward,
    ATR,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TakeProfit {
    pub tp_type: TakeProfitType,
    pub value: f64,
    /// ATR period used when tp_type is ATR.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atr_period: Option<usize>,
}

// ── Trailing Stop ──

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TrailingStopType {
    ATR,
    RiskReward,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrailingStop {
    pub ts_type: TrailingStopType,
    pub value: f64,
    /// ATR period used when ts_type is ATR.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atr_period: Option<usize>,
}

// ── Trading Costs ──

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CommissionType {
    Percentage,
    FixedPerLot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingCosts {
    pub spread_pips: f64,
    pub commission_type: CommissionType,
    pub commission_value: f64,
    pub slippage_pips: f64,
    pub slippage_random: bool,
    /// Skip entry if current spread exceeds this value (pips). None = no filter.
    /// In bar-mode uses fixed spread; in tick-mode uses real bid-ask spread.
    #[serde(default)]
    pub max_spread_pips: Option<f64>,
}

// ── Trade Direction ──

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TradeDirection {
    Long,
    Short,
    Both,
}

// ── Trading Hours ──

/// Time window during which the strategy is allowed to open new trades.
/// Supports ranges that cross midnight (e.g. 22:00 → 06:00).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingHours {
    pub start_hour: u8,
    pub start_minute: u8,
    pub end_hour: u8,
    pub end_minute: u8,
}

// ── Close Trades At ──

/// Force-close any open position at a specific time each day.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseTradesAt {
    pub hour: u8,
    pub minute: u8,
}

// ── Order Type ──

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    #[default]
    Market,
    Limit,
    Stop,
}

// ── Strategy ──

/// A complete trading strategy with direction-specific entry/exit rules and configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strategy {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    /// Entry rules for long positions. Alias "entry_rules" for backward compat.
    #[serde(alias = "entry_rules")]
    pub long_entry_rules: Vec<Rule>,
    #[serde(default)]
    pub short_entry_rules: Vec<Rule>,
    /// Exit rules for long positions. Alias "exit_rules" for backward compat.
    #[serde(alias = "exit_rules")]
    pub long_exit_rules: Vec<Rule>,
    #[serde(default)]
    pub short_exit_rules: Vec<Rule>,
    /// Entry rule groups for long positions (richer logic: (A AND B) OR (C AND D)).
    /// When non-empty, takes precedence over long_entry_rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub long_entry_groups: Vec<RuleGroup>,
    /// Entry rule groups for short positions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub short_entry_groups: Vec<RuleGroup>,
    /// Exit rule groups for long positions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub long_exit_groups: Vec<RuleGroup>,
    /// Exit rule groups for short positions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub short_exit_groups: Vec<RuleGroup>,
    pub position_sizing: PositionSizing,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<StopLoss>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<TakeProfit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailing_stop: Option<TrailingStop>,
    pub trading_costs: TradingCosts,
    pub trade_direction: TradeDirection,
    /// Optional time window for trading. No new trades open outside this range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trading_hours: Option<TradingHours>,
    /// Optional daily trade limit. No more than this many trades per day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_daily_trades: Option<u32>,
    /// Optional time to force-close all open positions each day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_trades_at: Option<CloseTradesAt>,
    /// Entry order type. Market = immediate fill; Limit/Stop = pending order.
    #[serde(default)]
    pub entry_order: OrderType,
    /// Distance from signal price to place a Limit/Stop pending order, in pips.
    #[serde(default)]
    pub entry_order_offset_pips: f64,
    /// Close position after this many bars regardless of SL/TP or rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_after_bars: Option<u32>,
    /// If true, move stop loss to entry price (breakeven) once profit ≥ SL distance.
    #[serde(default)]
    pub move_sl_to_be: bool,
    /// If set, use this indicator-based offset for Stop/Limit order target price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_order_indicator: Option<OrderPriceConfig>,
}

// ── Backtest Precision ──

/// Precision mode for backtest execution.
/// Controls how SL/TP/trailing stop are resolved within each bar.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BacktestPrecision {
    /// Check SL/TP only against the selected timeframe's OHLC (fastest).
    SelectedTfOnly,
    /// Use M1 sub-bars to resolve SL/TP hit order within each TF bar.
    M1TickSimulation,
    /// Use raw tick data with custom spread for SL/TP resolution.
    RealTickCustomSpread,
    /// Use raw tick data with real bid/ask spread for SL/TP resolution.
    RealTickRealSpread,
}

impl Default for BacktestPrecision {
    fn default() -> Self {
        Self::SelectedTfOnly
    }
}

// ── Backtest Config ──

/// Configuration for a single backtest run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestConfig {
    pub symbol_id: String,
    pub timeframe: Timeframe,
    pub start_date: String,
    pub end_date: String,
    pub initial_capital: f64,
    pub leverage: f64,
    /// Precision mode for SL/TP resolution. Defaults to SelectedTfOnly.
    #[serde(default)]
    pub precision: BacktestPrecision,
    /// Builder optimization: if set, abort early if no trades opened after this fraction of bars.
    /// Only used during builder evaluation — normal UI backtests leave this as None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub early_stop_no_trades_pct: Option<f32>,
}
