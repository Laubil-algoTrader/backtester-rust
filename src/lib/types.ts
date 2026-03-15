// ══════════════════════════════════════════════════════════════
// TypeScript types — mirror of Rust models
// ══════════════════════════════════════════════════════════════

// ── Instrument Configuration ──

export type SwapMode = "InPips" | "InPoints" | "InMoney" | "AsPercent";

export interface InstrumentConfig {
  pip_size: number;
  pip_value: number;
  lot_size: number;
  min_lot: number;
  tick_size: number;
  digits: number;
  // Swap / overnight financing
  swap_long?: number;
  swap_short?: number;
  swap_mode?: SwapMode;
  triple_swap_day?: number; // ISO weekday Mon=1…Sun=7, default 3 (Wednesday)
  // Stops level
  min_stop_distance_pips?: number;
  // Timezone shift applied at import time (hours, e.g. -2 for UTC-2, 5.5 for UTC+5:30)
  tz_offset_hours?: number;
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
  /** User-facing symbol / storage key (e.g. "BCOUSD"). */
  symbol: string;
  /**
   * Dukascopy data-feed URL symbol when it differs from `symbol`
   * (e.g. "BRENTCMDUSD" for Brent Crude).  Undefined means the
   * data-feed uses `symbol` directly.
   */
  api_symbol?: string;
  category: DukascopyCategory;
  /**
   * Dukascopy bi5 price divisor.  Raw u32 prices in the binary file
   * are divided by this value to recover the real price.
   * Source: dukascopy-node instrument-meta-data.json decimalFactor.
   */
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
  // decimalFactor=1000 for all indices (confirmed from dukascopy-node instrument-meta-data.json)
  { name: "S&P 500",     symbol: "USA500IDXUSD",  category: "Indices", point_value: 1000, preset: "Indices" },
  { name: "Dow Jones 30",symbol: "USA30IDXUSD",   category: "Indices", point_value: 1000, preset: "Indices" },
  { name: "Nasdaq 100",  symbol: "USATECHIDXUSD", category: "Indices", point_value: 1000, preset: "Indices" },
  { name: "DAX 40",      symbol: "DEUIDXEUR",     category: "Indices", point_value: 1000, preset: "Indices" },
  { name: "FTSE 100",    symbol: "GBRIDXGBP",     category: "Indices", point_value: 1000, preset: "Indices" },
  { name: "Nikkei 225",  symbol: "JPNIDXJPY",     category: "Indices", point_value: 1000, preset: "Indices" },
  { name: "CAC 40",      symbol: "FRAIDXEUR",     category: "Indices", point_value: 1000, preset: "Indices" },
  { name: "ASX 200",     symbol: "AUSIDXAUD",     category: "Indices", point_value: 1000, preset: "Indices" },
  // Euro Stoxx 50: Dukascopy feed uses "EUSIDXEUR" (not EABORIDXEUR)
  { name: "Euro Stoxx 50", symbol: "EUSIDXEUR",   category: "Indices", point_value: 1000, preset: "Indices" },
  { name: "Hang Seng",   symbol: "HKGIDXHKD",    category: "Indices", point_value: 1000, preset: "Indices" },
  // ── Commodities ──
  // Metals: decimalFactor=1000 (3 decimal places in bi5)
  // Oils/Gas/Copper: Dukascopy uses "CMDUSD" suffix in feed URL.
  //   api_symbol = the actual URL symbol; symbol = user-facing storage key.
  { name: "Gold (XAU/USD)",   symbol: "XAUUSD",  category: "Commodities", point_value: 1000,  preset: "Gold (XAUUSD)" },
  { name: "Silver (XAG/USD)", symbol: "XAGUSD",  category: "Commodities", point_value: 1000,  preset: "Forex Major" },
  { name: "Brent Crude Oil",  symbol: "BCOUSD",  api_symbol: "BRENTCMDUSD", category: "Commodities", point_value: 1000,  preset: "Forex Major" },
  { name: "WTI Crude Oil",    symbol: "WTIUSD",  api_symbol: "LIGHTCMDUSD", category: "Commodities", point_value: 1000,  preset: "Forex Major" },
  { name: "Natural Gas",      symbol: "NGUSD",   api_symbol: "GASCMDUSD",   category: "Commodities", point_value: 10000, preset: "Forex Major" },
  { name: "Platinum",         symbol: "XPTUSD",  api_symbol: "XPTCMDUSD",   category: "Commodities", point_value: 1000,  preset: "Gold (XAUUSD)" },
  { name: "Palladium",        symbol: "XPDUSD",  api_symbol: "XPDCMDUSD",   category: "Commodities", point_value: 1000,  preset: "Gold (XAUUSD)" },
  { name: "Copper",           symbol: "XCUUSD",  api_symbol: "COPPERCMDUSD",category: "Commodities", point_value: 10000, preset: "Forex Major" },
  // ── Crypto ──
  // decimalFactor=10 for BTC/ETH/LTC (confirmed from dukascopy-node)
  // XRP is NOT available in Dukascopy's historical feed — removed.
  { name: "BTC/USD", symbol: "BTCUSD", category: "Crypto", point_value: 10, preset: "Crypto" },
  { name: "ETH/USD", symbol: "ETHUSD", category: "Crypto", point_value: 10, preset: "Crypto" },
  { name: "LTC/USD", symbol: "LTCUSD", category: "Crypto", point_value: 10, preset: "Crypto" },
];

