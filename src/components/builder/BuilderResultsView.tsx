import { useState } from "react";
import { ArrowLeft, BarChart2, Loader2, Play, Trash2 } from "lucide-react";
import { useAppStore } from "@/stores/useAppStore";
import { runBacktest } from "@/lib/tauri";
import { MetricsGrid } from "@/components/backtest/MetricsGrid";
import { EquityCurve } from "@/components/backtest/EquityCurve";
import { DrawdownChart } from "@/components/backtest/DrawdownChart";
import { MonthlyReturns } from "@/components/backtest/MonthlyReturns";
import { MonthlyReturnsGrid } from "@/components/backtest/MonthlyReturnsGrid";
import { TradesList } from "@/components/backtest/TradesList";
import { DatePicker } from "@/components/ui/DatePicker";
import type { BuilderSavedStrategy, BacktestResults, Strategy } from "@/lib/types";
import { cn } from "@/lib/utils";

// ── Compact strategy table ────────────────────────────────────────────────────

function StrategyRow({
  s,
  onSelect,
  onRemove,
}: {
  s: BuilderSavedStrategy;
  onSelect: () => void;
  onRemove: () => void;
}) {
  const fmt = (n: number, d = 2) => n.toLocaleString(undefined, { maximumFractionDigits: d });
  return (
    <tr
      className="border-b border-border/20 hover:bg-muted/20 cursor-pointer"
      onClick={onSelect}
    >
      <td className="px-3 py-2 text-xs font-medium text-foreground max-w-[180px] truncate">{s.name}</td>
      <td className="px-3 py-2 text-xs text-muted-foreground">{s.symbolName}</td>
      <td className="px-3 py-2 text-xs text-muted-foreground">{s.timeframe.toUpperCase()}</td>
      <td className={cn("px-3 py-2 text-xs font-mono tabular-nums", s.netProfit >= 0 ? "text-emerald-400" : "text-red-400")}>
        {s.netProfit >= 0 ? "+" : ""}${Math.abs(s.netProfit).toLocaleString(undefined, { maximumFractionDigits: 0 })}
      </td>
      <td className={cn("px-3 py-2 text-xs font-mono tabular-nums", s.sharpeRatio >= 1 ? "text-emerald-400" : "text-foreground")}>
        {fmt(s.sharpeRatio)}
      </td>
      <td className={cn("px-3 py-2 text-xs font-mono tabular-nums", s.profitFactor >= 1.3 ? "text-emerald-400" : s.profitFactor < 1 ? "text-red-400" : "text-foreground")}>
        {fmt(s.profitFactor)}
      </td>
      <td className="px-3 py-2 text-xs font-mono tabular-nums text-muted-foreground">{s.trades}</td>
      <td className="px-3 py-2 text-xs font-mono tabular-nums text-red-400">{fmt(s.maxDrawdownAbs, 0) !== "0" ? `-$${fmt(s.maxDrawdownAbs, 0)}` : "—"}</td>
      <td className="px-3 py-2 text-xs font-mono tabular-nums text-primary">{s.fitness.toFixed(4)}</td>
      <td className="px-2 py-2 text-xs" onClick={(e) => e.stopPropagation()}>
        <button
          onClick={onRemove}
          className="flex items-center justify-center rounded p-1 text-muted-foreground/40 hover:text-destructive hover:bg-destructive/10 transition-colors"
          title="Eliminar de Resultados"
        >
          <Trash2 className="w-3 h-3" />
        </button>
      </td>
    </tr>
  );
}

// ── Main component ─────────────────────────────────────────────────────────────

