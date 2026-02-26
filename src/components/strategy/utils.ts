import type {
  IndicatorType,
  IndicatorConfig,
  IndicatorParams,
  Operand,
  Rule,
  Comparator,
  TimeField,
  CandlePatternType,
} from "@/lib/types";

// ── Indicator metadata ──

export interface ParamField {
  key: keyof IndicatorParams;
  label: string;
  defaultValue: number;
  min: number;
  max: number;
  step: number;
}

export const INDICATOR_OPTIONS: { value: IndicatorType; label: string }[] = [
  { value: "SMA", label: "SMA" },
  { value: "EMA", label: "EMA" },
  { value: "RSI", label: "RSI" },
  { value: "MACD", label: "MACD" },
  { value: "BollingerBands", label: "Bollinger Bands" },
  { value: "ATR", label: "ATR" },
  { value: "Stochastic", label: "Stochastic" },
  { value: "ADX", label: "ADX" },
  { value: "CCI", label: "CCI" },
  { value: "ROC", label: "ROC" },
  { value: "WilliamsR", label: "Williams %R" },
  { value: "ParabolicSAR", label: "Parabolic SAR" },
  { value: "VWAP", label: "VWAP" },
  // New indicators
  { value: "Aroon", label: "Aroon" },
  { value: "AwesomeOscillator", label: "Awesome Oscillator" },
  { value: "BarRange", label: "Bar Range" },
  { value: "BiggestRange", label: "Biggest Range" },
  { value: "HighestInRange", label: "Highest in Range" },
  { value: "LowestInRange", label: "Lowest in Range" },
  { value: "SmallestRange", label: "Smallest Range" },
  { value: "BearsPower", label: "Bears Power" },
  { value: "BullsPower", label: "Bulls Power" },
  { value: "DeMarker", label: "DeMarker" },
  { value: "Fibonacci", label: "Fibonacci" },
  { value: "Fractal", label: "Fractal" },
  { value: "GannHiLo", label: "Gann HiLo" },
  { value: "HeikenAshi", label: "Heiken Ashi" },
  { value: "HullMA", label: "Hull MA" },
  { value: "Ichimoku", label: "Ichimoku" },
  { value: "KeltnerChannel", label: "Keltner Channel" },
  { value: "LaguerreRSI", label: "Laguerre RSI" },
  { value: "LinearRegression", label: "Linear Regression" },
  { value: "Momentum", label: "Momentum" },
  { value: "SuperTrend", label: "SuperTrend" },
  { value: "TrueRange", label: "True Range" },
  { value: "StdDev", label: "Std Deviation" },
  { value: "Reflex", label: "Reflex" },
  { value: "Pivots", label: "Pivots" },
  { value: "UlcerIndex", label: "Ulcer Index" },
  { value: "Vortex", label: "Vortex" },
];

export const COMPARATOR_OPTIONS: { value: Comparator; label: string }[] = [
  { value: "GreaterThan", label: ">" },
  { value: "LessThan", label: "<" },
  { value: "GreaterOrEqual", label: ">=" },
  { value: "LessOrEqual", label: "<=" },
  { value: "Equal", label: "==" },
  { value: "CrossAbove", label: "Cross Above" },
  { value: "CrossBelow", label: "Cross Below" },
];

export const TIME_FIELD_OPTIONS: { value: TimeField; label: string }[] = [
  { value: "CurrentBar", label: "Current Bar" },
  { value: "BarTimeValue", label: "Bar Time (min)" },
  { value: "BarHour", label: "Bar Hour" },
  { value: "BarMinute", label: "Bar Minute" },
  { value: "BarDayOfWeek", label: "Bar Day of Week" },
  { value: "CurrentTime", label: "Current Time (min)" },
  { value: "CurrentHour", label: "Current Hour" },
  { value: "CurrentMinute", label: "Current Minute" },
  { value: "CurrentDayOfWeek", label: "Current Day of Week" },
  { value: "CurrentMonth", label: "Current Month" },
];

export const CANDLE_PATTERN_OPTIONS: { value: CandlePatternType; label: string }[] = [
  { value: "Doji", label: "Doji" },
  { value: "Hammer", label: "Hammer" },
  { value: "ShootingStar", label: "Shooting Star" },
  { value: "BearishEngulfing", label: "Bearish Engulfing" },
  { value: "BullishEngulfing", label: "Bullish Engulfing" },
  { value: "DarkCloud", label: "Dark Cloud" },
  { value: "PiercingLine", label: "Piercing Line" },
];

