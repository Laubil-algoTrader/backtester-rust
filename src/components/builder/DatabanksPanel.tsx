import { useState, useMemo, useRef, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronUp, ChevronDown, ChevronsUpDown,
  Plus, Trash2, RotateCcw, Pencil, X,
} from "lucide-react";
import { useAppStore } from "@/stores/useAppStore";
import type { BuilderSavedStrategy } from "@/lib/types";
import { cn } from "@/lib/utils";

// ── Sparkline ─────────────────────────────────────────────────────────────────

function MiniSparkline({ data }: { data: number[] }) {
  if (data.length < 2) return <span className="text-[10px] text-muted-foreground/30">—</span>;
  const min = Math.min(...data);
  const max = Math.max(...data);
  const range = max - min || 1;
  const w = 72; const h = 28;
  const pts = data
    .map((v, i) => `${(i / (data.length - 1)) * w},${h - ((v - min) / range) * (h - 4) - 2}`)
    .join(" ");
  const isPos = data[data.length - 1] >= data[0];
  return (
    <svg width={w} height={h} className="overflow-visible">
      <polyline
        points={pts}
        fill="none"
        stroke={isPos ? "rgb(34 197 94)" : "rgb(239 68 68)"}
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}

// ── Sort helpers ──────────────────────────────────────────────────────────────

type SortKey = keyof BuilderSavedStrategy;
type SortDir = "asc" | "desc";

function SortIcon({ col, sortKey, sortDir }: { col: SortKey; sortKey: SortKey; sortDir: SortDir }) {
  if (col !== sortKey) return <ChevronsUpDown className="h-3 w-3 text-muted-foreground/30" />;
  return sortDir === "asc"
    ? <ChevronUp className="h-3 w-3 text-primary" />
    : <ChevronDown className="h-3 w-3 text-primary" />;
}

// ── Column definitions ────────────────────────────────────────────────────────

interface ColDef {
  key: SortKey;
  label: string;
  width: string;
  align?: "left" | "right" | "center";
  noSort?: boolean;
  render: (s: BuilderSavedStrategy) => React.ReactNode;
}

const fmt = (n: number, dec = 2) =>
  n.toLocaleString(undefined, { minimumFractionDigits: dec, maximumFractionDigits: dec });
const fmtMoney = (n: number) =>
  (n >= 0 ? "+" : "") + "$" + Math.abs(n).toLocaleString(undefined, { maximumFractionDigits: 0 });

function isOverfit(s: BuilderSavedStrategy): boolean {
  if (s.oosSharpeRatio === undefined || s.sharpeRatio <= 0) return false;
  if (s.oosSharpeRatio < s.sharpeRatio * 0.5) return true;
  if (s.oosNetProfit !== undefined && s.oosNetProfit < 0 && s.netProfit > 0) return true;
  return false;
}

const renderName = (s: BuilderSavedStrategy) => (
  <span className="flex items-center gap-1">
    <span className="truncate font-medium text-foreground" title={s.name}>{s.name}</span>
    {isOverfit(s) && (
      <span
        title="Possible overfitting: OOS Sharpe is less than half of IS Sharpe, or OOS profit is negative"
        className="shrink-0 rounded bg-orange-500/20 px-1 py-0.5 text-[8px] font-bold text-orange-400"
      >
        OVR
      </span>
    )}
  </span>
);

const COLUMNS: ColDef[] = [
  { key: "name", label: "Strategy Name", width: "140px", align: "left",
    render: renderName },
  { key: "fitness", label: "Fitness", width: "72px", align: "right",
    render: (s) => <span className="font-bold text-primary tabular-nums">{s.fitness.toFixed(4)}</span> },
  { key: "symbolName", label: "Symbol", width: "90px", align: "left",
    render: (s) => <span className="text-muted-foreground">{s.symbolName}</span> },
  { key: "timeframe", label: "TF", width: "50px", align: "center",
    render: (s) => <span className="uppercase text-muted-foreground">{s.timeframe}</span> },
  { key: "netProfit", label: "Net Profit", width: "96px", align: "right",
    render: (s) => (
      <span className={cn("tabular-nums font-medium", s.netProfit >= 0 ? "text-emerald-400" : "text-red-400")}>
        {fmtMoney(s.netProfit)}
      </span>
    ) },
  { key: "miniEquityCurve", label: "Equity", width: "80px", align: "center", noSort: true,
    render: (s) => <MiniSparkline data={s.miniEquityCurve} /> },
  { key: "trades", label: "# Trades", width: "56px", align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{s.trades}</span> },
  { key: "profitFactor", label: "Profit Factor", width: "72px", align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{fmt(s.profitFactor)}</span> },
  { key: "sharpeRatio", label: "Sharpe", width: "60px", align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{fmt(s.sharpeRatio)}</span> },
  { key: "rExpectancy", label: "R Expectancy", width: "80px", align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{fmt(s.rExpectancy)}</span> },
  { key: "annualReturnPct", label: "Annual %", width: "72px", align: "right",
    render: (s) => (
      <span className={cn("tabular-nums", s.annualReturnPct >= 0 ? "text-emerald-400" : "text-red-400")}>
        {fmt(s.annualReturnPct)}%
      </span>
    ) },
  { key: "maxDrawdownAbs", label: "Drawdown", width: "80px", align: "right",
    render: (s) => (
      <span className="tabular-nums text-red-400">
        ${s.maxDrawdownAbs.toLocaleString(undefined, { maximumFractionDigits: 0 })}
      </span>
    ) },
  { key: "retDDRatio", label: "Ret/DD", width: "60px", align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{fmt(s.retDDRatio)}</span> },
  { key: "avgWin", label: "Avg Win", width: "72px", align: "right",
    render: (s) => <span className="tabular-nums text-emerald-400">${fmt(s.avgWin, 0)}</span> },
  { key: "avgLoss", label: "Avg Loss", width: "72px", align: "right",
    render: (s) => <span className="tabular-nums text-red-400">-${fmt(Math.abs(s.avgLoss), 0)}</span> },
];

const OOS_COLUMNS: ColDef[] = [
  { key: "name", label: "Strategy Name", width: "140px", align: "left",
    render: renderName },
  { key: "fitness", label: "Fitness", width: "72px", align: "right",
    render: (s) => <span className="font-bold text-primary tabular-nums">{s.fitness.toFixed(4)}</span> },
  { key: "oosNetProfit", label: "OOS Profit", width: "96px", align: "right",
    render: (s) => s.oosNetProfit !== undefined
      ? <span className={cn("tabular-nums font-medium", s.oosNetProfit >= 0 ? "text-emerald-400" : "text-red-400")}>
          {fmtMoney(s.oosNetProfit)}
        </span>
      : <span className="text-muted-foreground/30 text-[10px]">—</span> },
  { key: "oosTrades", label: "OOS #", width: "56px", align: "right",
    render: (s) => s.oosTrades !== undefined
      ? <span className="tabular-nums text-foreground">{s.oosTrades}</span>
      : <span className="text-muted-foreground/30 text-[10px]">—</span> },
  { key: "oosProfitFactor", label: "OOS PF", width: "72px", align: "right",
    render: (s) => s.oosProfitFactor !== undefined
      ? <span className="tabular-nums text-foreground">{fmt(s.oosProfitFactor)}</span>
      : <span className="text-muted-foreground/30 text-[10px]">—</span> },
  { key: "oosSharpeRatio", label: "OOS Sharpe", width: "80px", align: "right",
    render: (s) => s.oosSharpeRatio !== undefined
      ? <span className="tabular-nums text-foreground">{fmt(s.oosSharpeRatio)}</span>
      : <span className="text-muted-foreground/30 text-[10px]">—</span> },
  { key: "oosMaxDrawdownAbs", label: "OOS DD", width: "80px", align: "right",
    render: (s) => s.oosMaxDrawdownAbs !== undefined
      ? <span className="tabular-nums text-red-400">
          ${s.oosMaxDrawdownAbs.toLocaleString(undefined, { maximumFractionDigits: 0 })}
        </span>
      : <span className="text-muted-foreground/30 text-[10px]">—</span> },
  { key: "oosWinRatePct", label: "OOS Win%", width: "72px", align: "right",
    render: (s) => s.oosWinRatePct !== undefined
      ? <span className="tabular-nums text-foreground">{fmt(s.oosWinRatePct)}%</span>
      : <span className="text-muted-foreground/30 text-[10px]">—</span> },
];

// ── Main component ────────────────────────────────────────────────────────────

interface DatabanksProps {
  onStrategyOpen?: (strategy: BuilderSavedStrategy) => void;
}

export function DatabanksPanel({ onStrategyOpen }: DatabanksProps) {
  const { t } = useTranslation("builder");

  const {
    builderDatabanks,
    activeDatabankId,
    targetDatabankId,
    createBuilderDatabank,
    renameBuilderDatabank,
    deleteBuilderDatabank,
    clearBuilderDatabank,
    clearAllBuilderDatabanks,
    setActiveDatabankId,
    setTargetDatabankId,
    removeFromBuilderDatabank,
  } = useAppStore();

  // ── Resize ────────────────────────────────────────────────────────────────
  const [height, setHeight] = useState(260);
  const isDragging = useRef(false);
  const dragStartY = useRef(0);
  const dragStartH = useRef(0);

  const handleDragStart = useCallback((e: React.MouseEvent) => {
    isDragging.current = true;
    dragStartY.current = e.clientY;
    dragStartH.current = height;
    e.preventDefault();
  }, [height]);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!isDragging.current) return;
      const delta = dragStartY.current - e.clientY;
      setHeight(Math.max(140, Math.min(700, dragStartH.current + delta)));
    };
    const onUp = () => { isDragging.current = false; };
    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
    return () => {
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };
  }, []);

  // ── Rename ────────────────────────────────────────────────────────────────
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const renameInputRef = useRef<HTMLInputElement>(null);

  const startRename = (id: string, currentName: string) => {
    setRenamingId(id);
    setRenameValue(currentName);
    setTimeout(() => renameInputRef.current?.select(), 0);
  };

  const commitRename = () => {
    if (renamingId && renameValue.trim()) {
      renameBuilderDatabank(renamingId, renameValue.trim());
    }
    setRenamingId(null);
  };

  const cancelRename = () => setRenamingId(null);

  // ── OOS toggle ──────────────────────────────────────────────────────────
  const [showOos, setShowOos] = useState(false);
  const activeColumns = showOos ? OOS_COLUMNS : COLUMNS;

  // ── Sort / select ─────────────────────────────────────────────────────────
  const [sortKey, setSortKey] = useState<SortKey>("fitness");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const activeBank = builderDatabanks.find((db) => db.id === activeDatabankId)
    ?? builderDatabanks[0];
  const strategies = activeBank?.strategies ?? [];

  const sorted = useMemo(() => {
    return [...strategies].sort((a, b) => {
      const av = a[sortKey]; const bv = b[sortKey];
      if (typeof av === "number" && typeof bv === "number")
        return sortDir === "asc" ? av - bv : bv - av;
      if (typeof av === "string" && typeof bv === "string")
        return sortDir === "asc" ? av.localeCompare(bv) : bv.localeCompare(av);
      return 0;
    });
  }, [strategies, sortKey, sortDir]);

  // Reset selection when active databank changes
  useEffect(() => { setSelected(new Set()); }, [activeDatabankId]);

  const toggleSelect = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });

  const toggleAll = () =>
    setSelected(selected.size === sorted.length && sorted.length > 0
      ? new Set()
      : new Set(sorted.map((s) => s.id)));

  const handleSort = (col: ColDef) => {
    if (col.noSort) return;
    if (col.key === sortKey) setSortDir((d) => d === "asc" ? "desc" : "asc");
    else { setSortKey(col.key); setSortDir("desc"); }
  };

  const handleDeleteSelected = () => {
    if (!activeBank || selected.size === 0) return;
    removeFromBuilderDatabank(activeBank.id, Array.from(selected));
    setSelected(new Set());
  };

  const handleNewDatabank = () => {
    const id = createBuilderDatabank();
    setActiveDatabankId(id);
  };

  const handleDeleteTab = (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    if (builderDatabanks.length <= 1) return; // keep at least one
    deleteBuilderDatabank(id);
  };

  return (
    <div
      className="flex shrink-0 flex-col border-t border-border/40 bg-background"
      style={{ height }}
    >
      {/* ── Drag handle ─────────────────────────────────────────────────── */}
      <div
        onMouseDown={handleDragStart}
        className="group relative flex h-3 w-full cursor-ns-resize items-center justify-center transition-colors"
        title="Drag to resize"
      >
        <div className="h-px w-full bg-border transition-colors group-hover:bg-primary/60" />
        <div className="absolute flex gap-0.5">
          <div className="h-1 w-8 rounded-full bg-border/80 transition-colors group-hover:bg-primary/80" />
        </div>
      </div>

      {/* ── Tab bar ─────────────────────────────────────────────────────── */}
      <div className="flex shrink-0 items-center gap-0 border-b border-border/30 bg-muted/5 px-2">
        {/* Databank tabs */}
        <div className="flex items-end gap-0.5 overflow-x-auto py-1 pr-1">
          {builderDatabanks.map((db) => (
            <div
              key={db.id}
              onClick={() => setActiveDatabankId(db.id)}
              onDoubleClick={() => startRename(db.id, db.name)}
              className={cn(
                "group relative flex h-7 cursor-pointer select-none items-center gap-1.5 rounded-t border px-2.5 text-[11px] font-medium transition-colors whitespace-nowrap",
                db.id === activeDatabankId
                  ? "border-border/40 border-b-background bg-background text-foreground z-10 -mb-px"
                  : "border-transparent text-muted-foreground hover:text-foreground hover:bg-muted/30"
              )}
            >
              {/* Target indicator */}
              {db.id === targetDatabankId && (
                <span
                  className="h-1.5 w-1.5 rounded-full bg-emerald-400 shrink-0"
                  title="Target databank — new strategies go here"
                />
              )}

              {/* Name or rename input */}
              {renamingId === db.id ? (
                <input
                  ref={renameInputRef}
                  value={renameValue}
                  onChange={(e) => setRenameValue(e.target.value)}
                  onBlur={commitRename}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") commitRename();
                    if (e.key === "Escape") cancelRename();
                  }}
                  onClick={(e) => e.stopPropagation()}
                  className="w-24 rounded bg-muted/60 px-1 text-[11px] text-foreground outline-none ring-1 ring-primary/60"
                  autoFocus
                />
              ) : (
                <span>{db.name}</span>
              )}

              {/* Count badge */}
              <span className={cn(
                "rounded px-1 py-0.5 text-[9px] tabular-nums leading-none",
                db.id === activeDatabankId
                  ? "bg-primary/15 text-primary"
                  : "bg-muted/40 text-muted-foreground/60"
              )}>
                {db.strategies.length}
              </span>

              {/* Delete tab button */}
              {builderDatabanks.length > 1 && (
                <button
                  onClick={(e) => handleDeleteTab(e, db.id)}
                  className="ml-0.5 hidden h-3.5 w-3.5 items-center justify-center rounded text-muted-foreground/40 hover:text-red-400 group-hover:flex"
                >
                  <X className="h-2.5 w-2.5" />
                </button>
              )}
            </div>
          ))}
        </div>

        {/* New databank button */}
        <button
          onClick={handleNewDatabank}
          title="New databank"
          className="ml-1 flex h-6 w-6 shrink-0 items-center justify-center rounded border border-border/30 text-muted-foreground/50 hover:border-primary/50 hover:text-primary"
        >
          <Plus className="h-3.5 w-3.5" />
        </button>

        {/* Spacer */}
        <div className="flex-1" />

        {/* Toolbar buttons */}
        <div className="flex items-center gap-1 pr-1">
          <span className="mr-2 text-[10px] text-muted-foreground/50">
            Records: <span className="font-semibold text-foreground">{strategies.length}</span>
            {selected.size > 0 && (
              <span className="ml-1.5 text-primary">({selected.size} selected)</span>
            )}
          </span>

          {/* Set as target */}
          {activeBank && activeBank.id !== targetDatabankId && (
            <button
              onClick={() => setTargetDatabankId(activeBank.id)}
              title="Set as target databank (new strategies will go here)"
              className="flex items-center gap-1 rounded border border-border/30 px-2 py-1 text-[10px] text-muted-foreground hover:border-emerald-500/50 hover:text-emerald-400"
            >
              Set as target
            </button>
          )}

          {/* Rename */}
          {activeBank && (
            <button
              onClick={() => startRename(activeBank.id, activeBank.name)}
              title="Rename databank"
              className="flex items-center gap-1 rounded border border-border/30 px-2 py-1 text-[10px] text-muted-foreground hover:border-primary/50 hover:text-primary"
            >
              <Pencil className="h-3 w-3" />
              {t("results.rename")}
            </button>
          )}

          {/* Delete selected */}
          <button
            disabled={selected.size === 0}
            onClick={handleDeleteSelected}
            title="Delete selected strategies"
            className="flex items-center gap-1 rounded border border-border/30 px-2 py-1 text-[10px] text-muted-foreground hover:border-destructive/50 hover:text-destructive disabled:opacity-40"
          >
            <Trash2 className="h-3 w-3" />
            {t("results.delete")}
          </button>

          {/* Clear databank */}
          <button
            disabled={strategies.length === 0}
            onClick={() => activeBank && clearBuilderDatabank(activeBank.id)}
            title="Clear all strategies from this databank"
            className="flex items-center gap-1 rounded border border-border/30 px-2 py-1 text-[10px] text-muted-foreground hover:border-destructive/50 hover:text-destructive disabled:opacity-40"
          >
            <RotateCcw className="h-3 w-3" />
            {t("results.clearAll")}
          </button>

          {/* IS / OOS toggle */}
          <button
            onClick={() => setShowOos((v) => !v)}
            className={cn(
              "rounded border px-2 py-1 text-[10px]",
              showOos
                ? "border-primary/50 bg-primary/10 text-primary"
                : "border-border/30 text-muted-foreground hover:border-border/60 hover:text-foreground"
            )}
            title={showOos ? "Showing OOS metrics \u2014 click for IS" : "Show OOS metrics"}
          >
            {showOos ? "\u2190 IS" : "OOS \u2192"}
          </button>

          {/* Clear ALL databanks */}
          <button
            onClick={clearAllBuilderDatabanks}
            title="Clear all strategies from ALL databanks"
            className="rounded border border-border/30 px-2 py-1 text-[10px] text-muted-foreground hover:border-destructive/50 hover:text-destructive"
          >
            Clear all databanks
          </button>
        </div>
      </div>

      {/* ── Strategies table ─────────────────────────────────────────────── */}
      {strategies.length === 0 ? (
        <div className="flex flex-1 items-center justify-center">
          <span className="text-xs text-muted-foreground/40">
            {activeBank?.id === targetDatabankId
              ? "No strategies yet — run the builder to populate this databank"
              : "No strategies in this databank"}
          </span>
        </div>
      ) : (
        <div className="flex-1 overflow-auto">
          <table className="w-full border-collapse text-xs">
            <thead className="sticky top-0 z-10 bg-background">
              <tr className="border-b border-border/30">
                <th className="w-7 px-2 py-1.5">
                  <input
                    type="checkbox"
                    checked={selected.size === sorted.length && sorted.length > 0}
                    onChange={toggleAll}
                    className="h-3 w-3 accent-primary"
                  />
                </th>
                <th className="w-5 px-1.5 py-1.5 text-center text-[9px] font-medium uppercase tracking-wider text-muted-foreground/50">
                  #
                </th>
                {activeColumns.map((col) => (
                  <th
                    key={col.key}
                    style={{ minWidth: col.width }}
                    onClick={() => handleSort(col)}
                    className={cn(
                      "whitespace-nowrap px-2 py-1.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60",
                      col.noSort ? "cursor-default" : "cursor-pointer select-none hover:text-muted-foreground",
                      col.align === "right" ? "text-right" : col.align === "center" ? "text-center" : "text-left"
                    )}
                  >
                    <span className="inline-flex items-center gap-0.5">
                      {col.label}
                      {!col.noSort && <SortIcon col={col.key} sortKey={sortKey} sortDir={sortDir} />}
                    </span>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {sorted.map((strategy, idx) => (
                <tr
                  key={strategy.id}
                  onClick={() => toggleSelect(strategy.id)}
                  onDoubleClick={() => onStrategyOpen?.(strategy)}
                  title="Double-click to view full backtest"
                  className={cn(
                    "cursor-pointer border-b border-border/10 transition-colors",
                    selected.has(strategy.id) ? "bg-primary/10" : "hover:bg-muted/20"
                  )}
                >
                  <td className="px-2 py-1">
                    <input
                      type="checkbox"
                      checked={selected.has(strategy.id)}
                      onChange={() => toggleSelect(strategy.id)}
                      onClick={(e) => e.stopPropagation()}
                      className="h-3 w-3 accent-primary"
                    />
                  </td>
                  <td className="px-1.5 py-1 text-center text-[9px] tabular-nums text-muted-foreground/40">
                    {idx + 1}
                  </td>
                  {activeColumns.map((col) => (
                    <td
                      key={col.key}
                      className={cn(
                        "px-2 py-1",
                        col.align === "right" ? "text-right" : col.align === "center" ? "text-center" : "text-left"
                      )}
                    >
                      {col.render(strategy)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
