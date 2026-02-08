// ══════════════════════════════════════════════════════════════
// TypeScript types — mirror of Rust models
// ══════════════════════════════════════════════════════════════

// ── Instrument Configuration ──

export interface InstrumentConfig {
  pip_size: number;
  pip_value: number;
  lot_size: number;
  min_lot: number;
  tick_size: number;
  digits: number;
}

export const INSTRUMENT_PRESETS: Record<string, InstrumentConfig> = {
  "Forex Major": {
    pip_size: 0.0001,
    pip_value: 10,
    lot_size: 100000,
    min_lot: 0.01,
    tick_size: 0.00001,
    digits: 5,
  },
  "Forex JPY": {
    pip_size: 0.01,
    pip_value: 1000,
    lot_size: 100000,
    min_lot: 0.01,
    tick_size: 0.001,
    digits: 3,
  },
  Crypto: {
    pip_size: 0.01,
    pip_value: 0.01,
    lot_size: 1,
    min_lot: 0.001,
    tick_size: 0.01,
    digits: 2,
  },
  Indices: {
    pip_size: 1,
    pip_value: 1,
    lot_size: 1,
    min_lot: 0.1,
    tick_size: 0.5,
    digits: 1,
  },
};

// ── Symbol ──

export type Timeframe = "tick" | "m1" | "m5" | "m15" | "m30" | "h1" | "h4" | "d1";

export interface Symbol {
  id: string;
  name: string;
  base_timeframe: Timeframe;
  upload_date: string;
  total_rows: number;
  start_date: string;
  end_date: string;
  timeframe_paths: Partial<Record<Timeframe, string>>;
  instrument_config: InstrumentConfig;
}

// ── Indicators ──

export type IndicatorType =
  | "SMA"
  | "EMA"
  | "RSI"
  | "MACD"
  | "BollingerBands"
  | "ATR"
  | "Stochastic"
  | "ADX"
  | "CCI"
  | "ROC"
  | "WilliamsR"
  | "ParabolicSAR"
  | "VWAP";

export interface IndicatorParams {
  period?: number;
  fast_period?: number;
  slow_period?: number;
  signal_period?: number;
  std_dev?: number;
  k_period?: number;
  d_period?: number;
  acceleration_factor?: number;
  maximum_factor?: number;
}

export interface IndicatorConfig {
  indicator_type: IndicatorType;
  params: IndicatorParams;
  output_field?: string; // e.g., "upper", "middle", "lower" for Bollinger
}

// ── Rules ──

export type Comparator =
  | "GreaterThan"
  | "LessThan"
  | "GreaterOrEqual"
  | "LessOrEqual"
  | "Equal"
  | "CrossAbove"
  | "CrossBelow";

export type LogicalOperator = "AND" | "OR";

export type OperandType = "Indicator" | "Price" | "Constant";

export type PriceField = "Open" | "High" | "Low" | "Close";

export interface Operand {
  operand_type: OperandType;
  indicator?: IndicatorConfig;
  price_field?: PriceField;
  constant_value?: number;
  offset?: number; // N bars back
}

export interface Rule {
  id: string;
  left_operand: Operand;
  comparator: Comparator;
  right_operand: Operand;
  logical_operator?: LogicalOperator; // connector to next rule
}

// ── Position Sizing ──

export type PositionSizingType = "FixedLots" | "FixedAmount" | "PercentEquity" | "RiskBased";

export interface PositionSizing {
  sizing_type: PositionSizingType;
  value: number; // lots, amount, percentage, or risk %
}

// ── Stop Loss ──

export type StopLossType = "Pips" | "Percentage" | "ATR";

export interface StopLoss {
  sl_type: StopLossType;
  value: number;
  atr_period?: number;
}

// ── Take Profit ──

export type TakeProfitType = "Pips" | "RiskReward" | "ATR";

export interface TakeProfit {
  tp_type: TakeProfitType;
  value: number;
  atr_period?: number;
}

// ── Trailing Stop ──

export type TrailingStopType = "ATR" | "RiskReward";

export interface TrailingStop {
  ts_type: TrailingStopType;
  value: number;
  atr_period?: number;
}

// ── Trading Costs ──

export type CommissionType = "Percentage" | "FixedPerLot";

export interface TradingCosts {
  spread_pips: number;
  commission_type: CommissionType;
  commission_value: number;
  slippage_pips: number;
  slippage_random: boolean;
}

// ── Strategy ──

export type TradeDirection = "Long" | "Short" | "Both";

