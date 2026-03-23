import type { IndicatorType, IndicatorConfig, PoolLeaf } from "@/lib/types";

export type SubTab = "config" | "blocks" | "filters";

export interface IndicatorMeta {
  type: IndicatorType;
  label: string;
  bufferCount: number;
  bufferLabels?: string[];
  defaultPeriod?: number;
  hasFastSlow?: boolean;
  hasStdDev?: boolean;
  hasKD?: boolean;
  hasAccelMax?: boolean;
  hasGamma?: boolean;
  hasMultiplier?: boolean;
  noParams?: boolean;
}

export interface PoolEntryState {
  meta: IndicatorMeta;
  periodMin: number;
  periodMax: number;
  periodStep: number;
  period: number;
  fastPeriod: number;
  slowPeriod: number;
  stdDev: number;
  kPeriod: number;
  dPeriod: number;
  accel: number;
  maxFactor: number;
  gamma: number;
  multiplier: number;
  selectedBuffers: number[];
}

export const ALL_INDICATORS: IndicatorMeta[] = [
  { type: "SMA",               label: "SMA",               bufferCount: 1, defaultPeriod: 14 },
  { type: "EMA",               label: "EMA",               bufferCount: 1, defaultPeriod: 14 },
  { type: "RSI",               label: "RSI",               bufferCount: 1, defaultPeriod: 14 },
  { type: "MACD",              label: "MACD",              bufferCount: 3, bufferLabels: ["MACD","Signal","Histogram"], hasFastSlow: true, defaultPeriod: 9 },
  { type: "BollingerBands",    label: "Bollinger Bands",   bufferCount: 3, bufferLabels: ["Upper","Middle","Lower"], defaultPeriod: 20, hasStdDev: true },
  { type: "ATR",               label: "ATR",               bufferCount: 1, defaultPeriod: 14 },
  { type: "Stochastic",        label: "Stochastic",        bufferCount: 2, bufferLabels: ["%K","%D"], hasKD: true },
  { type: "ADX",               label: "ADX",               bufferCount: 1, defaultPeriod: 14 },
  { type: "CCI",               label: "CCI",               bufferCount: 1, defaultPeriod: 14 },
  { type: "ROC",               label: "ROC",               bufferCount: 1, defaultPeriod: 14 },
  { type: "WilliamsR",         label: "Williams %R",       bufferCount: 1, defaultPeriod: 14 },
  { type: "ParabolicSAR",      label: "Parabolic SAR",     bufferCount: 1, hasAccelMax: true },
  { type: "Momentum",          label: "Momentum",          bufferCount: 1, defaultPeriod: 14 },
  { type: "StdDev",            label: "StdDev",            bufferCount: 1, defaultPeriod: 14 },
  { type: "LinearRegression",  label: "Linear Regression", bufferCount: 1, defaultPeriod: 14 },
  { type: "HullMA",            label: "Hull MA",           bufferCount: 1, defaultPeriod: 14 },
  { type: "LaguerreRSI",       label: "Laguerre RSI",      bufferCount: 1, hasGamma: true },
  { type: "SuperTrend",        label: "SuperTrend",        bufferCount: 1, defaultPeriod: 14, hasMultiplier: true },
  { type: "KeltnerChannel",    label: "Keltner Channel",   bufferCount: 3, bufferLabels: ["Upper","Middle","Lower"], defaultPeriod: 20, hasMultiplier: true },
  { type: "BearsPower",        label: "Bears Power",       bufferCount: 1, defaultPeriod: 13 },
  { type: "BullsPower",        label: "Bulls Power",       bufferCount: 1, defaultPeriod: 13 },
  { type: "DeMarker",          label: "DeMarker",          bufferCount: 1, defaultPeriod: 14 },
  { type: "AwesomeOscillator", label: "Awesome Osc.",      bufferCount: 1, noParams: true },
  { type: "BarRange",          label: "Bar Range",         bufferCount: 1, noParams: true },
  { type: "TrueRange",         label: "True Range",        bufferCount: 1, noParams: true },
  { type: "Aroon",             label: "Aroon",             bufferCount: 2, bufferLabels: ["Up","Down"], defaultPeriod: 14 },
  { type: "UlcerIndex",        label: "Ulcer Index",       bufferCount: 1, defaultPeriod: 14 },
  { type: "Vortex",            label: "Vortex",            bufferCount: 2, bufferLabels: ["VI+","VI-"], defaultPeriod: 14 },
];

export function makeEntry(meta: IndicatorMeta): PoolEntryState {
  const dp = meta.defaultPeriod ?? 14;
  return {
    meta,
    periodMin: 5,
    periodMax: dp * 3,
    periodStep: 5,
    period: dp,
    fastPeriod: 12,
    slowPeriod: 26,
    stdDev: 2.0,
    kPeriod: 14,
    dPeriod: 3,
    accel: 0.02,
    maxFactor: 0.2,
    gamma: 0.5,
    multiplier: 2.0,
    selectedBuffers: [0],
  };
}

export function hasPeriodRange(meta: IndicatorMeta): boolean {
  return (
    !meta.noParams &&
    !meta.hasFastSlow &&
    !meta.hasKD &&
    !meta.hasAccelMax &&
    !meta.hasGamma &&
    meta.defaultPeriod !== undefined
  );
}

export function entryToPoolLeaves(e: PoolEntryState): PoolLeaf[] {
  const params: IndicatorConfig["params"] = {};
  if (!e.meta.noParams) {
    if (e.meta.hasFastSlow) {
      params.fast_period = e.fastPeriod;
      params.slow_period = e.slowPeriod;
      params.signal_period = e.period;
    } else if (e.meta.hasKD) {
      params.k_period = e.kPeriod;
      params.d_period = e.dPeriod;
    } else if (e.meta.hasAccelMax) {
      params.acceleration_factor = e.accel;
      params.maximum_factor = e.maxFactor;
    } else if (e.meta.hasGamma) {
      params.gamma = e.gamma;
    } else if (e.meta.defaultPeriod !== undefined) {
      params.period = e.periodMin;
    }
    if (e.meta.hasStdDev) params.std_dev = e.stdDev;
    if (e.meta.hasMultiplier) params.multiplier = e.multiplier;
  }
  const config: IndicatorConfig = { indicator_type: e.meta.type, params };
  const usesRange = hasPeriodRange(e.meta);
  return e.selectedBuffers.map((buf) => ({
    config,
    buffer_index: buf,
    ...(usesRange ? { period_min: e.periodMin, period_max: e.periodMax, period_step: e.periodStep } : {}),
  }));
}