export function BuilderResultsView() {
  const {
    builderDatabanks,
    builderConfig,
    removeFromBuilderDatabank,
  } = useAppStore();

  const resultsStrategies =
    builderDatabanks.find((db) => db.id === "results")?.strategies ?? [];

  const [selected, setSelected] = useState<BuilderSavedStrategy | null>(null);
  const [startDate, setStartDate] = useState(builderConfig.dataConfig.startDate ?? "");
  const [endDate, setEndDate] = useState(builderConfig.dataConfig.endDate ?? "");
  const [initialCapital, setInitialCapital] = useState(
    builderConfig.moneyManagement.initialCapital ?? 10000
  );
  const [localResults, setLocalResults] = useState<BacktestResults | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSelect = (s: BuilderSavedStrategy) => {
    setSelected(s);
    setLocalResults(null);
    setError(null);
  };

  const handleBack = () => {
    setSelected(null);
    setLocalResults(null);
    setError(null);
  };

  const handleRemove = (id: string) => {
    removeFromBuilderDatabank("results", [id]);
    if (selected?.id === id) handleBack();
  };

  const handleRun = async () => {
    if (!selected) return;
    let strategy: Strategy;
    try {
      strategy = JSON.parse(selected.strategyJson) as Strategy;
    } catch {
      setError("No se pudo parsear la estrategia.");
      return;
    }

    const symbolId = selected.symbolId ?? builderConfig.dataConfig.symbolId ?? "";
    if (!symbolId) { setError("No hay símbolo configurado."); return; }
    if (!startDate || !endDate) { setError("Configura las fechas de inicio y fin."); return; }

    setLoading(true);
    setError(null);
    try {
      const results = await runBacktest(strategy, {
        symbol_id: symbolId,
        timeframe: selected.timeframe,
        start_date: startDate,
        end_date: endDate,
        initial_capital: initialCapital,
        leverage: 1.0,
        precision: builderConfig.dataConfig.precision ?? "SelectedTfOnly",
      });
      setLocalResults(results);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  // ── Empty state ──
  if (resultsStrategies.length === 0) {
    return (
      <div className="flex flex-col h-full items-center justify-center gap-3 text-sm text-muted-foreground/40">
        <BarChart2 className="w-10 h-10 opacity-30" />
        <span>No hay estrategias en Resultados</span>
        <span className="text-[11px]">
          Usa el botón "Guardar en Resultados" en la vista de detalle de una estrategia del Builder
        </span>
      </div>
    );
  }

  // ── Strategy list ──
  if (!selected) {
    return (
      <div className="flex flex-col h-full overflow-hidden">
        <div className="shrink-0 px-4 py-2 border-b border-border/20 flex items-center justify-between">
          <span className="text-xs font-semibold text-foreground">
            Resultados Finales — {resultsStrategies.length} estrategia{resultsStrategies.length !== 1 ? "s" : ""}
          </span>
          <span className="text-[10px] text-muted-foreground/50">
            Haz clic en una estrategia para ejecutar el backtest completo
          </span>
        </div>
        <div className="flex-1 overflow-auto">
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 bg-background border-b border-border/30 z-10">
              <tr>
                <th className="px-3 py-2 text-left text-muted-foreground font-medium">Nombre</th>
                <th className="px-3 py-2 text-left text-muted-foreground font-medium">Símbolo</th>
                <th className="px-3 py-2 text-left text-muted-foreground font-medium">TF</th>
                <th className="px-3 py-2 text-center text-muted-foreground font-medium">Net Profit</th>
                <th className="px-3 py-2 text-center text-muted-foreground font-medium">Sharpe</th>
                <th className="px-3 py-2 text-center text-muted-foreground font-medium">PF</th>
                <th className="px-3 py-2 text-center text-muted-foreground font-medium">Trades</th>
                <th className="px-3 py-2 text-center text-muted-foreground font-medium">Max DD</th>
                <th className="px-3 py-2 text-center text-muted-foreground font-medium">Fitness</th>
                <th className="px-3 py-2 w-8" />
              </tr>
            </thead>
            <tbody>
              {resultsStrategies.map((s) => (
                <StrategyRow
                  key={s.id}
                  s={s}
                  onSelect={() => handleSelect(s)}
                  onRemove={() => handleRemove(s.id)}
                />
              ))}
            </tbody>
          </table>
        </div>
      </div>
    );
  }

  // ── Detail + backtest view ──
  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Header */}
      <div className="shrink-0 flex items-center gap-3 border-b border-border/20 px-4 py-2">
        <button
          onClick={handleBack}
          className="flex items-center gap-1.5 rounded border border-border/30 px-3 py-1.5 text-xs text-muted-foreground hover:border-border/60 hover:text-foreground"
        >
          <ArrowLeft className="w-3.5 h-3.5" />
          Volver
        </button>
        <div className="flex flex-col min-w-0">
          <span className="text-sm font-semibold truncate">{selected.name}</span>
          <span className="text-[10px] text-muted-foreground/60">
            {selected.symbolName} · {selected.timeframe.toUpperCase()}
          </span>
        </div>

        {/* Date / capital config inline */}
        <div className="ml-auto flex items-center gap-2 text-xs">
          <span className="text-muted-foreground/60">Desde</span>
          <DatePicker value={startDate} onChange={setStartDate} className="h-7 text-xs w-32" />
          <span className="text-muted-foreground/60">Hasta</span>
          <DatePicker value={endDate} onChange={setEndDate} className="h-7 text-xs w-32" />
          <span className="text-muted-foreground/60">Capital</span>
          <input
            type="number"
            value={initialCapital}
            min={100}
            step={1000}
            className="w-24 rounded border border-border/40 bg-muted/30 px-2 py-1 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
            onChange={(e) => { const v = parseFloat(e.target.value); if (!isNaN(v)) setInitialCapital(v); }}
          />
          <button
            onClick={handleRun}
            disabled={loading}
            className="flex items-center gap-1.5 rounded bg-primary px-3 py-1.5 text-xs font-medium text-white hover:bg-primary/80 disabled:opacity-50 transition-colors"
          >
            {loading ? <Loader2 className="w-3 h-3 animate-spin" /> : <Play className="w-3 h-3" />}
            {loading ? "Ejecutando…" : "Ejecutar Backtest"}
          </button>
        </div>
      </div>

      {/* Error */}
      {error && (
        <div className="shrink-0 px-4 py-2 text-xs text-red-400 bg-red-400/5 border-b border-red-400/20">
          {error}
        </div>
      )}

      {/* Results */}
      <div className="flex-1 overflow-auto">
        {!localResults && !loading && (
          <div className="flex flex-col h-full items-center justify-center gap-2 text-sm text-muted-foreground/40">
            <Play className="w-8 h-8 opacity-30" />
            <span>Haz clic en "Ejecutar Backtest" para ver el análisis completo</span>
          </div>
        )}

        {loading && (
          <div className="flex flex-col h-full items-center justify-center gap-2 text-sm text-muted-foreground/50">
            <Loader2 className="w-8 h-8 animate-spin opacity-50" />
            <span>Ejecutando backtest…</span>
          </div>
        )}

        {localResults && (
          <div className="space-y-4 p-4">
            {/* Metrics */}
            <div className="rounded border border-border/30 bg-card p-4">
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                Métricas de Rendimiento
              </p>
              <MetricsGrid metrics={localResults.metrics} />
            </div>

            {/* Equity + Drawdown */}
            <div className="rounded border border-border/30 bg-card p-4 space-y-4">
              <div>
                <p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                  Equity Curve
                </p>
                <EquityCurve
                  data={localResults.equity_curve}
                  initialCapital={initialCapital}
                  markers={[]}
                />
              </div>
              <div>
                <p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                  Drawdown
                </p>
                <DrawdownChart data={localResults.drawdown_curve} />
              </div>
            </div>

            {/* Monthly returns */}
            <div className="rounded border border-border/30 bg-card p-4">
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                Rendimiento Mensual
              </p>
              <MonthlyReturns equityCurve={localResults.equity_curve} />
              {localResults.trades.length > 0 && (
                <div className="mt-4">
                  <MonthlyReturnsGrid trades={localResults.trades} initialCapital={initialCapital} />
                </div>
              )}
            </div>

            {/* Trades */}
            <div className="rounded border border-border/30 bg-card p-4">
              <p className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                Trades ({localResults.trades.length})
              </p>
              <TradesList trades={localResults.trades} />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