// ── Symbol ──

/** Storage format for raw tick data (bid/ask). Mirrors `TickStorageFormat` in Rust. */
export type TickStorageFormat = "Parquet" | "Binary";

/** Download pipeline for tick-mode Dukascopy downloads.
 *  - "direct"  → bi5 → Parquet/Binary (fast, default)
 *  - "via_csv" → bi5 → CSV → Parquet/Binary (parity with manual import)
 */
export type TickPipeline = "direct" | "via_csv";

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

export type OperandType = "Indicator" | "Price" | "Constant" | "BarTime" | "CandlePattern" | "Compound";

export type ArithmeticOp = "Add" | "Sub" | "Mul" | "Div";

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
  // Compound operand fields (only used when operand_type === "Compound")
  compound_left?: Operand;
  compound_op?: ArithmeticOp;
  compound_right?: Operand;
}

export interface RuleGroup {
  id: string;
  rules: Rule[];
  internal: LogicalOperator; // How rules within this group combine
  join?: LogicalOperator;    // How this group connects to the next group
}

export interface Rule {
  id: string;
  left_operand: Operand;
  comparator: Comparator;
  right_operand: Operand;
  logical_operator?: LogicalOperator; // connector to next rule
}

// ── Position Sizing ──

export type PositionSizingType = "FixedLots" | "FixedAmount" | "PercentEquity" | "RiskBased" | "AntiMartingale";

export interface PositionSizing {
  sizing_type: PositionSizingType;
  value: number; // lots, amount, percentage, or risk %
  /** AntiMartingale: lot multiplier per consecutive loss (0,1]. Default 0.9 = −10% per loss. */
  decrease_factor?: number;
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
  long_entry_groups?: RuleGroup[];
  short_entry_groups?: RuleGroup[];
  long_exit_groups?: RuleGroup[];
  short_exit_groups?: RuleGroup[];
  position_sizing: PositionSizing;
  stop_loss?: StopLoss;
  take_profit?: TakeProfit;
  trailing_stop?: TrailingStop;
  trading_costs: TradingCosts;
  trade_direction: TradeDirection;
  trading_hours?: TradingHours;
  max_daily_trades?: number;
  close_trades_at?: CloseTradesAt;
  entry_order?: "market" | "limit" | "stop";
  entry_order_offset_pips?: number;
  close_after_bars?: number;
  move_sl_to_be?: boolean;
  entry_order_indicator?: OrderPriceConfig;
}

export type OrderPriceBaseField = "open" | "high" | "low" | "close";

export interface OrderPriceConfig {
  indicator: IndicatorConfig;
  multiplier: number;
  base_price_stop: OrderPriceBaseField;
  base_price_limit: OrderPriceBaseField;
}

export interface BuilderOrderPriceBlock {
  indicatorType: IndicatorType;
  enabled: boolean;
  weight: number;
  multiplierMin: number;
  multiplierMax: number;
  multiplierStep: number;
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

export type TradeCloseReason = "Signal" | "StopLoss" | "TakeProfit" | "TrailingStop" | "EndOfData" | "TimeClose" | "ExitAfterBars";

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
  /** Total swap charged over the life of this trade (negative = cost). */
  swap: number;
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