export interface Strategy {
  id: string;
  name: string;
  created_at: string;
  updated_at: string;
  entry_rules: Rule[];
  exit_rules: Rule[];
  position_sizing: PositionSizing;
  stop_loss?: StopLoss;
  take_profit?: TakeProfit;
  trailing_stop?: TrailingStop;
  trading_costs: TradingCosts;
  trade_direction: TradeDirection;
}

// ── Backtest Config ──

export interface BacktestConfig {
  symbol_id: string;
  timeframe: Timeframe;
  start_date: string;
  end_date: string;
  initial_capital: number;
  leverage: number;
}

// ── Trade Result ──

export type TradeCloseReason = "Signal" | "StopLoss" | "TakeProfit" | "TrailingStop" | "EndOfData";

export interface TradeResult {
  id: string;
  direction: "Long" | "Short";
  entry_time: string;
  entry_price: number;
  exit_time: string;
  exit_price: number;
  lots: number;
  pnl: number;
  pnl_pips: number;
  commission: number;
  close_reason: TradeCloseReason;
  duration_bars: number;
  duration_time: string;
  mae: number;
  mfe: number;
}

// ── Metrics ──

export interface BacktestMetrics {
  // Returns
  final_capital: number;
  total_return_pct: number;
  annualized_return_pct: number;
  monthly_return_avg_pct: number;

  // Risk-adjusted
  sharpe_ratio: number;
  sortino_ratio: number;
  calmar_ratio: number;

  // Drawdown
  max_drawdown_pct: number;
  max_drawdown_duration_bars: number;
  max_drawdown_duration_time: string;
  avg_drawdown_pct: number;
  recovery_factor: number;

  // Trades
  total_trades: number;
  winning_trades: number;
  losing_trades: number;
  breakeven_trades: number;
  win_rate_pct: number;

  // P&L
  gross_profit: number;
  gross_loss: number;
  net_profit: number;
  profit_factor: number;
  avg_trade: number;
  avg_win: number;
  avg_loss: number;
  largest_win: number;
  largest_loss: number;
  expectancy: number;

  // Consistency
  max_consecutive_wins: number;
  max_consecutive_losses: number;
  avg_consecutive_wins: number;
  avg_consecutive_losses: number;

  // Time
  avg_trade_duration: string;
  avg_bars_in_trade: number;
  avg_winner_duration: string;
  avg_loser_duration: string;

  // Risk
  mae_avg: number;
  mae_max: number;
  mfe_avg: number;
  mfe_max: number;
}

// ── Equity/Drawdown points ──

export interface EquityPoint {
  timestamp: string;
  equity: number;
}

export interface DrawdownPoint {
  timestamp: string;
  drawdown_pct: number;
}

// ── Backtest Results ──

export interface BacktestResults {
  trades: TradeResult[];
  equity_curve: EquityPoint[];
  drawdown_curve: DrawdownPoint[];
  returns: number[];
  metrics: BacktestMetrics;
}

// ── Optimization ──

export type OptimizationMethod = "GridSearch" | "GeneticAlgorithm";

export type ObjectiveFunction = "TotalProfit" | "SharpeRatio" | "ProfitFactor" | "WinRate";

export interface ParameterRange {
  rule_index: number;
  param_name: string;
  display_name: string;
  min: number;
  max: number;
  step: number;
}

export interface GeneticAlgorithmConfig {
  population_size: number;
  generations: number;
  mutation_rate: number;
  crossover_rate: number;
}

export interface OptimizationConfig {
  method: OptimizationMethod;
  parameter_ranges: ParameterRange[];
  objective: ObjectiveFunction;
  backtest_config: BacktestConfig;
  ga_config?: GeneticAlgorithmConfig;
}

export interface OptimizationResult {
  params: Record<string, number>;
  objective_value: number;
  total_return_pct: number;
  sharpe_ratio: number;
  max_drawdown_pct: number;
  total_trades: number;
  profit_factor: number;
}

// ── Progress Events ──

export interface ConversionProgress {
  percent: number;
  message: string;
}

export interface BacktestProgress {
  percent: number;
  current_bar: number;
  total_bars: number;
}

export interface OptimizationProgress {
  percent: number;
  current: number;
  total: number;
  best_so_far: number;
  eta_seconds: number;
}

// ── Error Response ──

export interface ErrorResponse {
  code: string;
  message: string;
}

// ── App Section ──

export type AppSection = "data" | "strategy" | "backtest" | "optimization";
