import { useState, useEffect, useRef, useCallback } from "react";
import { toast } from "sonner";
import {
  ArrowLeft, BarChart2, Loader2, Play, Trash2,
  Code2, Copy, Check, Download, FileCode2, FolderDown, RefreshCw, FileDown,
} from "lucide-react";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile, mkdir } from "@tauri-apps/plugin-fs";
import { useAppStore } from "@/stores/useAppStore";
import { runBacktest, runSrBacktest, generateStrategyCode, generateSrCode } from "@/lib/tauri";
import { MetricsGrid } from "@/components/backtest/MetricsGrid";
import { EquityCurve } from "@/components/backtest/EquityCurve";
import { DrawdownChart } from "@/components/backtest/DrawdownChart";
import { MonthlyReturnsGrid } from "@/components/backtest/MonthlyReturnsGrid";
import { TradesList } from "@/components/backtest/TradesList";
import { DatePicker } from "@/components/ui/DatePicker";
import type { BuilderSavedStrategy, BacktestResults, Strategy, SrStrategy, CodeGenerationResult, CodeFile } from "@/lib/types";
import { cn } from "@/lib/utils";

// ── Compact strategy table ─────────────────────────────────────────────────────

function StrategyRow({
  s, onSelect, onRemove,
}: {
  s: BuilderSavedStrategy;
  onSelect: () => void;
  onRemove: () => void;
}) {
  const fmt = (n: number, d = 2) => n.toLocaleString(undefined, { maximumFractionDigits: d });
  return (
    <tr className="border-b border-border/20 hover:bg-muted/20 cursor-pointer" onClick={onSelect}>
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

// ── Inline code panel ─────────────────────────────────────────────────────────

function CodePanel({ saved }: { saved: BuilderSavedStrategy }) {
  type Lang = "mql5" | "pinescript";
  const [language, setLanguage] = useState<Lang>("mql5");
  const [result, setResult] = useState<CodeGenerationResult | null>(null);
  const [selectedIdx, setSelectedIdx] = useState(0);
  const [generating, setGenerating] = useState(false);
  const [genError, setGenError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const generate = useCallback(async (lang: Lang) => {
    setGenerating(true);
    setGenError(null);
    setResult(null);
    try {
      let res: CodeGenerationResult;
      if (saved.isSr) {
        const srStrategy = JSON.parse(saved.strategyJson) as SrStrategy;
        res = await generateSrCode(srStrategy, saved.name);
      } else {
        const strategy = JSON.parse(saved.strategyJson) as Strategy;
        res = await generateStrategyCode(lang, strategy);
      }
      setResult(res);
      const mainIdx = res.files.findIndex((f) => f.is_main);
      setSelectedIdx(mainIdx >= 0 ? mainIdx : 0);
    } catch (e) {
      setGenError(String(e));
    } finally {
      setGenerating(false);
    }
  }, [saved]);

  // Auto-generate on mount
  const autoGenRef = useRef(false);
  useEffect(() => {
    if (autoGenRef.current) return;
    autoGenRef.current = true;
    generate(language);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleLangChange = (lang: Lang) => {
    setLanguage(lang);
    generate(lang);
  };

  const selectedFile: CodeFile | null = result?.files[selectedIdx] ?? null;

  const handleCopy = async () => {
    if (!selectedFile) return;
    await navigator.clipboard.writeText(selectedFile.code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleDownloadCurrent = async () => {
    if (!selectedFile) return;
    const path = await save({
      defaultPath: selectedFile.filename,
      filters: [{ name: selectedFile.filename.endsWith(".mq5") ? "MQL5 File" : "Pine Script", extensions: [selectedFile.filename.split(".").pop() || "txt"] }],
    });
    if (path) await writeTextFile(path, selectedFile.code);
  };

  const handleDownloadAll = async () => {
    if (!result) return;
    if (result.files.length === 1) { await handleDownloadCurrent(); return; }
    const safeName = saved.name.replace(/[^a-zA-Z0-9_-]/g, "_");
    const folderPath = await save({ defaultPath: safeName, filters: [{ name: "Folder", extensions: [""] }] });
    if (folderPath) {
      try { await mkdir(folderPath, { recursive: true }); } catch { /* already exists */ }
      for (const file of result.files) await writeTextFile(`${folderPath}/${file.filename}`, file.code);
    }
  };

  const totalFiles = result?.files.length ?? 0;
  const indicatorFiles = result?.files.filter((f) => !f.is_main) ?? [];
  const mainFile = result?.files.find((f) => f.is_main) ?? null;
  const lineCount = selectedFile ? selectedFile.code.split("\n").length : 0;

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Toolbar */}
      <div className="shrink-0 flex items-center gap-2 border-b border-border/20 px-4 py-2 flex-wrap">
        {/* Language tabs (hidden for SR since it only supports MQL5) */}
        {!saved.isSr && (
          <div className="flex items-center gap-1">
            {(["mql5", "pinescript"] as Lang[]).map((lang) => (
              <button
                key={lang}
                onClick={() => handleLangChange(lang)}
                className={cn(
                  "rounded px-2.5 py-1 text-xs font-medium transition-colors",
                  language === lang
                    ? "bg-primary/15 text-primary ring-1 ring-primary/30"
                    : "text-muted-foreground hover:text-foreground hover:bg-muted/40"
                )}
              >
                {lang === "mql5" ? "MQL5" : "Pine Script v6"}
              </button>
            ))}
          </div>
        )}
        {saved.isSr && (
          <span className="text-xs font-medium text-muted-foreground">MQL5 — Expert Advisor</span>
        )}

        {/* File tabs (multi-file) */}
        {result && totalFiles > 1 && (
          <div className="flex items-center gap-1">
            {mainFile && (
              <button
                onClick={() => setSelectedIdx(result.files.indexOf(mainFile))}
                className={cn(
                  "flex items-center gap-1 rounded px-2 py-1 text-xs font-medium transition-colors",
                  selectedFile === mainFile ? "bg-primary/10 text-primary" : "text-muted-foreground hover:text-foreground"
                )}
              >
                <Code2 className="w-3 h-3" />
                {mainFile.filename}
                <span className="rounded bg-primary/10 px-1 text-[9px] text-primary">EA</span>
              </button>
            )}
            {indicatorFiles.map((file) => {
              const idx = result.files.indexOf(file);
              return (
                <button
                  key={file.filename}
                  onClick={() => setSelectedIdx(idx)}
                  className={cn(
                    "flex items-center gap-1 rounded px-2 py-1 text-xs font-medium transition-colors",
                    selectedIdx === idx ? "bg-primary/10 text-primary" : "text-muted-foreground hover:text-foreground"
                  )}
                >
                  <FileCode2 className="w-3 h-3" />
                  {file.filename}
                </button>
              );
            })}
          </div>
        )}

        <div className="ml-auto flex items-center gap-1.5">
          {selectedFile && (
            <span className="text-[10px] text-muted-foreground/50 tabular-nums mr-1">{lineCount} líneas</span>
          )}
          <button
            onClick={() => generate(language)}
            disabled={generating}
            className="flex items-center gap-1 rounded border border-border/30 px-2 py-1 text-xs text-muted-foreground hover:text-foreground disabled:opacity-40 transition-colors"
            title="Regenerar"
          >
            <RefreshCw className={cn("w-3 h-3", generating && "animate-spin")} />
          </button>
          <button
            onClick={handleCopy}
            disabled={!selectedFile}
            className="flex items-center gap-1.5 rounded border border-border/30 px-2.5 py-1 text-xs font-medium text-muted-foreground hover:text-foreground disabled:opacity-40 transition-colors"
          >
            {copied ? <Check className="w-3 h-3 text-emerald-400" /> : <Copy className="w-3 h-3" />}
            {copied ? "Copiado" : "Copiar"}
          </button>
          {totalFiles > 1 ? (
            <button
              onClick={handleDownloadAll}
              disabled={!result}
              className="flex items-center gap-1.5 rounded bg-primary px-2.5 py-1 text-xs font-medium text-white hover:bg-primary/80 disabled:opacity-40 transition-colors"
            >
              <FolderDown className="w-3 h-3" />
              Descargar todo ({totalFiles})
            </button>
          ) : (
            <button
              onClick={handleDownloadCurrent}
              disabled={!selectedFile}
              className="flex items-center gap-1.5 rounded bg-primary px-2.5 py-1 text-xs font-medium text-white hover:bg-primary/80 disabled:opacity-40 transition-colors"
            >
              <Download className="w-3 h-3" />
              Descargar .{language === "mql5" ? "mq5" : "pine"}
            </button>
          )}
        </div>
      </div>

      {/* Code area */}
      <div className="flex-1 overflow-hidden">
        {generating && (
          <div className="flex h-full items-center justify-center gap-2 text-sm text-muted-foreground/50">
            <Loader2 className="w-5 h-5 animate-spin" />
            <span>Generando código…</span>
          </div>
        )}
        {genError && !generating && (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-muted-foreground/50">
            <Code2 className="w-8 h-8 opacity-30" />
            <span className="text-red-400">{genError}</span>
          </div>
        )}
        {selectedFile && !generating && (
          <pre className="h-full overflow-auto bg-zinc-950 p-4 font-mono text-xs leading-relaxed text-zinc-300">
            <code>{selectedFile.code}</code>
          </pre>
        )}
      </div>
    </div>
  );
}

// ── Shared backtest detail panel ───────────────────────────────────────────────

export function StrategyBacktestDetail({
  strategy: saved,
  onBack,
  backLabel = "Volver",
}: {
  strategy: BuilderSavedStrategy;
  onBack: () => void;
  backLabel?: string;
}) {
  const { builderConfig } = useAppStore();

  const [view, setView] = useState<"backtest" | "code">("backtest");

  const [startDate, setStartDate] = useState(
    saved.startDate || builderConfig.dataConfig.startDate || ""
  );
  const [endDate, setEndDate] = useState(
    saved.endDate || builderConfig.dataConfig.endDate || ""
  );
  const [initialCapital, setInitialCapital] = useState(
    saved.initialCapital ?? builderConfig.moneyManagement.initialCapital ?? 10000
  );
  const [localResults, setLocalResults] = useState<BacktestResults | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleExport = async () => {
    if (!localResults) return;
    try {
      const { trades, metrics } = localResults;
      const lines: string[] = [
        "# Backtest Report",
        `# Strategy: ${saved.name}`,
        `# Symbol: ${saved.symbolName} | TF: ${saved.timeframe.toUpperCase()}`,
        `# Period: ${startDate} to ${endDate} | Capital: $${initialCapital}`,
        "#",
        `# Net Profit: $${metrics.net_profit.toFixed(2)} | Sharpe: ${metrics.sharpe_ratio.toFixed(2)} | PF: ${metrics.profit_factor.toFixed(2)} | Max DD: ${metrics.max_drawdown_pct.toFixed(2)}%`,
        "#",
        "Trade,Direction,Entry Time,Entry Price,Exit Time,Exit Price,Lots,PnL,Pips,Commission,Reason,Duration",
        ...trades.map((t, i) =>
          [i + 1, t.direction, t.entry_time, t.entry_price.toFixed(5), t.exit_time, t.exit_price.toFixed(5),
            t.lots.toFixed(2), t.pnl.toFixed(2), t.pnl_pips.toFixed(1), t.commission.toFixed(2),
            t.close_reason, t.duration_bars + "b"].join(",")
        ),
      ];
      const csv = lines.join("\n");
      const path = await save({
        defaultPath: `${saved.name.replace(/[^a-zA-Z0-9_-]/g, "_")}_backtest.csv`,
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      if (!path) return;
      await writeTextFile(path, csv);
      toast.success("Reporte exportado correctamente");
    } catch (err) {
      toast.error(`Error al exportar: ${err instanceof Error ? err.message : String(err)}`);
    }
  };

  const handleRun = async () => {
    const symbolId = saved.symbolId ?? builderConfig.dataConfig.symbolId ?? "";
    if (!symbolId) { setError("No hay símbolo configurado."); return; }
    if (!startDate || !endDate) { setError("Configura las fechas de inicio y fin."); return; }

    setLoading(true);
    setError(null);
    try {
      let results: BacktestResults;
      if (saved.isSr) {
        const srStrategy = JSON.parse(saved.strategyJson) as SrStrategy;
        results = await runSrBacktest(srStrategy, symbolId, saved.timeframe, startDate, endDate, initialCapital);
      } else {
        const strategy = JSON.parse(saved.strategyJson) as Strategy;
        results = await runBacktest(strategy, {
          symbol_id: symbolId,
          timeframe: saved.timeframe,
          start_date: startDate,
          end_date: endDate,
          initial_capital: initialCapital,
          leverage: 1.0,
          precision: builderConfig.dataConfig.precision ?? "SelectedTfOnly",
        });
      }
      setLocalResults(results);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  // Auto-run on mount
  const autoRanRef = useRef(false);
  useEffect(() => {
    if (autoRanRef.current) return;
    autoRanRef.current = true;
    handleRun();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Header */}
      <div className="shrink-0 flex items-center gap-3 border-b border-border/20 px-4 py-2">
        <button
          onClick={onBack}
          className="flex items-center gap-1.5 rounded border border-border/30 px-3 py-1.5 text-xs text-muted-foreground hover:border-border/60 hover:text-foreground"
        >
          <ArrowLeft className="w-3.5 h-3.5" />
          {backLabel}
        </button>

        <div className="flex flex-col min-w-0">
          <span className="text-sm font-semibold truncate">{saved.name}</span>
          <span className="text-[10px] text-muted-foreground/60">
            {saved.symbolName} · {saved.timeframe.toUpperCase()}
            {saved.isSr && <span className="ml-1.5 rounded bg-violet-500/15 px-1 py-0.5 text-[9px] font-bold text-violet-400">SR</span>}
          </span>
        </div>

        {/* View toggle */}
        <div className="flex items-center rounded border border-border/30 overflow-hidden">
          <button
            onClick={() => setView("backtest")}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium transition-colors",
              view === "backtest"
                ? "bg-primary/15 text-primary"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            <Play className="w-3 h-3" />
            Backtest
          </button>
          <button
            onClick={() => setView("code")}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium transition-colors border-l border-border/30",
              view === "code"
                ? "bg-primary/15 text-primary"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            <Code2 className="w-3 h-3" />
            Código
          </button>
        </div>

        {/* Date / capital — only shown in backtest view */}
        {view === "backtest" && (
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
              {loading ? "Ejecutando…" : "Ejecutar"}
            </button>
            <button
              onClick={handleExport}
              disabled={!localResults}
              title="Exportar reporte CSV"
              className="flex items-center gap-1.5 rounded border border-border/30 px-3 py-1.5 text-xs font-medium text-muted-foreground hover:text-foreground hover:border-border/60 disabled:opacity-30 transition-colors"
            >
              <FileDown className="w-3 h-3" />
              Exportar
            </button>
          </div>
        )}
      </div>

      {/* Error */}
      {error && view === "backtest" && (
        <div className="shrink-0 px-4 py-2 text-xs text-red-400 bg-red-400/5 border-b border-red-400/20">
          {error}
        </div>
      )}

      {/* Content */}
      <div className="flex-1 overflow-hidden">
        {view === "code" ? (
          <CodePanel saved={saved} />
        ) : (
          <div className="h-full overflow-auto">
            {loading && (
              <div className="flex flex-col h-full items-center justify-center gap-2 text-sm text-muted-foreground/50">
                <Loader2 className="w-8 h-8 animate-spin opacity-50" />
                <span>Ejecutando backtest…</span>
              </div>
            )}

            {localResults && (
              <div className="space-y-4 p-4">
                <div className="rounded border border-border/30 bg-card p-4">
                  <p className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                    Métricas de Rendimiento
                  </p>
                  <MetricsGrid metrics={localResults.metrics} />
                </div>

                <div className="rounded border border-border/30 bg-card p-4 space-y-4">
                  <div>
                    <p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                      Equity Curve
                    </p>
                    <EquityCurve data={localResults.equity_curve} initialCapital={initialCapital} markers={[]} />
                  </div>
                  <div>
                    <p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                      Drawdown
                    </p>
                    <DrawdownChart data={localResults.drawdown_curve} />
                  </div>
                </div>

                {localResults.trades.length > 0 && (
                  <div className="rounded border border-border/30 bg-card p-4">
                    <p className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                      Rendimiento Mensual
                    </p>
                    <MonthlyReturnsGrid trades={localResults.trades} initialCapital={initialCapital} />
                  </div>
                )}

                <div className="rounded border border-border/30 bg-card p-4">
                  <p className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
                    Trades ({localResults.trades.length})
                  </p>
                  <TradesList trades={localResults.trades} />
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Main component (Results databank list) ────────────────────────────────────

export function BuilderResultsView() {
  const { builderDatabanks, removeFromBuilderDatabank } = useAppStore();

  const resultsStrategies =
    builderDatabanks.find((db) => db.id === "results")?.strategies ?? [];

  const [selected, setSelected] = useState<BuilderSavedStrategy | null>(null);

  const handleRemove = (id: string) => {
    removeFromBuilderDatabank("results", [id]);
    if (selected?.id === id) setSelected(null);
  };

  if (resultsStrategies.length === 0) {
    return (
      <div className="flex flex-col h-full items-center justify-center gap-3 text-sm text-muted-foreground/40">
        <BarChart2 className="w-10 h-10 opacity-30" />
        <span>No hay estrategias en Resultados</span>
        <span className="text-[11px]">
          Ejecuta la Regresión Simbólica o usa "Guardar en Resultados" desde el detalle de una estrategia
        </span>
      </div>
    );
  }

  if (selected) {
    return (
      <StrategyBacktestDetail
        strategy={selected}
        onBack={() => setSelected(null)}
        backLabel="← Resultados"
      />
    );
  }

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
                onSelect={() => setSelected(s)}
                onRemove={() => handleRemove(s.id)}
              />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
