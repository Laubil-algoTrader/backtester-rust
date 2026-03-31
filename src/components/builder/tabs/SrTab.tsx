/// SR Builder configuration tab — split into 3 sub-tabs.

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
  SrConfig,
  SrProgressEvent,
  SrFrontItem,
  Timeframe,
  TradeDirection,
  BuilderSavedStrategy,
} from "@/lib/types";
import {
  ALL_INDICATORS,
  makeEntry,
  hasPeriodRange,
  entryToPoolLeaves,
} from "@/lib/srBuilderTypes";
import type { IndicatorMeta, PoolEntryState } from "@/lib/srBuilderTypes";
import { cn } from "@/lib/utils";
import { useState } from "react";
import { Save, X } from "lucide-react";

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
      <span className="text-muted-foreground w-36 shrink-0">{label}</span>
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

function TabBtn({ label, active, onClick }: { label: string; active: boolean; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "px-4 py-2 text-xs font-medium transition-colors border-b-2 -mb-px whitespace-nowrap",
        active
          ? "border-primary text-foreground"
          : "border-transparent text-muted-foreground hover:text-foreground hover:border-border/60"
      )}
    >
      {label}
    </button>
  );
}

// ── Main SrTab ────────────────────────────────────────────────────────────────

const TIMEFRAMES: Timeframe[] = ["m1", "m5", "m15", "m30", "h1", "h4", "d1"];

