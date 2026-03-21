import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { ChevronUp, ChevronDown, ChevronsUpDown, Trash2 } from "lucide-react";
import { useAppStore } from "@/stores/useAppStore";
import type { BuilderSavedStrategy } from "@/lib/types";
import { cn } from "@/lib/utils";

// ── Sparkline ────────────────────────────────────────────────────────────────

function MiniSparkline({ data }: { data: number[] }) {
  if (data.length < 2) return <span className="text-[10px] text-muted-foreground/30">—</span>;
  const min = Math.min(...data);
  const max = Math.max(...data);
  const range = max - min || 1;
  const w = 72;
  const h = 28;
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

// ── Sort helpers ─────────────────────────────────────────────────────────────

type SortKey = keyof BuilderSavedStrategy;
type SortDir = "asc" | "desc";

function SortIcon({ col, sortKey, sortDir }: { col: SortKey; sortKey: SortKey; sortDir: SortDir }) {
  if (col !== sortKey) return <ChevronsUpDown className="h-3 w-3 text-muted-foreground/30" />;
  return sortDir === "asc"
    ? <ChevronUp className="h-3 w-3 text-primary" />
    : <ChevronDown className="h-3 w-3 text-primary" />;
}

// ── Column definitions ───────────────────────────────────────────────────────

interface ColDef {
  key: SortKey;
  labelKey: string;
  width: string;
  align?: "left" | "right" | "center";
  noSort?: boolean;
  render: (s: BuilderSavedStrategy) => React.ReactNode;
}

const fmt = (n: number, dec = 2) =>
  n.toLocaleString(undefined, { minimumFractionDigits: dec, maximumFractionDigits: dec });

const fmtMoney = (n: number) =>
  (n >= 0 ? "+" : "") + "$" + Math.abs(n).toLocaleString(undefined, { maximumFractionDigits: 0 });

const COLUMNS: ColDef[] = [
  {
    key: "name",
    labelKey: "columns.name",
    width: "140px",
    align: "left",
    render: (s) => (
      <span className="truncate font-medium text-foreground" title={s.name}>{s.name}</span>
    ),
  },
  {
    key: "fitness",
    labelKey: "columns.fitness",
    width: "72px",
    align: "right",
    render: (s) => <span className="font-bold text-primary tabular-nums">{s.fitness.toFixed(4)}</span>,
  },
  {
    key: "symbolName",
    labelKey: "columns.symbol",
    width: "90px",
    align: "left",
    render: (s) => <span className="text-muted-foreground">{s.symbolName}</span>,
  },
  {
    key: "timeframe",
    labelKey: "columns.timeframe",
    width: "60px",
    align: "center",
    render: (s) => <span className="uppercase text-muted-foreground">{s.timeframe}</span>,
  },
  {
    key: "netProfit",
    labelKey: "columns.netProfit",
    width: "96px",
    align: "right",
    render: (s) => (
      <span className={cn("tabular-nums font-medium", s.netProfit >= 0 ? "text-emerald-400" : "text-red-400")}>
        {fmtMoney(s.netProfit)}
      </span>
    ),
  },
  {
    key: "miniEquityCurve",
    labelKey: "columns.equityChart",
    width: "80px",
    align: "center",
    noSort: true,
    render: (s) => <MiniSparkline data={s.miniEquityCurve} />,
  },
  {
    key: "trades",
    labelKey: "columns.trades",
    width: "56px",
    align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{s.trades}</span>,
  },
  {
    key: "profitFactor",
    labelKey: "columns.profitFactor",
    width: "64px",
    align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{fmt(s.profitFactor)}</span>,
  },
  {
    key: "sharpeRatio",
    labelKey: "columns.sharpe",
    width: "60px",
    align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{fmt(s.sharpeRatio)}</span>,
  },
  {
    key: "rExpectancy",
    labelKey: "columns.rExpectancy",
    width: "72px",
    align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{fmt(s.rExpectancy)}</span>,
  },
  {
    key: "annualReturnPct",
    labelKey: "columns.annualReturn",
    width: "72px",
    align: "right",
    render: (s) => (
      <span className={cn("tabular-nums", s.annualReturnPct >= 0 ? "text-emerald-400" : "text-red-400")}>
        {fmt(s.annualReturnPct)}%
      </span>
    ),
  },
  {
    key: "maxDrawdownAbs",
    labelKey: "columns.drawdown",
    width: "80px",
    align: "right",
    render: (s) => (
      <span className="tabular-nums text-red-400">
        ${s.maxDrawdownAbs.toLocaleString(undefined, { maximumFractionDigits: 0 })}
      </span>
    ),
  },
  {
    key: "retDDRatio",
    labelKey: "columns.retDD",
    width: "60px",
    align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{fmt(s.retDDRatio)}</span>,
  },
  {
    key: "avgWin",
    labelKey: "columns.avgWin",
    width: "72px",
    align: "right",
    render: (s) => <span className="tabular-nums text-emerald-400">${fmt(s.avgWin, 0)}</span>,
  },
  {
    key: "avgLoss",
    labelKey: "columns.avgLoss",
    width: "72px",
    align: "right",
    render: (s) => <span className="tabular-nums text-red-400">-${fmt(Math.abs(s.avgLoss), 0)}</span>,
  },
  {
    key: "avgBarsWin",
    labelKey: "columns.avgBars",
    width: "56px",
    align: "right",
    render: (s) => <span className="tabular-nums text-foreground">{fmt(s.avgBarsWin, 1)}</span>,
  },
];

// ── Main component ───────────────────────────────────────────────────────────

export function ResultsTab() {
  const { t } = useTranslation("builder");
  const { builderDatabanks, activeDatabankId, clearBuilderDatabank, setBuilderTopTab } = useAppStore();
  const builderDatabank = builderDatabanks.find((db) => db.id === activeDatabankId)?.strategies ?? [];

  const [sortKey, setSortKey] = useState<SortKey>("fitness");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const handleSort = (col: ColDef) => {
    if (col.noSort) return;
    if (col.key === sortKey) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(col.key);
      setSortDir("desc");
    }
  };

  const sorted = useMemo(() => {
    return [...builderDatabank].sort((a, b) => {
      const av = a[sortKey];
      const bv = b[sortKey];
      if (typeof av === "number" && typeof bv === "number") {
        return sortDir === "asc" ? av - bv : bv - av;
      }
      if (typeof av === "string" && typeof bv === "string") {
        return sortDir === "asc" ? av.localeCompare(bv) : bv.localeCompare(av);
      }
      return 0;
    });
  }, [builderDatabank, sortKey, sortDir]);

  const toggleSelect = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });

  const toggleAll = () =>
    setSelected(selected.size === sorted.length ? new Set() : new Set(sorted.map((s) => s.id)));

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Toolbar */}
      <div className="flex shrink-0 items-center gap-1 border-b border-border/30 bg-background/50 px-4 py-2">
        <div className="flex items-center gap-1">
          <button className="rounded border border-border/40 px-3 py-1 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary">
            {t("results.load")}
          </button>
          <button className="rounded border border-border/40 px-3 py-1 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary">
            {t("results.save")}
          </button>
          <button
            disabled={selected.size === 0}
            className="rounded border border-border/40 px-3 py-1 text-xs text-muted-foreground hover:border-destructive/50 hover:text-destructive disabled:opacity-40"
          >
            {t("results.delete")}
          </button>
          <button
            onClick={() => { clearBuilderDatabank(activeDatabankId); setSelected(new Set()); }}
            disabled={builderDatabank.length === 0}
            className="flex items-center gap-1 rounded border border-border/40 px-3 py-1 text-xs text-muted-foreground hover:border-destructive/50 hover:text-destructive disabled:opacity-40"
          >
            <Trash2 className="h-3 w-3" />
            {t("results.clearAll")}
          </button>
        </div>

        <div className="ml-auto flex items-center gap-3">
          <span className="text-[10px] text-muted-foreground/60">
            {t("results.databankCount")}:{" "}
            <span className="font-bold text-foreground">{builderDatabank.length}</span>
            {selected.size > 0 && (
              <span className="ml-2 text-primary">({selected.size} selected)</span>
            )}
          </span>
          <button
            onClick={() => setBuilderTopTab("progress")}
            className="rounded border border-border/40 px-3 py-1 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary"
          >
            ← {t("tabs.progress")}
          </button>
        </div>
      </div>

      {/* Empty state */}
      {builderDatabank.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3">
          <p className="text-sm text-muted-foreground/50">{t("results.noStrategies")}</p>
          <button
            onClick={() => setBuilderTopTab("progress")}
            className="rounded bg-primary px-4 py-2 text-sm text-primary-foreground hover:opacity-90"
          >
            Go to Progress → Start building
          </button>
        </div>
      ) : (
        /* Scrollable table */
        <div className="flex-1 overflow-auto">
          <table className="w-full border-collapse text-xs">
            <thead className="sticky top-0 z-10 bg-background">
              <tr className="border-b border-border/30">
                <th className="w-8 px-3 py-2">
                  <input
                    type="checkbox"
                    checked={selected.size === sorted.length && sorted.length > 0}
                    onChange={toggleAll}
                    className="h-3.5 w-3.5 accent-primary"
                  />
                </th>
                <th className="w-6 px-2 py-2 text-center text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
                  #
                </th>
                {COLUMNS.map((col) => (
                  <th
                    key={col.key}
                    style={{ minWidth: col.width }}
                    onClick={() => handleSort(col)}
                    className={cn(
                      "whitespace-nowrap px-2 py-2 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60",
                      col.noSort ? "cursor-default" : "cursor-pointer select-none hover:text-muted-foreground",
                      col.align === "right" ? "text-right" : col.align === "center" ? "text-center" : "text-left"
                    )}
                  >
                    <span className="inline-flex items-center gap-0.5">
                      {t(`results.${col.labelKey}`)}
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
                  className={cn(
                    "cursor-pointer border-b border-border/10 transition-colors",
                    selected.has(strategy.id) ? "bg-primary/10" : "hover:bg-muted/20"
                  )}
                >
                  <td className="px-3 py-1.5">
                    <input
                      type="checkbox"
                      checked={selected.has(strategy.id)}
                      onChange={() => toggleSelect(strategy.id)}
                      onClick={(e) => e.stopPropagation()}
                      className="h-3.5 w-3.5 accent-primary"
                    />
                  </td>
                  <td className="px-2 py-1.5 text-center text-[10px] tabular-nums text-muted-foreground/50">
                    {idx + 1}
                  </td>
                  {COLUMNS.map((col) => (
                    <td
                      key={col.key}
                      className={cn(
                        "px-2 py-1.5",
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