  // Additional metrics (P4.4)
  k_ratio: number;
  omega_ratio: number;
  monthly_returns: MonthlyReturn[];

  // Costs breakdown
  total_swap_charged: number;
  total_commission_charged: number;
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
  /** The backtest configuration used to produce these results. */
  backtest_config: BacktestConfig;
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

export type AppSection = "data" | "strategy" | "backtest" | "optimization" | "robustez" | "export" | "builder" | "projects";

// ── Monte Carlo ──

/** Configuration for a Monte Carlo simulation run. */
export interface MonteCarloConfig {
  n_simulations: number;
  /** Apply bootstrap resampling (draw N trades with replacement). */
  use_resampling: boolean;
  /** Randomly skip each trade with skip_probability. */
  use_skip_trades: boolean;
  /** Probability 0–1 of skipping each trade. Only used when use_skip_trades is true. */
  skip_probability: number;
  /**
   * Ruin threshold as a % loss of initial capital (0–100).
   * A simulation is "ruined" when equity drops below initial_capital × (1 − ruin_threshold_pct/100).
   * Default 20 = losing 20% of capital counts as ruin.
   */
  ruin_threshold_pct: number;
}

/**
 * One row in the confidence-level table.
 * C% confidence = "only (100−C)% chance results will be worse than these values."
 * For net_profit / ret_dd_ratio / expectancy: pessimistic tail (lower values).
 * For max_drawdown_abs: worst-case tail (higher values).
 */
export interface MonteCarloConfidenceRow {
  level: number;
  net_profit: number;
  max_drawdown_abs: number;
  ret_dd_ratio: number;
  expectancy: number;
}

export interface MonteCarloResult {
  n_simulations: number;
  /** Fraction of simulations (0–1) where equity fell below initial capital at any point */
  ruin_probability: number;

  // Original strategy metrics (comparison row in table)
  original_net_profit: number;
  original_max_drawdown_abs: number;
  original_ret_dd_ratio: number;
  original_expectancy: number;
  original_return_pct: number;
  original_max_drawdown_pct: number;

  /** Confidence table rows at levels [50, 60, 70, 80, 90, 92, 95, 97, 98] */
  confidence_table: MonteCarloConfidenceRow[];

