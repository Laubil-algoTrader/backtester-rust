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
  "Gold (XAUUSD)": {
    pip_size: 0.01,
    pip_value: 1,
    lot_size: 100,
    min_lot: 0.01,
    tick_size: 0.01,
    digits: 2,
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

// ── Dukascopy Download ──

export type DukascopyCategory = "Forex" | "Indices" | "Commodities" | "Crypto";

export interface DukascopyInstrument {
  name: string;
  symbol: string;
  category: DukascopyCategory;
  point_value: number;
  preset: string;
}

export const DUKASCOPY_INSTRUMENTS: DukascopyInstrument[] = [
  // ── Forex Major ──
  { name: "EUR/USD", symbol: "EURUSD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "GBP/USD", symbol: "GBPUSD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/CHF", symbol: "USDCHF", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "AUD/USD", symbol: "AUDUSD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/CAD", symbol: "USDCAD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "NZD/USD", symbol: "NZDUSD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "EUR/GBP", symbol: "EURGBP", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "EUR/CHF", symbol: "EURCHF", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "GBP/CHF", symbol: "GBPCHF", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "AUD/CAD", symbol: "AUDCAD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "AUD/CHF", symbol: "AUDCHF", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "AUD/NZD", symbol: "AUDNZD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "EUR/AUD", symbol: "EURAUD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "EUR/NZD", symbol: "EURNZD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "EUR/CAD", symbol: "EURCAD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "GBP/AUD", symbol: "GBPAUD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "GBP/NZD", symbol: "GBPNZD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "GBP/CAD", symbol: "GBPCAD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "NZD/CAD", symbol: "NZDCAD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "NZD/CHF", symbol: "NZDCHF", category: "Forex", point_value: 100000, preset: "Forex Major" },
  // ── Forex JPY pairs ──
  { name: "USD/JPY", symbol: "USDJPY", category: "Forex", point_value: 1000, preset: "Forex JPY" },
  { name: "EUR/JPY", symbol: "EURJPY", category: "Forex", point_value: 1000, preset: "Forex JPY" },
  { name: "GBP/JPY", symbol: "GBPJPY", category: "Forex", point_value: 1000, preset: "Forex JPY" },
  { name: "AUD/JPY", symbol: "AUDJPY", category: "Forex", point_value: 1000, preset: "Forex JPY" },
  { name: "CAD/JPY", symbol: "CADJPY", category: "Forex", point_value: 1000, preset: "Forex JPY" },
  { name: "CHF/JPY", symbol: "CHFJPY", category: "Forex", point_value: 1000, preset: "Forex JPY" },
  { name: "NZD/JPY", symbol: "NZDJPY", category: "Forex", point_value: 1000, preset: "Forex JPY" },
  // ── Forex Exotic ──
  { name: "USD/ZAR", symbol: "USDZAR", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/MXN", symbol: "USDMXN", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/TRY", symbol: "USDTRY", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/SEK", symbol: "USDSEK", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/NOK", symbol: "USDNOK", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/SGD", symbol: "USDSGD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/HKD", symbol: "USDHKD", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/PLN", symbol: "USDPLN", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/CZK", symbol: "USDCZK", category: "Forex", point_value: 100000, preset: "Forex Major" },
  { name: "USD/HUF", symbol: "USDHUF", category: "Forex", point_value: 1000, preset: "Forex JPY" },
  // ── Indices ──
  { name: "S&P 500", symbol: "USA500IDXUSD", category: "Indices", point_value: 1, preset: "Indices" },
  { name: "Dow Jones 30", symbol: "USA30IDXUSD", category: "Indices", point_value: 1, preset: "Indices" },
  { name: "Nasdaq 100", symbol: "USATECHIDXUSD", category: "Indices", point_value: 1, preset: "Indices" },
  { name: "DAX 40", symbol: "DEUIDXEUR", category: "Indices", point_value: 1, preset: "Indices" },
  { name: "FTSE 100", symbol: "GBRIDXGBP", category: "Indices", point_value: 1, preset: "Indices" },
  { name: "Nikkei 225", symbol: "JPNIDXJPY", category: "Indices", point_value: 1, preset: "Indices" },
  { name: "CAC 40", symbol: "FRAIDXEUR", category: "Indices", point_value: 1, preset: "Indices" },
  { name: "ASX 200", symbol: "AUSIDXAUD", category: "Indices", point_value: 1, preset: "Indices" },
  { name: "Euro Stoxx 50", symbol: "EABORIDXEUR", category: "Indices", point_value: 1, preset: "Indices" },
  { name: "Hang Seng", symbol: "HKGIDXHKD", category: "Indices", point_value: 1, preset: "Indices" },
  // ── Commodities ──
  { name: "Gold (XAU/USD)", symbol: "XAUUSD", category: "Commodities", point_value: 10, preset: "Gold (XAUUSD)" },
  { name: "Silver (XAG/USD)", symbol: "XAGUSD", category: "Commodities", point_value: 1000, preset: "Forex Major" },
  { name: "Brent Crude Oil", symbol: "BCOUSD", category: "Commodities", point_value: 1000, preset: "Forex Major" },
  { name: "WTI Crude Oil", symbol: "WTIUSD", category: "Commodities", point_value: 1000, preset: "Forex Major" },
  { name: "Natural Gas", symbol: "NGUSD", category: "Commodities", point_value: 10000, preset: "Forex Major" },
  { name: "Platinum", symbol: "XPTUSD", category: "Commodities", point_value: 10, preset: "Gold (XAUUSD)" },
  { name: "Palladium", symbol: "XPDUSD", category: "Commodities", point_value: 10, preset: "Gold (XAUUSD)" },
  { name: "Copper", symbol: "XCUUSD", category: "Commodities", point_value: 10000, preset: "Forex Major" },
  // ── Crypto ──
  { name: "BTC/USD", symbol: "BTCUSD", category: "Crypto", point_value: 1, preset: "Crypto" },
  { name: "ETH/USD", symbol: "ETHUSD", category: "Crypto", point_value: 1, preset: "Crypto" },
  { name: "LTC/USD", symbol: "LTCUSD", category: "Crypto", point_value: 1, preset: "Crypto" },
  { name: "XRP/USD", symbol: "XRPUSD", category: "Crypto", point_value: 100000, preset: "Crypto" },
];

// ── Symbol ──

export type Timeframe = "tick" | "m1" | "m5" | "m15" | "m30" | "h1" | "h4" | "d1";

export const TIMEFRAME_ORDER: Timeframe[] = ["tick", "m1", "m5", "m15", "m30", "h1", "h4", "d1"];

export function sortTimeframes(timeframes: string[]): string[] {
  return [...timeframes].sort(
    (a, b) => TIMEFRAME_ORDER.indexOf(a as Timeframe) - TIMEFRAME_ORDER.indexOf(b as Timeframe)
  );
}

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
  | "VWAP"
  | "Aroon"
  | "AwesomeOscillator"
  | "BarRange"
  | "BiggestRange"
  | "HighestInRange"
  | "LowestInRange"
  | "SmallestRange"
  | "BearsPower"
  | "BullsPower"
  | "DeMarker"
  | "Fibonacci"
  | "Fractal"
  | "GannHiLo"
  | "HeikenAshi"
  | "HullMA"
  | "Ichimoku"
  | "KeltnerChannel"
  | "LaguerreRSI"
  | "LinearRegression"
  | "Momentum"
  | "SuperTrend"
  | "TrueRange"
  | "StdDev"
  | "Reflex"
  | "Pivots"
  | "UlcerIndex"
  | "Vortex";

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
  gamma?: number;
  multiplier?: number;
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

export type OperandType = "Indicator" | "Price" | "Constant" | "BarTime" | "CandlePattern";

export type PriceField = "Open" | "High" | "Low" | "Close" | "DailyOpen" | "DailyHigh" | "DailyLow" | "DailyClose";

export type TimeField =
  | "CurrentBar"
  | "BarTimeValue"
  | "BarHour"
  | "BarMinute"
  | "BarDayOfWeek"
  | "CurrentTime"
  | "CurrentHour"
  | "CurrentMinute"
  | "CurrentDayOfWeek"
  | "CurrentMonth";

export type CandlePatternType =
  | "Doji"
  | "Hammer"
  | "ShootingStar"
  | "BearishEngulfing"
  | "BullishEngulfing"
  | "DarkCloud"
  | "PiercingLine";

export interface Operand {
  operand_type: OperandType;
  indicator?: IndicatorConfig;
  price_field?: PriceField;
  constant_value?: number;
  time_field?: TimeField;
  candle_pattern?: CandlePatternType;
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

export interface TradingHours {
  start_hour: number;
  start_minute: number;
  end_hour: number;
  end_minute: number;
}

export interface CloseTradesAt {
  hour: number;
  minute: number;
}

export interface Strategy {
  id: string;
  name: string;
  created_at: string;
  updated_at: string;
  long_entry_rules: Rule[];
  short_entry_rules: Rule[];
  long_exit_rules: Rule[];
  short_exit_rules: Rule[];
  position_sizing: PositionSizing;
  stop_loss?: StopLoss;
  take_profit?: TakeProfit;
  trailing_stop?: TrailingStop;
  trading_costs: TradingCosts;
  trade_direction: TradeDirection;
  trading_hours?: TradingHours;
  max_daily_trades?: number;
  close_trades_at?: CloseTradesAt;
}

// ── Backtest Precision ──

export type BacktestPrecision =
  | "SelectedTfOnly"
  | "M1TickSimulation"
  | "RealTickCustomSpread"
  | "RealTickRealSpread";

export const PRECISION_LABELS: Record<BacktestPrecision, string> = {
  SelectedTfOnly: "Selected TF only (fastest)",
  M1TickSimulation: "M1 tick simulation (slow)",
  RealTickCustomSpread: "Real Tick - custom spread (slowest)",
  RealTickRealSpread: "Real Tick - real spread (slowest)",
};

// ── Backtest Config ──

export interface BacktestConfig {
  symbol_id: string;
  timeframe: Timeframe;
  start_date: string;
  end_date: string;
  initial_capital: number;
  leverage: number;
  precision: BacktestPrecision;
}

// ── Trade Result ──

export type TradeCloseReason = "Signal" | "StopLoss" | "TakeProfit" | "TrailingStop" | "EndOfData" | "TimeClose";

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

  // Stagnation & Ulcer
  stagnation_bars: number;
  stagnation_time: string;
  ulcer_index_pct: number;

  // Return / Drawdown ratio
  return_dd_ratio: number;
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

export type ObjectiveFunction = "TotalProfit" | "SharpeRatio" | "ProfitFactor" | "WinRate" | "ReturnDdRatio" | "MinStagnation" | "MinUlcerIndex";

export type ParamSource = "long_entry" | "short_entry" | "long_exit" | "short_exit" | "stop_loss" | "take_profit" | "trailing_stop" | "trading_hours" | "close_trades_at";

export interface ParameterRange {
  rule_index: number;
  param_name: string;
  display_name: string;
  min: number;
  max: number;
  step: number;
  operand_side: "left" | "right";
  param_source: ParamSource;
}

export interface GeneticAlgorithmConfig {
  population_size: number;
  generations: number;
  mutation_rate: number;
  crossover_rate: number;
}

export interface OosPeriod {
  label: string;
  start_date: string;
  end_date: string;
}

export interface OosResult {
  label: string;
  total_return_pct: number;
  sharpe_ratio: number;
  max_drawdown_pct: number;
  profit_factor: number;
  total_trades: number;
}

export interface OptimizationConfig {
  method: OptimizationMethod;
  parameter_ranges: ParameterRange[];
  objectives: ObjectiveFunction[];
  backtest_config: BacktestConfig;
  ga_config?: GeneticAlgorithmConfig;
  oos_periods: OosPeriod[];
}

export interface OptimizationResult {
  params: Record<string, number>;
  objective_value: number;
  composite_score: number;
  total_return_pct: number;
  sharpe_ratio: number;
  max_drawdown_pct: number;
  total_trades: number;
  profit_factor: number;
  return_dd_ratio: number;
  stagnation_bars: number;
  ulcer_index_pct: number;
  oos_results: OosResult[];
  equity_curve: EquityPoint[];
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

// ── Code Generation ──

export interface CodeFile {
  filename: string;
  code: string;
  is_main: boolean;
}

export interface CodeGenerationResult {
  files: CodeFile[];
}

// ── App Section ──

export type AppSection = "data" | "strategy" | "backtest" | "optimization" | "export";

// ── License ──

export type LicenseTier = "free" | "pro";

export interface LicenseResponse {
  valid: boolean;
  tier: LicenseTier;
  message?: string;
}

export interface SavedCredentials {
  username: string;
  license_key: string;
}