/** Returns which parameter fields to show for a given indicator type. */
export function getIndicatorParamFields(type: IndicatorType): ParamField[] {
  switch (type) {
    case "SMA":
    case "EMA":
    case "RSI":
    case "ATR":
    case "ADX":
    case "CCI":
    case "ROC":
    case "WilliamsR":
      return [
        { key: "period", label: "Period", defaultValue: 14, min: 1, max: 500, step: 1 },
      ];
    case "MACD":
      return [
        { key: "fast_period", label: "Fast", defaultValue: 12, min: 1, max: 200, step: 1 },
        { key: "slow_period", label: "Slow", defaultValue: 26, min: 1, max: 200, step: 1 },
        { key: "signal_period", label: "Signal", defaultValue: 9, min: 1, max: 200, step: 1 },
      ];
    case "BollingerBands":
      return [
        { key: "period", label: "Period", defaultValue: 20, min: 1, max: 500, step: 1 },
        { key: "std_dev", label: "Std Dev", defaultValue: 2.0, min: 0.1, max: 5, step: 0.1 },
      ];
    case "Stochastic":
      return [
        { key: "k_period", label: "%K Period", defaultValue: 14, min: 1, max: 200, step: 1 },
        { key: "d_period", label: "%D Period", defaultValue: 3, min: 1, max: 200, step: 1 },
      ];
    case "ParabolicSAR":
      return [
        { key: "acceleration_factor", label: "Accel", defaultValue: 0.02, min: 0.001, max: 0.5, step: 0.001 },
        { key: "maximum_factor", label: "Max", defaultValue: 0.2, min: 0.01, max: 1.0, step: 0.01 },
      ];
    case "VWAP":
    case "AwesomeOscillator":
    case "BarRange":
    case "Fractal":
    case "HeikenAshi":
    case "TrueRange":
    case "Pivots":
      return [];
    case "Aroon":
    case "BiggestRange":
    case "HighestInRange":
    case "LowestInRange":
    case "SmallestRange":
    case "DeMarker":
    case "Fibonacci":
    case "GannHiLo":
    case "HullMA":
    case "LinearRegression":
    case "Momentum":
    case "StdDev":
    case "Reflex":
    case "UlcerIndex":
    case "Vortex":
      return [
        { key: "period", label: "Period", defaultValue: 14, min: 1, max: 500, step: 1 },
      ];
    case "BearsPower":
    case "BullsPower":
      return [
        { key: "period", label: "Period", defaultValue: 13, min: 1, max: 500, step: 1 },
      ];
    case "Ichimoku":
      return [
        { key: "fast_period", label: "Tenkan", defaultValue: 9, min: 1, max: 200, step: 1 },
        { key: "slow_period", label: "Kijun", defaultValue: 26, min: 1, max: 200, step: 1 },
        { key: "signal_period", label: "Senkou B", defaultValue: 52, min: 1, max: 200, step: 1 },
      ];
    case "KeltnerChannel":
      return [
        { key: "period", label: "Period", defaultValue: 20, min: 1, max: 500, step: 1 },
        { key: "multiplier", label: "Mult", defaultValue: 1.5, min: 0.1, max: 10, step: 0.1 },
      ];
    case "LaguerreRSI":
      return [
        { key: "gamma", label: "Gamma", defaultValue: 0.8, min: 0.01, max: 0.99, step: 0.01 },
      ];
    case "SuperTrend":
      return [
        { key: "period", label: "Period", defaultValue: 10, min: 1, max: 500, step: 1 },
        { key: "multiplier", label: "Mult", defaultValue: 3.0, min: 0.1, max: 10, step: 0.1 },
      ];
    default:
      return [];
  }
}

/** Returns output_field options for multi-output indicators. */
export function getOutputFieldOptions(
  type: IndicatorType
): { value: string; label: string }[] | null {
  switch (type) {
    case "BollingerBands":
      return [
        { value: "upper", label: "Upper" },
        { value: "middle", label: "Middle" },
        { value: "lower", label: "Lower" },
      ];
    case "Stochastic":
      return [
        { value: "k", label: "%K" },
        { value: "d", label: "%D" },
      ];
    case "MACD":
      return [
        { value: "macd", label: "MACD Line" },
        { value: "signal", label: "Signal Line" },
        { value: "histogram", label: "Histogram" },
      ];
    case "Aroon":
      return [
        { value: "aroon_up", label: "Aroon Up" },
        { value: "aroon_down", label: "Aroon Down" },
      ];
    case "Fractal":
      return [
        { value: "fractal_up", label: "Fractal Up" },
        { value: "fractal_down", label: "Fractal Down" },
      ];
    case "HeikenAshi":
      return [
        { value: "ha_close", label: "HA Close" },
        { value: "ha_open", label: "HA Open" },
      ];
    case "Vortex":
      return [
        { value: "vi_plus", label: "VI+" },
        { value: "vi_minus", label: "VI-" },
      ];
    case "KeltnerChannel":
      return [
        { value: "upper", label: "Upper" },
        { value: "middle", label: "Middle" },
        { value: "lower", label: "Lower" },
      ];
    case "Ichimoku":
      return [
        { value: "tenkan", label: "Tenkan" },
        { value: "kijun", label: "Kijun" },
        { value: "senkou_a", label: "Senkou A" },
        { value: "senkou_b", label: "Senkou B" },
        { value: "chikou", label: "Chikou" },
      ];
    case "Fibonacci":
      return [
        { value: "level_236", label: "23.6%" },
        { value: "level_382", label: "38.2%" },
        { value: "level_500", label: "50.0%" },
        { value: "level_618", label: "61.8%" },
        { value: "level_786", label: "78.6%" },
      ];
    case "Pivots":
      return [
        { value: "pp", label: "Pivot" },
        { value: "r1", label: "R1" },
        { value: "r2", label: "R2" },
        { value: "r3", label: "R3" },
        { value: "s1", label: "S1" },
        { value: "s2", label: "S2" },
        { value: "s3", label: "S3" },
      ];
    default:
      return null;
  }
}

/** Create a default IndicatorConfig for a given type with sensible defaults. */
export function createDefaultIndicatorConfig(
  type: IndicatorType
): IndicatorConfig {
  const fields = getIndicatorParamFields(type);
  const params: IndicatorParams = {};
  for (const field of fields) {
    (params as Record<string, number>)[field.key] = field.defaultValue;
  }

  const outputFields = getOutputFieldOptions(type);
  return {
    indicator_type: type,
    params,
    output_field: outputFields ? outputFields[0].value : undefined,
  };
}

/** Create a default Price/Close operand. */
export function createDefaultOperand(): Operand {
  return {
    operand_type: "Price",
    price_field: "Close",
  };
}

/** Create a default rule with random id and default operands. */
export function createDefaultRule(): Rule {
  return {
    id: crypto.randomUUID(),
    left_operand: createDefaultOperand(),
    comparator: "GreaterThan",
    right_operand: createDefaultOperand(),
  };
}
