/// SR Builder configuration tab.

import { useState } from "react";
import { useAppStore } from "@/stores/useAppStore";
import { listen } from "@tauri-apps/api/event";
import { runSrBuilder, cancelSrBuilder } from "@/lib/tauri";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/Select";
import { DatePicker } from "@/components/ui/DatePicker";
import type {
  IndicatorType,
  IndicatorConfig,
  PoolLeaf,
  SrConfig,
  SrProgressEvent,
  Timeframe,
  TradeDirection,
} from "@/lib/types";
import { cn } from "@/lib/utils";

// ── Indicator metadata ─────────────────────────────────────────────────────────

interface IndicatorMeta {
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

const ALL_INDICATORS: IndicatorMeta[] = [
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
  { type: "VWAP",              label: "VWAP",              bufferCount: 1, noParams: true },
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

// ── Pool entry state ──────────────────────────────────────────────────────────

interface PoolEntryState {
  meta: IndicatorMeta;
  /** Period range for simple single-period indicators (expanded by backend). */
  periodMin: number;
  periodMax: number;
  periodStep: number;
  /** Fixed signal period for MACD. */
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

function makeEntry(meta: IndicatorMeta): PoolEntryState {
  const dp = meta.defaultPeriod ?? 14;
  return {
    meta,
    periodMin: 5,
    periodMax: dp * 3,
    periodStep: 5,
    period: dp,        // MACD signal period (fixed)
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

/** Returns true for indicators whose main tunable param is a simple integer period. */
function hasPeriodRange(meta: IndicatorMeta): boolean {
  return (
    !meta.noParams &&
    !meta.hasFastSlow &&
    !meta.hasKD &&
    !meta.hasAccelMax &&
    !meta.hasGamma &&
    meta.defaultPeriod !== undefined
  );
}

function entryToPoolLeaves(e: PoolEntryState): PoolLeaf[] {
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
      // Use periodMin as the template period; backend expands the range
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

// ── Sub-components ─────────────────────────────────────────────────────────────

function Num({
  label,
  value,
  min,
  max,
  step,
  onChange,
}: {
  label: string;
  value: number;
  min?: number;
  max?: number;
  step?: number;
  onChange: (v: number) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-xs">
      <span className="text-muted-foreground w-28 shrink-0">{label}</span>
      <input
        type="number"
        value={value}
        min={min}
        max={max}
        step={step ?? 1}
        className="w-20 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-foreground focus:outline-none focus:border-primary/60"
        onChange={(e) => { const v = parseFloat(e.target.value); if (!isNaN(v)) onChange(v); }}
      />
    </label>
  );
}

function Section({ title, children, actions }: { title: string; children: React.ReactNode; actions?: React.ReactNode }) {
  return (
    <div className="rounded border border-border/30 bg-card/40 p-3">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/60">{title}</span>
        {actions}
      </div>
      <div className="space-y-2">{children}</div>
    </div>
  );
}

// ── Main SrTab ────────────────────────────────────────────────────────────────

const TIMEFRAMES: Timeframe[] = ["m1", "m5", "m15", "m30", "h1", "h4", "d1"];

export function SrTab() {
  const {
    symbols,
    srRunning, setSrRunning,
    srProgress, setSrProgress,
    setSrResults,
    setSrLastConfig,
    builderConfig,
  } = useAppStore();

  // ── Symbol / data ──
  const [symbolId, setSymbolId] = useState<string>(builderConfig.dataConfig.symbolId ?? "");
  const [timeframe, setTimeframe] = useState<Timeframe>(builderConfig.dataConfig.timeframe ?? "h1");
  const [startDate, setStartDate] = useState(builderConfig.dataConfig.startDate ?? "");
  const [endDate, setEndDate] = useState(builderConfig.dataConfig.endDate ?? "");
  const [initialCapital, setInitialCapital] = useState(10000);

  const symbol = symbols.find((s) => s.id === symbolId);
  const availableTimeframes = symbol
    ? TIMEFRAMES.filter((tf) => tf in (symbol.timeframe_paths ?? {}))
    : TIMEFRAMES;

  // ── Pool state ──
  const [activePool, setActivePool] = useState<PoolEntryState[]>([]);
  const [checkedTypes, setCheckedTypes] = useState<Set<IndicatorType>>(new Set());

  // ── Stopping conditions ──
  const [databankLimit, setDatabankLimit] = useState(50);
  const [maxTradesPerDay, setMaxTradesPerDay] = useState(0); // 0 = no limit
  const [tradeDirection, setTradeDirection] = useState<TradeDirection>("Both");

  // ── NSGA-II params ──
  const [populationSize, setPopulationSize] = useState(150);
  const [generations, setGenerations] = useState(0); // 0 = unlimited, stop via databank
  const [crossoverRate, setCrossoverRate] = useState(0.7);
  const [mutationRate, setMutationRate] = useState(0.3);

  // ── Tree + SR ──
  const [maxDepth, setMaxDepth] = useState(5);
  const [minTrades, setMinTrades] = useState(20);

  // ── CMA-ES ──
  const [cmaesTopK, setCmaesTopK] = useState(10);
  const [cmaesIterations, setCmaesIterations] = useState(200);

  // ── Pool handlers ──
  const toggleIndicator = (meta: IndicatorMeta) => {
    setCheckedTypes((prev) => {
      const next = new Set(prev);
      if (next.has(meta.type)) {
        next.delete(meta.type);
        setActivePool((p) => p.filter((e) => e.meta.type !== meta.type));
      } else {
        next.add(meta.type);
        setActivePool((p) => [...p, makeEntry(meta)]);
      }
      return next;
    });
  };

  const selectAll = () => {
    setCheckedTypes(new Set(ALL_INDICATORS.map((m) => m.type)));
    setActivePool(ALL_INDICATORS.map(makeEntry));
  };

  const deselectAll = () => {
    setCheckedTypes(new Set());
    setActivePool([]);
  };

  const updateEntry = (type: IndicatorType, patch: Partial<PoolEntryState>) =>
    setActivePool((p) => p.map((e) => (e.meta.type === type ? { ...e, ...patch } : e)));

  const toggleBuffer = (type: IndicatorType, buf: number) => {
    setActivePool((p) =>
      p.map((e) => {
        if (e.meta.type !== type) return e;
        const has = e.selectedBuffers.includes(buf);
        const next = has ? e.selectedBuffers.filter((b) => b !== buf) : [...e.selectedBuffers, buf];
        return { ...e, selectedBuffers: next.length === 0 ? [buf] : next };
      })
    );
  };

  // ── Run / Stop ──
  const handleRun = async () => {
    if (!symbol) return;
    const pool: PoolLeaf[] = activePool.flatMap(entryToPoolLeaves);
    if (pool.length === 0) { alert("Agrega al menos un indicador al pool."); return; }

    setSrRunning(true);
    setSrProgress(null);
    setSrResults([]);
    setSrLastConfig({ symbolId: symbol.id, timeframe, startDate, endDate, initialCapital });

    const config: SrConfig = {
      pool,
      population_size: populationSize,
      generations,
      max_depth: maxDepth,
      min_trades: minTrades,
      cmaes_top_k: cmaesTopK,
      cmaes_iterations: cmaesIterations,
      crossover_rate: crossoverRate,
      mutation_rate: mutationRate,
      databank_limit: databankLimit,
      max_trades_per_day: maxTradesPerDay > 0 ? maxTradesPerDay : undefined,
      symbol_id: symbol.id,
      timeframe,
      start_date: startDate,
      end_date: endDate,
      initial_capital: initialCapital,
      leverage: 1.0,
      position_sizing: { sizing_type: "FixedLots", value: 1.0 },
      stop_loss: undefined,
      take_profit: undefined,
      trailing_stop: undefined,
      trading_costs: {
        spread_pips: builderConfig.dataConfig.spreadPips ?? 1.0,
        commission_type: "FixedPerLot",
        commission_value: 0,
        slippage_pips: builderConfig.dataConfig.slippagePips ?? 0,
        slippage_random: false,
      },
      trade_direction: tradeDirection,
    };

    const unlisten = await listen<SrProgressEvent>("sr-progress", (event) => {
      const ev = event.payload;
      if (ev.type === "Generation") {
        setSrProgress({ phase: "generation", ...ev.data });
      } else if (ev.type === "CmaesProgress") {
        setSrProgress({ phase: "cmaes", ...ev.data });
      } else if (ev.type === "Done") {
        setSrResults(ev.data.front);
        setSrRunning(false);
        setSrProgress(null);
        unlisten();
      } else if (ev.type === "Error") {
        console.error("SR error:", ev.data);
        setSrRunning(false);
        setSrProgress(null);
        unlisten();
      }
    });

    try {
      await runSrBuilder(config);
    } catch (e) {
      console.error(e);
      setSrRunning(false);
      unlisten();
    }
  };

  const handleStop = async () => {
    await cancelSrBuilder();
    setSrRunning(false);
    setSrProgress(null);
  };

  return (
    <div className="space-y-4 p-4">

      {/* Symbol / data config */}
      <Section title="Símbolo y Datos">
        <div className="space-y-2.5">

          {/* Symbol */}
          <div className="flex items-center gap-2">
            <span className="w-28 shrink-0 text-xs text-muted-foreground">Símbolo</span>
            <Select value={symbolId} onValueChange={(v) => { setSymbolId(v); setStartDate(""); setEndDate(""); }}>
              <SelectTrigger className="h-8 flex-1 text-xs">
                <SelectValue placeholder="— Seleccionar símbolo —" />
              </SelectTrigger>
              <SelectContent>
                {symbols.map((s) => (
                  <SelectItem key={s.id} value={s.id}>{s.name}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Timeframe */}
          <div className="flex items-center gap-2">
            <span className="w-28 shrink-0 text-xs text-muted-foreground">Timeframe</span>
            <Select value={timeframe} onValueChange={(v) => setTimeframe(v as Timeframe)}>
              <SelectTrigger className="h-8 flex-1 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {availableTimeframes.map((tf) => (
                  <SelectItem key={tf} value={tf}>{tf.toUpperCase()}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {/* Date range */}
          <div className="flex items-center gap-2">
            <span className="w-28 shrink-0 text-xs text-muted-foreground">Desde</span>
            <div className="flex-1">
              <DatePicker value={startDate} onChange={setStartDate} className="h-8 text-xs" />
            </div>
          </div>
          <div className="flex items-center gap-2">
            <span className="w-28 shrink-0 text-xs text-muted-foreground">Hasta</span>
            <div className="flex-1">
              <DatePicker value={endDate} onChange={setEndDate} className="h-8 text-xs" />
            </div>
          </div>

          {symbol && (
            <p className="text-[10px] text-muted-foreground/50">
              Datos disponibles: {symbol.start_date} → {symbol.end_date}
            </p>
          )}

          <Num label="Capital inicial $" value={initialCapital} min={100} step={1000} onChange={setInitialCapital} />
        </div>
      </Section>

      {/* Indicator pool */}
      <Section
        title={`Pool de indicadores (${checkedTypes.size} / ${ALL_INDICATORS.length})`}
        actions={
          <div className="flex gap-1">
            <button
              onClick={selectAll}
              className="rounded border border-border/40 px-2 py-0.5 text-[10px] text-muted-foreground hover:border-primary/50 hover:text-primary transition-colors"
            >
              Todos
            </button>
            <button
              onClick={deselectAll}
              className="rounded border border-border/40 px-2 py-0.5 text-[10px] text-muted-foreground hover:border-destructive/50 hover:text-destructive transition-colors"
            >
              Ninguno
            </button>
          </div>
        }
      >
        <p className="text-[10px] text-muted-foreground/60 mb-2">
          Los indicadores seleccionados son el vocabulario que SR puede usar. Configurá el rango de períodos — el backend expande cada indicador en múltiples variantes (ej. SMA 5, 10, 15…) que SR elige libremente.
        </p>
        <div className="space-y-1.5">
          {ALL_INDICATORS.map((meta) => {
            const checked = checkedTypes.has(meta.type);
            const entry = activePool.find((e) => e.meta.type === meta.type);
            return (
              <div key={meta.type} className="rounded border border-border/20 bg-background/40">
                <label className="flex cursor-pointer items-center gap-2 px-3 py-1.5">
                  <input
                    type="checkbox"
                    checked={checked}
                    onChange={() => toggleIndicator(meta)}
                    className="accent-primary"
                  />
                  <span className="text-xs font-medium">{meta.label}</span>
                  {meta.noParams && (
                    <span className="ml-auto text-[10px] text-muted-foreground/40">sin parámetros</span>
                  )}
                </label>
                {checked && entry && (
                  <div className="border-t border-border/20 px-3 pb-2 pt-1 space-y-1.5">
                    {!meta.noParams && meta.hasFastSlow && (
                      <>
                        <Num label="Fast period" value={entry.fastPeriod} min={2} onChange={(v) => updateEntry(meta.type, { fastPeriod: v })} />
                        <Num label="Slow period" value={entry.slowPeriod} min={2} onChange={(v) => updateEntry(meta.type, { slowPeriod: v })} />
                        <Num label="Signal period" value={entry.period} min={2} onChange={(v) => updateEntry(meta.type, { period: v })} />
                      </>
                    )}
                    {!meta.noParams && meta.hasKD && (
                      <>
                        <Num label="%K period" value={entry.kPeriod} min={1} onChange={(v) => updateEntry(meta.type, { kPeriod: v })} />
                        <Num label="%D period" value={entry.dPeriod} min={1} onChange={(v) => updateEntry(meta.type, { dPeriod: v })} />
                      </>
                    )}
                    {!meta.noParams && meta.hasAccelMax && (
                      <>
                        <Num label="Accel. factor" value={entry.accel} min={0.001} step={0.01} onChange={(v) => updateEntry(meta.type, { accel: v })} />
                        <Num label="Max factor" value={entry.maxFactor} min={0.01} step={0.01} onChange={(v) => updateEntry(meta.type, { maxFactor: v })} />
                      </>
                    )}
                    {!meta.noParams && meta.hasGamma && (
                      <Num label="Gamma" value={entry.gamma} min={0.01} max={1} step={0.05} onChange={(v) => updateEntry(meta.type, { gamma: v })} />
                    )}
                    {hasPeriodRange(meta) && (
                      <div className="flex items-center gap-2 text-xs">
                        <span className="shrink-0 text-muted-foreground">Período</span>
                        <div className="flex items-center gap-1">
                          <span className="text-[10px] text-muted-foreground/50">mín</span>
                          <input type="number" value={entry.periodMin} min={2} step={1}
                            className="w-14 rounded border border-border/40 bg-background px-1.5 py-0.5 text-xs text-foreground focus:outline-none focus:border-primary/60"
                            onChange={(ev) => { const v = parseInt(ev.target.value); if (!isNaN(v) && v >= 2) updateEntry(meta.type, { periodMin: v }); }} />
                        </div>
                        <div className="flex items-center gap-1">
                          <span className="text-[10px] text-muted-foreground/50">máx</span>
                          <input type="number" value={entry.periodMax} min={2} step={5}
                            className="w-14 rounded border border-border/40 bg-background px-1.5 py-0.5 text-xs text-foreground focus:outline-none focus:border-primary/60"
                            onChange={(ev) => { const v = parseInt(ev.target.value); if (!isNaN(v) && v >= 2) updateEntry(meta.type, { periodMax: v }); }} />
                        </div>
                        <div className="flex items-center gap-1">
                          <span className="text-[10px] text-muted-foreground/50">paso</span>
                          <input type="number" value={entry.periodStep} min={1} step={1}
                            className="w-12 rounded border border-border/40 bg-background px-1.5 py-0.5 text-xs text-foreground focus:outline-none focus:border-primary/60"
                            onChange={(ev) => { const v = parseInt(ev.target.value); if (!isNaN(v) && v >= 1) updateEntry(meta.type, { periodStep: v }); }} />
                        </div>
                      </div>
                    )}
                    {meta.hasStdDev && (
                      <Num label="Std dev" value={entry.stdDev} min={0.5} step={0.5} onChange={(v) => updateEntry(meta.type, { stdDev: v })} />
                    )}
                    {meta.hasMultiplier && (
                      <Num label="Multiplicador" value={entry.multiplier} min={0.1} step={0.5} onChange={(v) => updateEntry(meta.type, { multiplier: v })} />
                    )}
                    {/* Buffer selection (only for multi-buffer indicators) */}
                    {meta.bufferCount > 1 && (
                      <div className="flex gap-3 flex-wrap mt-1">
                        {Array.from({ length: meta.bufferCount }, (_, i) => (
                          <label key={i} className="flex items-center gap-1 text-[10px] text-muted-foreground cursor-pointer">
                            <input
                              type="checkbox"
                              checked={entry.selectedBuffers.includes(i)}
                              onChange={() => toggleBuffer(meta.type, i)}
                              className="accent-primary"
                            />
                            {meta.bufferLabels?.[i] ?? `Buffer ${i}`}
                          </label>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </Section>

      {/* Stopping conditions */}
      <Section title="Objetivo y Límites">
        <Num label="Databank limit" value={databankLimit} min={1} max={1000} step={10} onChange={setDatabankLimit} />
        <Num label="Trades/día máx." value={maxTradesPerDay} min={0} step={1} onChange={setMaxTradesPerDay} />
        <div className="flex items-center gap-2 text-xs">
          <span className="text-muted-foreground w-28 shrink-0">Dirección</span>
          <Select value={tradeDirection} onValueChange={(v) => setTradeDirection(v as TradeDirection)}>
            <SelectTrigger className="h-7 flex-1 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="Both">Long y Short</SelectItem>
              <SelectItem value="Long">Solo Long</SelectItem>
              <SelectItem value="Short">Solo Short</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <p className="text-[10px] text-muted-foreground/50">
          0 en Trades/día = sin límite.
        </p>
      </Section>

      {/* NSGA-II params */}
      <Section title="Evolución NSGA-II">
        <Num label="Población" value={populationSize} min={20} max={2000} step={10} onChange={setPopulationSize} />
        <Num label="Max generaciones" value={generations} min={0} max={5000} step={50} onChange={setGenerations} />
        <Num label="Tasa cruce" value={crossoverRate} min={0} max={1} step={0.05} onChange={setCrossoverRate} />
        <Num label="Tasa mutación" value={mutationRate} min={0} max={1} step={0.05} onChange={setMutationRate} />
        <p className="text-[10px] text-muted-foreground/50">Max generaciones = 0 detiene solo por Databank limit.</p>
      </Section>

      {/* Tree params */}
      <Section title="Árbol de Fórmulas">
        <Num label="Profundidad máx." value={maxDepth} min={2} max={10} step={1} onChange={setMaxDepth} />
        <Num label="Trades mínimos" value={minTrades} min={1} step={5} onChange={setMinTrades} />
      </Section>

      {/* CMA-ES */}
      <Section title="CMA-ES — Refinamiento de Constantes">
        <Num label="Refinar top-K" value={cmaesTopK} min={0} max={50} onChange={setCmaesTopK} />
        <Num label="Evaluaciones máx." value={cmaesIterations} min={50} max={2000} step={50} onChange={setCmaesIterations} />
      </Section>

      {/* Progress */}
      {srRunning && srProgress && (
        <div className="rounded border border-primary/20 bg-primary/5 px-3 py-2 text-xs">
          <div className="flex items-center justify-between gap-3">
            {srProgress.phase === "generation" ? (
              <span className="text-muted-foreground">
                Gen {srProgress.gen} · Databank: {srProgress.databank_count}/{srProgress.databank_limit} · Pareto: {srProgress.pareto_size} · Sharpe: {srProgress.best_sharpe.toFixed(2)}
              </span>
            ) : (
              <span className="text-muted-foreground">
                Refinando constantes {srProgress.current}/{srProgress.total}…
              </span>
            )}
            <div className="h-1.5 w-32 shrink-0 rounded-full bg-border/40 overflow-hidden">
              <div
                className="h-full rounded-full bg-primary transition-all"
                style={{
                  width: srProgress.phase === "generation"
                    ? `${Math.min(100, (srProgress.databank_count / Math.max(1, srProgress.databank_limit)) * 100)}%`
                    : `${(srProgress.current / Math.max(1, srProgress.total)) * 100}%`,
                }}
              />
            </div>
          </div>
        </div>
      )}

      {/* Run / Stop button */}
      <div className="flex justify-end pb-2">
        {srRunning ? (
          <button
            onClick={handleStop}
            className="rounded bg-destructive/80 px-4 py-1.5 text-xs font-medium text-white hover:bg-destructive"
          >
            Detener SR Builder
          </button>
        ) : (
          <button
            disabled={!symbol || activePool.length === 0}
            onClick={handleRun}
            className={cn(
              "rounded px-4 py-1.5 text-xs font-medium text-white transition-colors",
              !symbol || activePool.length === 0
                ? "bg-muted/40 text-muted-foreground cursor-not-allowed"
                : "bg-primary hover:bg-primary/80"
            )}
          >
            Iniciar SR Builder
          </button>
        )}
      </div>

    </div>
  );
}