  /** Sampled simulation equity curves for visualization (max 200, each ≤300 points) */
  sim_equity_curves: number[][];
  /** Original historical equity curve, same downsampling */
  original_equity_curve: number[];
}

// ── New BacktestMetrics fields (P4.4) ──

export interface MonthlyReturn {
  year: number;
  month: number;
  return_pct: number;
}

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

// ── Builder / Strategy Miner ──────────────────────────────────────────────────

export type BuilderDirection = "long_only" | "short_only" | "both_symmetric" | "both_asymmetric";
export type BuilderBuildMode = "genetic_evolution";
export type BuilderSLType = "atr" | "pips" | "percentage";
export type BuilderTPType = "atr" | "pips" | "rr";
export type BuilderStagnationSample = "in_sample" | "out_of_sample" | "full";
export type BuilderMMMethod =
  | "fixed_size"
  | "risk_fixed_balance"
  | "risk_fixed_account"
  | "fixed_amount"
  | "crypto_by_price"
  | "stocks_by_price"
  | "simple_martingale";
export type BuilderFitnessSource = "main_data" | "in_sample" | "out_of_sample" | "full";
export type BuilderComputeFrom =
  | "net_profit"
  | "return_dd"
  | "r_expectancy"
  | "annual_max_dd"
  | "weighted_fitness";
export type BuilderStopWhen = "never" | "totally" | "databank_full" | "after_time";
export type BuilderOrderTypesToClose = "all" | "market" | "stop" | "limit";
export type BuilderDataRangePartType = "is" | "oos";
export type BuilderFilterOperator = ">=" | ">" | "<=" | "<" | "==";

export interface BuilderFilterCondition {
  id: string;
  leftValue: string;
  operator: BuilderFilterOperator;
  rightValue: number;
}

export interface BuilderWeightedCriterion {
  id: string;
  criterium: string;
  type: "minimize" | "maximize";
  weight: number;
  target: number;
}

export interface BuilderDataRangePart {
  id: string;
  type: BuilderDataRangePartType;
  percent: number;
}

export interface BuilderIndicatorBlock {
  indicatorType: IndicatorType;
  enabled: boolean;
  weight: number;
  /** Custom period range override for this indicator. Undefined = use global range. */
  periodMin?: number;
  periodMax?: number;
  periodStep?: number;
}

export interface BuilderOrderTypeBlock {
  orderType: "stop" | "limit" | "market";
  enabled: boolean;
  weight: number;
}

export interface BuilderExitTypeBlock {
  exitType:
    | "exit_after_bars"
    | "move_sl_be"
    | "profit_target"
    | "stop_loss"
    | "trailing_stop"
    | "exit_rule";
  enabled: boolean;
  required: boolean;
}

// ── Sub-configs per settings tab ──

export interface BuilderWhatToBuild {
  direction: BuilderDirection;
  buildMode: BuilderBuildMode;
  minEntryRules: number;
  maxEntryRules: number;
  minExitRules: number;
  maxExitRules: number;
  maxLookback: number;
  indicatorPeriodMin: number;
  indicatorPeriodMax: number;
  indicatorPeriodStep: number;
  slRequired: boolean;
  slType: BuilderSLType;
  slCoeffMin: number;
  slCoeffMax: number;
  slCoeffStep: number;
  slAtrPeriodMin: number;
  slAtrPeriodMax: number;
  slAtrPeriodStep: number;
  tpRequired: boolean;
  tpType: BuilderTPType;
  tpCoeffMin: number;
  tpCoeffMax: number;
  tpCoeffStep: number;
  tpAtrPeriodMin: number;
  tpAtrPeriodMax: number;
  tpAtrPeriodStep: number;
}

export interface BuilderGeneticOptions {
  maxGenerations: number;
  populationPerIsland: number;
  crossoverProbability: number;
  mutationProbability: number;
  islands: number;
  migrateEveryN: number;
  migrationRate: number;
  initialPopulationSize: number;
  useFromDatabank: boolean;
  decimationCoefficient: number;
  initialFilters: BuilderFilterCondition[];
  freshBloodDetectDuplicates: boolean;
  freshBloodReplacePercent: number;
  freshBloodReplaceEvery: number;
  showLastGeneration: boolean;
  startAgainWhenFinished: boolean;
  restartOnStagnation: boolean;
  stagnationSample: BuilderStagnationSample;
  stagnationGenerations: number;
  prefilterWindowPct: number;
  prefilterMinTrades: number;
  phaseBasedAdaptation: boolean;
  fitnessSharingSigma: number;
  fitnessSharingAlpha: number;
  /** Distance metric for fitness sharing niching. Default: "structural". */
  nichingMode: BehavioralNichingMode;
  /** EMA learning rate for meta-learning grammar adaptation. 0 = disabled, e.g. 0.05. */
  metaLearningRate: number;
  /** Top fraction of population used for meta-learning updates. e.g. 0.25. */
  metaLearningTopPct: number;
}

export type BehavioralNichingMode =
  | "structural"     // Jaccard on indicator types (fast, always available)
  | "equity_curve"   // 1 - Pearson correlation of equity curves
  | "trade_overlap"  // Jaccard on trade entry timestamps
  | "combined";      // average of equity_curve + trade_overlap

export interface BuilderDataConfig {
  symbolId: string | null;
  timeframe: Timeframe;
  startDate: string;
  endDate: string;
  precision: BacktestPrecision;
  spreadPips: number;
  slippagePips: number;
  minDistancePips: number;
  dataRangeParts: BuilderDataRangePart[];
}

export interface BuilderTradingOptions {
  dontTradeWeekends: boolean;
  fridayCloseTime: string;
  sundayOpenTime: string;
  exitAtEndOfDay: boolean;
  endOfDayExitTime: string;
  exitOnFriday: boolean;
  fridayExitTime: string;
  limitTimeRange: boolean;
  timeRangeFrom: string;
  timeRangeTo: string;
  exitAtEndOfRange: boolean;
  orderTypesToClose: BuilderOrderTypesToClose;
  maxDistanceFromMarket: boolean;
  maxDistancePercent: number;
  maxTradesPerDay: number;
  minimumSL: number;
  maximumSL: number;
  minimumPT: number;
  maximumPT: number;
}

export interface BuilderBuildingBlocks {
  indicators: BuilderIndicatorBlock[];
  orderTypes: BuilderOrderTypeBlock[];
  exitTypes: BuilderExitTypeBlock[];
  orderPriceIndicators: BuilderOrderPriceBlock[];
  orderPriceBaseStop: OrderPriceBaseField;
  orderPriceBaseLimit: OrderPriceBaseField;
}

export interface BuilderMoneyManagement {
  initialCapital: number;
  method: BuilderMMMethod;
  riskedMoney: number;
  sizeDecimals: number;
  sizeIfNoMM: number;
  maximumLots: number;
}

export interface BuilderRanking {
  maxStrategiesToStore: number;
  stopWhen: BuilderStopWhen;
  stopTotallyCount: number;
  stopAfterDays: number;
  stopAfterHours: number;
  stopAfterMinutes: number;
  fitnessSource: BuilderFitnessSource;
  computeFrom: BuilderComputeFrom;
  weightedCriteria: BuilderWeightedCriterion[];
  customFilters: BuilderFilterCondition[];
  dismissSimilar: boolean;
  complexityAlpha: number;
}

export interface BuilderCrossChecks {
  disableAll: boolean;
  whatIf: boolean;
  monteCarlo: boolean;
  higherPrecision: boolean;
  additionalMarkets: boolean;
  monteCarloRetest: boolean;
  sequentialOpt: boolean;
  walkForward: boolean;
  walkForwardMatrix: boolean;
}

export interface BuilderConfig {
  whatToBuild: BuilderWhatToBuild;
  geneticOptions: BuilderGeneticOptions;
  dataConfig: BuilderDataConfig;
  tradingOptions: BuilderTradingOptions;
  buildingBlocks: BuilderBuildingBlocks;
  moneyManagement: BuilderMoneyManagement;
  ranking: BuilderRanking;
  crossChecks: BuilderCrossChecks;
}

// ── Builder Runtime State ──────────────────────────────────────────────────

/** Per-island stats emitted after each generation. */
export interface BuilderIslandStats {
  islandId: number;
  generation: number;
  population: number;
  bestFitness: number;
}

/** Live stats emitted by the builder engine during a run. */
export interface BuilderRuntimeStats {
  generated: number;
  accepted: number;
  rejected: number;
  inDatabank: number;
  startTime: number | null;   // Date.now() when run started
  strategiesPerHour: number;
  acceptedPerHour: number;
  timePerStrategyMs: number;
  generation: number;
  island: number;
  bestFitness: number;
}

/** A named collection of strategies produced by the builder. */
export interface BuilderDatabank {
  id: string;
  name: string;
  strategies: BuilderSavedStrategy[];
}

/** A strategy saved to the results databank by the builder. */
export interface BuilderSavedStrategy {
  id: string;
  name: string;
  createdAt: string;
  fitness: number;
  symbolId: string | null;
  symbolName: string;
  timeframe: Timeframe;
  // IS metrics
  netProfit: number;
  miniEquityCurve: number[];
  trades: number;
  profitFactor: number;
  sharpeRatio: number;
  rExpectancy: number;
  annualReturnPct: number;
  maxDrawdownAbs: number;
  winLossRatio: number;
  retDDRatio: number;
  cagrMaxDDPct: number;
  avgWin: number;
  avgLoss: number;
  avgBarsWin: number;
  strategyJson: string;
  /** Hash for fast duplicate detection — set by backend, optional for backward compat. */
  fingerprint?: number;
  // OOS metrics — present only when builder had an OOS split configured
  oosNetProfit?: number;
  oosTrades?: number;
  oosProfitFactor?: number;
  oosSharpeRatio?: number;
  oosMaxDrawdownAbs?: number;
  oosWinRatePct?: number;
}

// ── Custom Projects ──────────────────────────────────────────────────────────

export interface ProjectTask {
  id: string;
  name: string;
  type: "builder";
  config: BuilderConfig;
  databanks: BuilderDatabank[];
  status: "idle" | "running" | "paused";
  strategiesCount: number;
  databankCount: number;
  createdAt: string;
  lastRunAt?: string;
}

export interface Project {
  id: string;
  name: string;
  tasks: ProjectTask[];
  createdAt: string;
  updatedAt: string;
}
