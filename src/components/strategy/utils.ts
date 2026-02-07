import type {
  IndicatorType,
  IndicatorConfig,
  IndicatorParams,
  Operand,
  Rule,
  Comparator,
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
