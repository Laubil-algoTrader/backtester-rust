import { useState } from "react";
import { ChevronDown, ChevronRight, Download, BarChart2, Loader2, X, Copy, Check } from "lucide-react";
import { useAppStore } from "@/stores/useAppStore";
import { generateSrCode, runSrBacktest } from "@/lib/tauri";
import type { SrFrontItem } from "@/lib/types";
import type { SrRunMeta } from "@/stores/useAppStore";

function fmt(v: number, decimals = 2) {
  return isFinite(v) ? v.toFixed(decimals) : "—";
}

// ── Code Modal ────────────────────────────────────────────────────────────────

function CodeModal({
  filename,
  code,
  onClose,
}: {
  filename: string;
  code: string;
  onClose: () => void;
}) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    await navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
    >
      <div
        className="relative flex flex-col bg-background border border-border rounded-lg shadow-2xl w-[780px] max-w-[95vw] h-[75vh]"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between shrink-0 px-4 py-3 border-b border-border/30">
          <span className="text-sm font-semibold font-mono text-foreground">{filename}</span>
          <div className="flex items-center gap-2">
            <button
              onClick={handleCopy}
              className="flex items-center gap-1.5 rounded px-2.5 py-1 text-xs bg-primary/10 text-primary border border-primary/20 hover:bg-primary/20 transition-colors"
            >
              {copied ? <Check className="w-3 h-3" /> : <Copy className="w-3 h-3" />}
              {copied ? "Copiado" : "Copiar"}
            </button>
            <button
              onClick={onClose}
              className="rounded p-1 text-muted-foreground hover:text-foreground hover:bg-muted/40 transition-colors"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        </div>
        {/* Code */}
        <pre className="flex-1 overflow-auto p-4 text-[11px] font-mono text-foreground/90 leading-relaxed">
          {code}
        </pre>
      </div>
    </div>
  );
}

// ── Formula Row ───────────────────────────────────────────────────────────────

