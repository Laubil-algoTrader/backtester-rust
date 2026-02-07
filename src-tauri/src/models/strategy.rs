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
    VWAP,
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
}

/// Configuration for a single indicator instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndicatorConfig {
    pub indicator_type: IndicatorType,
    pub params: IndicatorParams,
    /// For multi-output indicators (e.g. "upper"/"middle"/"lower" for Bollinger Bands).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_field: Option<String>,
}

impl IndicatorConfig {
    /// Generate a unique cache key for this indicator configuration.
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
        key
    }
}

// ── Rules ──

/// Comparison operators for rule evaluation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogicalOperator {
    #[serde(rename = "AND")]
    And,
    #[serde(rename = "OR")]
    Or,
}

/// Type discriminator for operands.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperandType {
    Indicator,
    Price,
    Constant,
}

/// Which price field to use as an operand.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PriceField {
    Open,
    High,
    Low,
    Close,
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
    /// Look back N bars for the operand value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionSizing {
    pub sizing_type: PositionSizingType,
    pub value: f64,
}

// ── Stop Loss ──

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
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
}

// ── Trade Direction ──

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TradeDirection {
    Long,
    Short,
    Both,
}

// ── Strategy ──

/// A complete trading strategy with entry/exit rules and configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strategy {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub entry_rules: Vec<Rule>,
    pub exit_rules: Vec<Rule>,
    pub position_sizing: PositionSizing,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<StopLoss>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<TakeProfit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trailing_stop: Option<TrailingStop>,
    pub trading_costs: TradingCosts,
    pub trade_direction: TradeDirection,
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
}