export function SrTab() {
  const {
    symbols,
    srRunning, setSrRunning,
    setSrProgress,
    setSrResults,
    setSrLastConfig,
    builderConfig,
    setBuilderTopTab,
    srBuilderConfig,
    updateSrBuilderConfig,
    addToBuilderDatabankById,
    clearBuilderDatabank,
    srSavedConfigs,
    saveSrConfig,
    loadSrConfig,
    deleteSrConfig,
  } = useAppStore();

  const [presetName, setPresetName] = useState("");

  // Destructure all config fields for convenient access
  const {
    activeTab, symbolId, timeframe, startDate, endDate, initialCapital,
    activePool, checkedTypes,
    databankLimit, maxTradesPerDay, tradeDirection,
    positionSizingType, positionSizingValue,
    tradingHoursEnabled, tradingHoursStartHour, tradingHoursStartMinute,
    tradingHoursEndHour, tradingHoursEndMinute,
    closeAtTimeEnabled, closeAtTimeHour, closeAtTimeMinute,
    slEnabled, slType, slValue, slAtrPeriod,
    slAtrPeriodMin, slAtrPeriodMax, slAtrMultMin, slAtrMultMax,
    tpEnabled, tpType, tpValue, tpAtrPeriod,
    tpAtrPeriodMin, tpAtrPeriodMax, tpAtrMultMin, tpAtrMultMax,
    tsEnabled, tsType, tsValue, tsAtrPeriod,
    useExitFormula,
    spreadPips, commissionType, commissionValue,
    populationSize, generations, crossoverRate, mutationRate,
    maxDepth, minTrades,
    cmaesTopK, cmaesIterations,
    initMinSharpe, initMinPF, initMaxDD,
    finalMinSharpe, finalMinPF, finalMinTrades, finalMaxDD,
  } = srBuilderConfig;

  const u = updateSrBuilderConfig;

  const symbol = symbols.find((s) => s.id === symbolId);
  const availableTimeframes = symbol
    ? TIMEFRAMES.filter((tf) => tf in (symbol.timeframe_paths ?? {}))
    : TIMEFRAMES;

  // ── Pool handlers ──
  const toggleIndicator = (meta: IndicatorMeta) => {
    const checked = checkedTypes.includes(meta.type);
    if (checked) {
      u({
        checkedTypes: checkedTypes.filter((t) => t !== meta.type),
        activePool: activePool.filter((e) => e.meta.type !== meta.type),
      });
    } else {
      u({
        checkedTypes: [...checkedTypes, meta.type],
        activePool: [...activePool, makeEntry(meta)],
      });
    }
  };

  const selectAll = () => {
    u({
      checkedTypes: ALL_INDICATORS.map((m) => m.type),
      activePool: ALL_INDICATORS.map(makeEntry),
    });
  };

  const deselectAll = () => {
    u({ checkedTypes: [], activePool: [] });
  };

  const updateEntry = (type: IndicatorType, patch: Partial<PoolEntryState>) =>
    u({ activePool: activePool.map((e) => (e.meta.type === type ? { ...e, ...patch } : e)) });

  const toggleBuffer = (type: IndicatorType, buf: number) => {
    u({
      activePool: activePool.map((e) => {
        if (e.meta.type !== type) return e;
        const has = e.selectedBuffers.includes(buf);
        const next = has ? e.selectedBuffers.filter((b) => b !== buf) : [...e.selectedBuffers, buf];
        return { ...e, selectedBuffers: next.length === 0 ? [buf] : next };
      }),
    });
  };

  // ── SR front item → BuilderSavedStrategy conversion ──
  const srFrontItemToSaved = (item: SrFrontItem, sid: string, sName: string): BuilderSavedStrategy => {
    const { objectives: o, metrics: m } = item;
    return {
      id: crypto.randomUUID(),
      name: `SR Rank ${item.rank} — Sharpe ${o.sharpe.toFixed(2)}`,
      createdAt: new Date().toISOString(),
      fitness: o.sharpe,
      symbolId: sid,
      symbolName: sName,
      timeframe,
      netProfit: m.net_profit,
      miniEquityCurve: [],
      trades: m.total_trades,
      profitFactor: o.profit_factor,
      sharpeRatio: o.sharpe,
      rExpectancy: o.expectancy_ratio,
      annualReturnPct: m.annualized_return_pct,
      maxDrawdownAbs: m.max_drawdown_pct,
      winRatePct: m.win_rate_pct,
      winLossRatio: m.avg_loss !== 0 ? Math.abs(m.avg_win / m.avg_loss) : 0,
      retDDRatio: m.max_drawdown_pct > 0 ? m.annualized_return_pct / m.max_drawdown_pct : 0,
      cagrMaxDDPct: 0,
      avgWin: m.avg_win,
      avgLoss: m.avg_loss,
      avgBarsWin: m.avg_bars_in_trade,
      strategyJson: JSON.stringify(item.strategy),
      startDate: srBuilderConfig.startDate,
      endDate: srBuilderConfig.endDate,
      initialCapital: srBuilderConfig.initialCapital,
      isSr: true,
    };
  };

  // ── Run / Stop ──
  const handleRun = async () => {
    if (!symbol) return;
    const pool = activePool.flatMap(entryToPoolLeaves);
    if (pool.length === 0) { alert("Agrega al menos un indicador al pool."); return; }

    setSrRunning(true);
    setSrProgress(null);
    setSrResults([]);
    clearBuilderDatabank("builder");
    clearBuilderDatabank("results");
    setBuilderTopTab("progress");
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
      trading_hours: tradingHoursEnabled ? {
        start_hour: tradingHoursStartHour,
        start_minute: tradingHoursStartMinute,
        end_hour: tradingHoursEndHour,
        end_minute: tradingHoursEndMinute,
      } : undefined,
      close_trades_at: closeAtTimeEnabled ? {
        hour: closeAtTimeHour,
        minute: closeAtTimeMinute,
      } : undefined,
      symbol_id: symbol.id,
      timeframe,
      start_date: startDate,
      end_date: endDate,
      initial_capital: initialCapital,
      leverage: 1.0,
      position_sizing: { sizing_type: positionSizingType, value: positionSizingValue },
      stop_loss: slEnabled ? { sl_type: slType, value: slType === "ATR" ? slAtrMultMin : slValue, ...(slType === "ATR" ? { atr_period: slAtrPeriodMin } : {}) } : undefined,
      sl_atr_range: slEnabled && slType === "ATR" ? { period_min: slAtrPeriodMin, period_max: slAtrPeriodMax, mult_min: slAtrMultMin, mult_max: slAtrMultMax } : undefined,
      take_profit: tpEnabled ? { tp_type: tpType, value: tpType === "ATR" ? tpAtrMultMin : tpValue, ...(tpType === "ATR" ? { atr_period: tpAtrPeriodMin } : {}) } : undefined,
      tp_atr_range: tpEnabled && tpType === "ATR" ? { period_min: tpAtrPeriodMin, period_max: tpAtrPeriodMax, mult_min: tpAtrMultMin, mult_max: tpAtrMultMax } : undefined,
      trailing_stop: tsEnabled ? { ts_type: tsType, value: tsValue, ...(tsType === "ATR" ? { atr_period: tsAtrPeriod } : {}) } : undefined,
      trading_costs: {
        spread_pips: spreadPips,
        commission_type: commissionType,
        commission_value: commissionValue,
        slippage_pips: 0,
        slippage_random: false,
      },
      trade_direction: tradeDirection,
      initial_min_sharpe: initMinSharpe > 0 ? initMinSharpe : undefined,
      initial_min_profit_factor: initMinPF > 0 ? initMinPF : undefined,
      initial_max_drawdown_pct: initMaxDD > 0 ? initMaxDD : undefined,
      final_min_sharpe: finalMinSharpe > 0 ? finalMinSharpe : undefined,
      final_min_profit_factor: finalMinPF > 0 ? finalMinPF : undefined,
      final_min_trades: finalMinTrades > 0 ? finalMinTrades : undefined,
      final_max_drawdown_pct: finalMaxDD > 0 ? finalMaxDD : undefined,
      use_exit_formula: useExitFormula,
    };

    const symName = symbol.name;
    const symId = symbol.id;

    const unlisten = await listen<SrProgressEvent>("sr-progress", (event) => {
      const ev = event.payload;
      if (ev.type === "Generation") {
        setSrProgress({ phase: "generation", ...ev.data });
      } else if (ev.type === "CmaesProgress") {
        setSrProgress({ phase: "cmaes", ...ev.data });
      } else if (ev.type === "NsgaDone") {
        // Save NSGA-II pre-refinement strategies to "builder" databank
        ev.data.front.forEach((item) => {
          addToBuilderDatabankById("builder", srFrontItemToSaved(item, symId, symName));
        });
      } else if (ev.type === "Done") {
        // Save CMA-ES-refined strategies to "results" databank
        ev.data.front.forEach((item) => {
          addToBuilderDatabankById("results", srFrontItemToSaved(item, symId, symName));
        });
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

  // ── Render ────────────────────────────────────────────────────────────────────

  return (
    <div className="flex flex-col h-full">

      {/* Sub-tab navigation */}
      <div className="shrink-0 flex gap-0 px-3 border-b border-border/30">
        <TabBtn label="Configuración" active={activeTab === "config"} onClick={() => u({ activeTab: "config" })} />
        <TabBtn label="Bloques de Construcción" active={activeTab === "blocks"} onClick={() => u({ activeTab: "blocks" })} />
        <TabBtn label="Filtros" active={activeTab === "filters"} onClick={() => u({ activeTab: "filters" })} />
      </div>

      {/* ── Persistent presets bar ─────────────────────────────────────────── */}
      <div className="shrink-0 flex items-center gap-2 border-b border-border/20 bg-muted/5 px-3 py-1.5">
        <span className="shrink-0 text-[10px] uppercase tracking-widest text-muted-foreground/50">
          Presets
        </span>

        {/* Saved presets list (horizontal scroll) */}
        <div className="flex flex-1 items-center gap-1 overflow-x-auto">
          {srSavedConfigs.length === 0 ? (
            <span className="text-[10px] italic text-muted-foreground/30">Sin presets guardados</span>
          ) : (
            srSavedConfigs.map((preset) => (
              <div
                key={preset.id}
                className="group flex shrink-0 items-center gap-0.5 rounded border border-border/30 bg-background px-2 py-0.5"
              >
                <button
                  onClick={() => loadSrConfig(preset.id)}
                  title={`Cargar "${preset.name}"`}
                  className="max-w-[120px] truncate text-[11px] font-medium text-foreground hover:text-primary transition-colors"
                >
                  {preset.name}
                </button>
                <button
                  onClick={() => deleteSrConfig(preset.id)}
                  title="Eliminar preset"
                  className="ml-0.5 text-muted-foreground/30 hover:text-destructive transition-colors opacity-0 group-hover:opacity-100"
                >
                  <X className="h-2.5 w-2.5" />
                </button>
              </div>
            ))
          )}
        </div>

        {/* Save new preset */}
        <div className="flex shrink-0 items-center gap-1">
          <input
            type="text"
            value={presetName}
            onChange={(e) => setPresetName(e.target.value)}
            placeholder="Nombre…"
            className="h-6 w-32 rounded border border-border/40 bg-background px-2 text-[11px] text-foreground placeholder:text-muted-foreground/35 focus:outline-none focus:border-primary/60"
            onKeyDown={(e) => {
              if (e.key === "Enter" && presetName.trim()) {
                saveSrConfig(presetName);
                setPresetName("");
              }
            }}
          />
          <button
            onClick={() => { if (presetName.trim()) { saveSrConfig(presetName); setPresetName(""); } }}
            disabled={!presetName.trim()}
            className="flex items-center gap-1 rounded border border-border/40 px-2 py-1 text-[10px] text-muted-foreground hover:border-primary/50 hover:text-primary disabled:opacity-40 transition-colors"
          >
            <Save className="h-3 w-3" />
            Guardar
          </button>
        </div>
      </div>

      {/* Scrollable content area */}
      <div className="flex-1 overflow-auto">

        {/* ── TAB: Configuración ──────────────────────────────────────────────── */}
        {activeTab === "config" && (
          <div className="space-y-4 p-4">

            <Section title="Símbolo y Datos">
              <div className="space-y-2.5">
                <div className="flex items-center gap-2">
                  <span className="w-36 shrink-0 text-xs text-muted-foreground">Símbolo</span>
                  <Select value={symbolId} onValueChange={(v) => {
                      const sym = symbols.find((s) => s.id === v);
                      u({
                        symbolId: v,
                        startDate: sym?.start_date?.slice(0, 10) ?? "",
                        endDate: sym?.end_date?.slice(0, 10) ?? "",
                      });
                    }}>
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

                <div className="flex items-center gap-2">
                  <span className="w-36 shrink-0 text-xs text-muted-foreground">Timeframe</span>
                  <Select value={timeframe} onValueChange={(v) => u({ timeframe: v as Timeframe })}>
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

                <div className="flex items-center gap-2">
                  <span className="w-36 shrink-0 text-xs text-muted-foreground">Desde</span>
                  <div className="flex-1">
                    <DatePicker value={startDate} onChange={(v) => u({ startDate: v })} className="h-8 text-xs" />
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <span className="w-36 shrink-0 text-xs text-muted-foreground">Hasta</span>
                  <div className="flex-1">
                    <DatePicker value={endDate} onChange={(v) => u({ endDate: v })} className="h-8 text-xs" />
                  </div>
                </div>

                {symbol && (
                  <p className="text-[10px] text-muted-foreground/50">
                    Datos disponibles: {symbol.start_date} → {symbol.end_date}
                  </p>
                )}

                <Num label="Capital inicial $" value={initialCapital} min={100} step={1000} onChange={(v) => u({ initialCapital: v })} />
              </div>
            </Section>

            <Section title="Objetivo y Límites">
              <Num label="Databank limit" value={databankLimit} min={1} max={1000} step={10} onChange={(v) => u({ databankLimit: v })} />
              <Num label="Trades mínimos" value={minTrades} min={1} step={5} onChange={(v) => u({ minTrades: v })} />
              <Num label="Trades/día máx." value={maxTradesPerDay} min={0} step={1} onChange={(v) => u({ maxTradesPerDay: v })} />
              <div className="flex items-center gap-2 text-xs">
                <span className="text-muted-foreground w-36 shrink-0">Dirección</span>
                <Select value={tradeDirection} onValueChange={(v) => u({ tradeDirection: v as TradeDirection })}>
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
                Trades/día = 0 → sin límite diario.
              </p>
            </Section>

            <Section title="Tamaño de Posición">
              <div className="flex gap-2">
                {(["FixedLots", "FixedAmount", "PercentEquity"] as const).map((t) => (
                  <button
                    key={t}
                    onClick={() => u({ positionSizingType: t })}
                    className={cn(
                      "flex-1 rounded border px-2 py-1.5 text-[11px] font-medium transition-colors",
                      positionSizingType === t
                        ? "border-primary bg-primary/10 text-primary"
                        : "border-border/40 text-muted-foreground hover:border-primary/40 hover:text-foreground"
                    )}
                  >
                    {t === "FixedLots" ? "Lotaje Fijo" : t === "FixedAmount" ? "Monto Fijo" : "% Capital"}
                  </button>
                ))}
              </div>
              <Num
                label={positionSizingType === "FixedLots" ? "Lotes" : positionSizingType === "FixedAmount" ? "Monto $" : "% del capital"}
                value={positionSizingValue}
                min={0.01}
                step={positionSizingType === "FixedLots" ? 0.01 : positionSizingType === "PercentEquity" ? 0.5 : 100}
                onChange={(v) => u({ positionSizingValue: v })}
              />
            </Section>

            <Section title="Horarios de Trading">
              <label className="flex items-center gap-2 text-xs cursor-pointer">
                <input
                  type="checkbox"
                  checked={tradingHoursEnabled}
                  onChange={(e) => u({ tradingHoursEnabled: e.target.checked })}
                  className="accent-primary"
                />
                <span className="text-muted-foreground">Limitar horario de entradas</span>
              </label>
              {tradingHoursEnabled && (
                <div className="space-y-1.5 pl-4">
                  <div className="flex items-center gap-2 text-xs">
                    <span className="w-28 shrink-0 text-muted-foreground">Desde (HH:MM)</span>
                    <input type="number" value={tradingHoursStartHour} min={0} max={23}
                      className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                      onChange={(e) => { const v = parseInt(e.target.value); if (!isNaN(v)) u({ tradingHoursStartHour: Math.max(0, Math.min(23, v)) }); }} />
                    <span className="text-muted-foreground">:</span>
                    <input type="number" value={tradingHoursStartMinute} min={0} max={59} step={5}
                      className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                      onChange={(e) => { const v = parseInt(e.target.value); if (!isNaN(v)) u({ tradingHoursStartMinute: Math.max(0, Math.min(59, v)) }); }} />
                  </div>
                  <div className="flex items-center gap-2 text-xs">
                    <span className="w-28 shrink-0 text-muted-foreground">Hasta (HH:MM)</span>
                    <input type="number" value={tradingHoursEndHour} min={0} max={23}
                      className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                      onChange={(e) => { const v = parseInt(e.target.value); if (!isNaN(v)) u({ tradingHoursEndHour: Math.max(0, Math.min(23, v)) }); }} />
                    <span className="text-muted-foreground">:</span>
                    <input type="number" value={tradingHoursEndMinute} min={0} max={59} step={5}
                      className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                      onChange={(e) => { const v = parseInt(e.target.value); if (!isNaN(v)) u({ tradingHoursEndMinute: Math.max(0, Math.min(59, v)) }); }} />
                  </div>
                  <p className="text-[10px] text-muted-foreground/50">Soporta rangos que cruzan medianoche (ej. 22:00 → 06:00).</p>
                </div>
              )}
            </Section>

            <Section title="Cierre Automático">
              <label className="flex items-center gap-2 text-xs cursor-pointer">
                <input
                  type="checkbox"
                  checked={closeAtTimeEnabled}
                  onChange={(e) => u({ closeAtTimeEnabled: e.target.checked })}
                  className="accent-primary"
                />
                <span className="text-muted-foreground">Cerrar posición abierta a una hora fija</span>
              </label>
              {closeAtTimeEnabled && (
                <div className="space-y-1.5 pl-4">
                  <div className="flex items-center gap-2 text-xs">
                    <span className="w-28 shrink-0 text-muted-foreground">Hora cierre (HH:MM)</span>
                    <input type="number" value={closeAtTimeHour} min={0} max={23}
                      className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                      onChange={(e) => { const v = parseInt(e.target.value); if (!isNaN(v)) u({ closeAtTimeHour: Math.max(0, Math.min(23, v)) }); }} />
                    <span className="text-muted-foreground">:</span>
                    <input type="number" value={closeAtTimeMinute} min={0} max={59} step={5}
                      className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                      onChange={(e) => { const v = parseInt(e.target.value); if (!isNaN(v)) u({ closeAtTimeMinute: Math.max(0, Math.min(59, v)) }); }} />
                  </div>
                  <p className="text-[10px] text-muted-foreground/50">Cierra cualquier trade abierto cuando el tiempo de la barra ≥ la hora indicada.</p>
                </div>
              )}
            </Section>

            <Section title="Stop Loss">
              <label className="flex items-center gap-2 text-xs cursor-pointer">
                <input type="checkbox" checked={slEnabled} onChange={(e) => u({ slEnabled: e.target.checked })} className="accent-primary" />
                <span className="text-muted-foreground">Activar Stop Loss</span>
              </label>
              {slEnabled && (
                <div className="space-y-2 pl-4">
                  <div className="flex items-center gap-2 text-xs">
                    <span className="w-28 shrink-0 text-muted-foreground">Tipo</span>
                    <select value={slType} onChange={(e) => u({ slType: e.target.value as typeof slType })}
                      className="flex-1 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-foreground focus:outline-none focus:border-primary/60">
                      <option value="Pips">Pips</option>
                      <option value="Percentage">Porcentaje</option>
                      <option value="ATR">ATR (rango)</option>
                    </select>
                  </div>
                  {slType === "ATR" ? (
                    <>
                      <p className="text-[10px] text-muted-foreground/50">El builder busca el período y multiplicador óptimos dentro de los rangos indicados.</p>
                      <div className="flex items-center gap-2 text-xs">
                        <span className="w-28 shrink-0 text-muted-foreground">Período (min→max)</span>
                        <input type="number" value={slAtrPeriodMin} min={1} step={1}
                          className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                          onChange={(e) => { const v = parseInt(e.target.value); if (!isNaN(v) && v >= 1) u({ slAtrPeriodMin: v }); }} />
                        <span className="text-muted-foreground/50">→</span>
                        <input type="number" value={slAtrPeriodMax} min={1} step={1}
                          className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                          onChange={(e) => { const v = parseInt(e.target.value); if (!isNaN(v) && v >= 1) u({ slAtrPeriodMax: v }); }} />
                      </div>
                      <div className="flex items-center gap-2 text-xs">
                        <span className="w-28 shrink-0 text-muted-foreground">Multiplicador (min→max)</span>
                        <input type="number" value={slAtrMultMin} min={0.01} step={0.1}
                          className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                          onChange={(e) => { const v = parseFloat(e.target.value); if (!isNaN(v) && v > 0) u({ slAtrMultMin: v }); }} />
                        <span className="text-muted-foreground/50">→</span>
                        <input type="number" value={slAtrMultMax} min={0.01} step={0.1}
                          className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                          onChange={(e) => { const v = parseFloat(e.target.value); if (!isNaN(v) && v > 0) u({ slAtrMultMax: v }); }} />
                      </div>
                    </>
                  ) : (
                    <Num label={slType === "Percentage" ? "% del precio" : "Pips"} value={slValue} min={0.01} step={slType === "Pips" ? 1 : 0.1} onChange={(v) => u({ slValue: v })} />
                  )}
                </div>
              )}
            </Section>

            <Section title="Take Profit">
              <label className="flex items-center gap-2 text-xs cursor-pointer">
                <input type="checkbox" checked={tpEnabled} onChange={(e) => u({ tpEnabled: e.target.checked })} className="accent-primary" />
                <span className="text-muted-foreground">Activar Take Profit</span>
              </label>
              {tpEnabled && (
                <div className="space-y-2 pl-4">
                  <div className="flex items-center gap-2 text-xs">
                    <span className="w-28 shrink-0 text-muted-foreground">Tipo</span>
                    <select value={tpType} onChange={(e) => u({ tpType: e.target.value as typeof tpType })}
                      className="flex-1 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-foreground focus:outline-none focus:border-primary/60">
                      <option value="Pips">Pips</option>
                      <option value="RiskReward">Risk/Reward</option>
                      <option value="ATR">ATR (rango)</option>
                    </select>
                  </div>
                  {tpType === "ATR" ? (
                    <>
                      <p className="text-[10px] text-muted-foreground/50">El builder busca el período y multiplicador óptimos dentro de los rangos indicados.</p>
                      <div className="flex items-center gap-2 text-xs">
                        <span className="w-28 shrink-0 text-muted-foreground">Período (min→max)</span>
                        <input type="number" value={tpAtrPeriodMin} min={1} step={1}
                          className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                          onChange={(e) => { const v = parseInt(e.target.value); if (!isNaN(v) && v >= 1) u({ tpAtrPeriodMin: v }); }} />
                        <span className="text-muted-foreground/50">→</span>
                        <input type="number" value={tpAtrPeriodMax} min={1} step={1}
                          className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                          onChange={(e) => { const v = parseInt(e.target.value); if (!isNaN(v) && v >= 1) u({ tpAtrPeriodMax: v }); }} />
                      </div>
                      <div className="flex items-center gap-2 text-xs">
                        <span className="w-28 shrink-0 text-muted-foreground">Multiplicador (min→max)</span>
                        <input type="number" value={tpAtrMultMin} min={0.01} step={0.1}
                          className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                          onChange={(e) => { const v = parseFloat(e.target.value); if (!isNaN(v) && v > 0) u({ tpAtrMultMin: v }); }} />
                        <span className="text-muted-foreground/50">→</span>
                        <input type="number" value={tpAtrMultMax} min={0.01} step={0.1}
                          className="w-14 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-center"
                          onChange={(e) => { const v = parseFloat(e.target.value); if (!isNaN(v) && v > 0) u({ tpAtrMultMax: v }); }} />
                      </div>
                    </>
                  ) : (
                    <Num label={tpType === "RiskReward" ? "Ratio R/R (ej. 2 = 2:1)" : "Pips"} value={tpValue} min={0.01} step={tpType === "Pips" ? 1 : 0.1} onChange={(v) => u({ tpValue: v })} />
                  )}
                </div>
              )}
            </Section>

            <Section title="Trailing Stop">
              <label className="flex items-center gap-2 text-xs cursor-pointer">
                <input type="checkbox" checked={tsEnabled} onChange={(e) => u({ tsEnabled: e.target.checked })} className="accent-primary" />
                <span className="text-muted-foreground">Activar Trailing Stop</span>
              </label>
              {tsEnabled && (
                <div className="space-y-2 pl-4">
                  <div className="flex items-center gap-2 text-xs">
                    <span className="w-28 shrink-0 text-muted-foreground">Tipo</span>
                    <select value={tsType} onChange={(e) => u({ tsType: e.target.value as typeof tsType })}
                      className="flex-1 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-foreground focus:outline-none focus:border-primary/60">
                      <option value="ATR">ATR Multiplicador</option>
                      <option value="RiskReward">Risk/Reward</option>
                    </select>
                  </div>
                  {tsType === "ATR" && (
                    <Num label="Período ATR" value={tsAtrPeriod} min={1} step={1} onChange={(v) => u({ tsAtrPeriod: v })} />
                  )}
                  <Num label="Multiplicador" value={tsValue} min={0.01} step={0.1} onChange={(v) => u({ tsValue: v })} />
                </div>
              )}
            </Section>

            <Section title="Fórmula de Salida">
              <label className="flex items-center gap-2 text-xs cursor-pointer">
                <input type="checkbox" checked={useExitFormula} onChange={(e) => u({ useExitFormula: e.target.checked })} className="accent-primary" />
                <span className="text-muted-foreground">Usar fórmula de salida (sign-change)</span>
              </label>
              <p className="text-[10px] text-muted-foreground/50">
                Cuando está activo, SR evoluciona un árbol de salida y cierra posiciones cuando cambia de signo. Desactivar es útil cuando solo se quiere salir por SL/TP.
              </p>
            </Section>

            <Section title="Costos de Trading">
              <Num label="Spread (pips)" value={spreadPips} min={0} step={0.1} onChange={(v) => u({ spreadPips: v })} />
              <div className="flex items-center gap-2 text-xs">
                <span className="text-muted-foreground w-36 shrink-0">Tipo comisión</span>
                <select
                  value={commissionType}
                  onChange={(e) => u({ commissionType: e.target.value as typeof commissionType })}
                  className="flex-1 rounded border border-border/40 bg-background px-2 py-0.5 text-xs text-foreground focus:outline-none focus:border-primary/60"
                >
                  <option value="FixedPerLot">Fija por lote</option>
                  <option value="Percentage">Porcentaje (%)</option>
                </select>
              </div>
              <Num
                label={commissionType === "FixedPerLot" ? "Comisión ($ / lote)" : "Comisión (%)"}
                value={commissionValue}
                min={0}
                step={commissionType === "FixedPerLot" ? 0.5 : 0.001}
                onChange={(v) => u({ commissionValue: v })}
              />
            </Section>

            <Section title="Evolución NSGA-II">
              <Num label="Población" value={populationSize} min={20} max={2000} step={10} onChange={(v) => u({ populationSize: v })} />
              <Num label="Max generaciones" value={generations} min={0} max={5000} step={50} onChange={(v) => u({ generations: v })} />
              <Num label="Tasa cruce" value={crossoverRate} min={0} max={1} step={0.05} onChange={(v) => u({ crossoverRate: v })} />
              <Num label="Tasa mutación" value={mutationRate} min={0} max={1} step={0.05} onChange={(v) => u({ mutationRate: v })} />
              <p className="text-[10px] text-muted-foreground/50">Max generaciones = 0 detiene solo por Databank limit.</p>
            </Section>

            <Section title="Árbol de Fórmulas">
              <Num label="Profundidad máx." value={maxDepth} min={2} max={10} step={1} onChange={(v) => u({ maxDepth: v })} />
            </Section>

            <Section title="CMA-ES — Refinamiento de Constantes">
              <Num label="Refinar top-K" value={cmaesTopK} min={0} max={50} onChange={(v) => u({ cmaesTopK: v })} />
              <Num label="Evaluaciones máx." value={cmaesIterations} min={50} max={2000} step={50} onChange={(v) => u({ cmaesIterations: v })} />
            </Section>

          </div>
        )}

        {/* ── TAB: Bloques de Construcción ────────────────────────────────────── */}
        {activeTab === "blocks" && (
          <div className="space-y-4 p-4">
            <Section
              title={`Pool de indicadores (${checkedTypes.length} / ${ALL_INDICATORS.length})`}
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
                  const checked = checkedTypes.includes(meta.type);
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
          </div>
        )}

        {/* ── TAB: Filtros ────────────────────────────────────────────────────── */}
        {activeTab === "filters" && (
          <div className="space-y-4 p-4">

            <Section title="Filtros Iniciales — Aceptación en Databank">
              <p className="text-[10px] text-muted-foreground/60 mb-1">
                Condiciones mínimas que una estrategia debe cumplir para ser aceptada en el databank durante la Fase 1 (evolución). Valor 0 = sin filtro.
              </p>
              <Num label="Sharpe mínimo" value={initMinSharpe} min={0} step={0.1} onChange={(v) => u({ initMinSharpe: v })} />
              <Num label="Profit Factor mín." value={initMinPF} min={0} step={0.1} onChange={(v) => u({ initMinPF: v })} />
              <Num label="Max Drawdown % máx." value={initMaxDD} min={0} max={100} step={5} onChange={(v) => u({ initMaxDD: v })} />
              <p className="text-[10px] text-muted-foreground/50">
                Filtros iniciales muy restrictivos pueden dificultar el llenado del databank. Usá valores conservadores o dejalos en 0.
              </p>
            </Section>

            <Section title="Filtros Finales — Pareto Front">
              <p className="text-[10px] text-muted-foreground/60 mb-1">
                Se aplican al front final antes de mostrarlo. Las estrategias que no cumplan estos umbrales no aparecen en los resultados. Valor 0 = sin filtro.
              </p>
              <Num label="Sharpe mínimo" value={finalMinSharpe} min={0} step={0.1} onChange={(v) => u({ finalMinSharpe: v })} />
              <Num label="Profit Factor mín." value={finalMinPF} min={0} step={0.1} onChange={(v) => u({ finalMinPF: v })} />
              <Num label="Trades mínimos" value={finalMinTrades} min={0} step={5} onChange={(v) => u({ finalMinTrades: v })} />
              <Num label="Max Drawdown % máx." value={finalMaxDD} min={0} max={100} step={5} onChange={(v) => u({ finalMaxDD: v })} />
            </Section>

          </div>
        )}

      </div>

      {/* ── Run/Stop — always visible at bottom ─────────────────────────────── */}
      <div className="shrink-0 border-t border-border/20 px-4 py-3">
        <div className="flex justify-end">
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

    </div>
  );
}