function FormulaRow({
  item,
  srMeta,
  onViewBacktest,
}: {
  item: SrFrontItem;
  srMeta: SrRunMeta | null;
  onViewBacktest: (item: SrFrontItem) => Promise<void>;
}) {
  const [expanded, setExpanded] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);
  const [viewingBacktest, setViewingBacktest] = useState(false);
  const [codeModal, setCodeModal] = useState<{ filename: string; code: string } | null>(null);

  async function handleViewClick() {
    setViewingBacktest(true);
    try {
      await onViewBacktest(item);
    } finally {
      setViewingBacktest(false);
    }
  }

  const { objectives: o, metrics: m } = item;
  const maxDD = (-o.neg_max_drawdown * 100).toFixed(1);

  async function handleExport() {
    setExporting(true);
    setExportError(null);
    try {
      const name = `SR_Rank${item.rank}_Sharpe${o.sharpe.toFixed(2).replace(".", "_")}`;
      const result = await generateSrCode(item.strategy, name);
      const mainFile = result.files.find((f) => f.is_main) ?? result.files[0];
      setCodeModal({ filename: mainFile.filename, code: mainFile.code });
    } catch (e) {
      setExportError(String(e));
    } finally {
      setExporting(false);
    }
  }

  return (
    <>
      {codeModal && (
        <CodeModal
          filename={codeModal.filename}
          code={codeModal.code}
          onClose={() => setCodeModal(null)}
        />
      )}
      <tr
        className="border-b border-border/20 hover:bg-muted/20 cursor-pointer select-none"
        onClick={() => setExpanded((v) => !v)}
      >
        <td className="px-3 py-2 text-xs text-center text-muted-foreground w-8">
          {expanded ? <ChevronDown className="inline w-3 h-3" /> : <ChevronRight className="inline w-3 h-3" />}
        </td>
        <td className="px-3 py-2 text-xs text-center font-mono text-muted-foreground">{item.rank}</td>
        <td className="px-3 py-2 text-xs text-center font-mono text-emerald-400">{fmt(o.sharpe)}</td>
        <td className="px-3 py-2 text-xs text-center font-mono">
          <span className={o.profit_factor >= 1.3 ? "text-emerald-400" : o.profit_factor >= 1 ? "text-yellow-400" : "text-red-400"}>
            {fmt(o.profit_factor)}
          </span>
        </td>
        <td className="px-3 py-2 text-xs text-center font-mono">{m.total_trades}</td>
        <td className="px-3 py-2 text-xs text-center font-mono text-sky-400">{fmt(o.temporal_consistency)}</td>
        <td className="px-3 py-2 text-xs text-center font-mono text-violet-400">{fmt(o.expectancy_ratio)}</td>
        <td className="px-3 py-2 text-xs text-center font-mono text-red-400">{maxDD}%</td>
        <td className="px-3 py-2 text-xs font-mono text-muted-foreground max-w-[260px] truncate" title={item.formula_long}>
          {item.formula_long}
        </td>
        <td className="px-3 py-2 text-xs whitespace-nowrap">
          <div className="flex gap-1 justify-end" onClick={(e) => e.stopPropagation()}>
            <button
              onClick={handleViewClick}
              disabled={!srMeta || viewingBacktest}
              className="flex items-center gap-1 rounded px-2 py-1 text-[10px] bg-primary/10 text-primary border border-primary/20 hover:bg-primary/20 transition-colors disabled:opacity-40"
              title="Ver en Backtest"
            >
              {viewingBacktest
                ? <Loader2 className="w-3 h-3 animate-spin" />
                : <BarChart2 className="w-3 h-3" />}
            </button>
            <button
              onClick={handleExport}
              disabled={exporting}
              className="flex items-center gap-1 rounded px-2 py-1 text-[10px] bg-muted/40 text-muted-foreground border border-border/30 hover:bg-muted/60 transition-colors disabled:opacity-40"
              title="Ver código MQL5"
            >
              <Download className="w-3 h-3" />
              {exporting ? "..." : "MQL5"}
            </button>
          </div>
          {exportError && (
            <div className="text-[10px] text-red-400 mt-1 max-w-[160px] truncate" title={exportError}>
              {exportError}
            </div>
          )}
        </td>
      </tr>
      {expanded && (
        <tr className="border-b border-border/10 bg-muted/10">
          <td colSpan={10} className="px-4 py-3">
            <div className="grid gap-2 text-[11px]">
              <FormulaBlock label="Entry Long" formula={item.formula_long} threshold={item.strategy.long_threshold} op=">" />
              <FormulaBlock label="Entry Short" formula={item.formula_short} threshold={item.strategy.short_threshold} op="<" />
              <FormulaBlock label="Exit" formula={item.formula_exit} threshold={0} op="crosses 0" />
              <MetricsRow m={m} o={o} />
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

function FormulaBlock({
  label,
  formula,
  threshold,
  op,
}: {
  label: string;
  formula: string;
  threshold: number;
  op: string;
}) {
  return (
    <div>
      <span className="text-muted-foreground font-medium">{label}: </span>
      <code className="rounded bg-muted/40 px-1.5 py-0.5 text-foreground font-mono break-all">
        {formula} {op} {op !== "crosses 0" ? threshold.toFixed(4) : ""}
      </code>
    </div>
  );
}

function MetricsRow({
  m,
  o,
}: {
  m: SrFrontItem["metrics"];
  o: SrFrontItem["objectives"];
}) {
  const items = [
    { label: "Net Profit", value: `$${fmt(m.net_profit, 0)}` },
    { label: "Win Rate", value: `${fmt(m.win_rate_pct, 1)}%` },
    { label: "Profit Factor", value: fmt(o.profit_factor) },
    { label: "Sharpe", value: fmt(o.sharpe) },
    { label: "Sortino", value: fmt(m.sortino_ratio) },
    { label: "Max DD", value: `${fmt(m.max_drawdown_pct, 1)}%` },
    { label: "Trades", value: String(m.total_trades) },
    { label: "Avg Trade", value: `$${fmt(m.avg_trade, 0)}` },
  ];
  return (
    <div className="flex flex-wrap gap-x-4 gap-y-1 pt-1 border-t border-border/20">
      {items.map(({ label, value }) => (
        <span key={label} className="text-muted-foreground">
          {label}: <span className="text-foreground font-mono">{value}</span>
        </span>
      ))}
    </div>
  );
}

export function SrResultsPanel() {
  const {
    srResults, srRunning, srProgress, srLastConfig,
    setBacktestResults, setActiveSection,
  } = useAppStore();
  const [viewError, setViewError] = useState<string | null>(null);

  async function handleViewBacktest(item: SrFrontItem) {
    if (!srLastConfig) return;
    setViewError(null);
    try {
      const result = await runSrBacktest(
        item.strategy,
        srLastConfig.symbolId,
        srLastConfig.timeframe,
        srLastConfig.startDate,
        srLastConfig.endDate,
        srLastConfig.initialCapital,
      );
      setBacktestResults(result);
      setActiveSection("backtest");
    } catch (e) {
      setViewError(String(e));
    }
  }

  if (srRunning) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4 text-sm text-muted-foreground">
        <div className="w-8 h-8 border-2 border-primary/30 border-t-primary rounded-full animate-spin" />
        {srProgress ? (
          <div className="text-center">
            {srProgress.phase === "generation" ? (
              <>
                <div>Generación {srProgress.gen} / {srProgress.total}</div>
                <div className="text-xs text-muted-foreground/60 mt-1">
                  Pareto size: {srProgress.pareto_size} · Best Sharpe: {srProgress.best_sharpe.toFixed(3)}
                </div>
              </>
            ) : (
              <div>Refinando constantes {srProgress.current} / {srProgress.total}…</div>
            )}
          </div>
        ) : (
          <div>Inicializando regresión simbólica…</div>
        )}
      </div>
    );
  }

  if (srResults.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-2 text-sm text-muted-foreground/40">
        <BarChart2 className="w-10 h-10 opacity-30" />
        <span>Ejecuta la Regresión Simbólica para ver el Pareto front</span>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Header */}
      <div className="shrink-0 px-4 py-2 border-b border-border/20 flex items-center justify-between">
        <span className="text-xs font-semibold text-foreground">
          Pareto Front — {srResults.length} estrategias
        </span>
        <span className="text-[10px] text-muted-foreground/60">
          Haz clic en una fila para ver las fórmulas · <BarChart2 className="inline w-3 h-3" /> para ver el backtest completo
        </span>
      </div>
      {viewError && (
        <div className="shrink-0 px-4 py-1.5 text-[11px] text-red-400 bg-red-400/5 border-b border-red-400/20">
          Error al cargar backtest: {viewError}
        </div>
      )}

      {/* Table */}
      <div className="flex-1 overflow-auto">
        <table className="w-full text-xs border-collapse">
          <thead className="sticky top-0 bg-background border-b border-border/30 z-10">
            <tr>
              <th className="px-3 py-2 w-8" />
              <th className="px-3 py-2 text-center text-muted-foreground font-medium">Rank</th>
              <th className="px-3 py-2 text-center text-muted-foreground font-medium">Sharpe</th>
              <th className="px-3 py-2 text-center text-muted-foreground font-medium">PF</th>
              <th className="px-3 py-2 text-center text-muted-foreground font-medium">Trades</th>
              <th className="px-3 py-2 text-center text-muted-foreground font-medium">Consist.</th>
              <th className="px-3 py-2 text-center text-muted-foreground font-medium">Expect. R</th>
              <th className="px-3 py-2 text-center text-muted-foreground font-medium">Max DD</th>
              <th className="px-3 py-2 text-left text-muted-foreground font-medium">Fórmula Entry Long</th>
              <th className="px-3 py-2 text-right text-muted-foreground font-medium">Acciones</th>
            </tr>
          </thead>
          <tbody>
            {srResults.map((item, i) => (
              <FormulaRow key={i} item={item} srMeta={srLastConfig} onViewBacktest={handleViewBacktest} />
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
