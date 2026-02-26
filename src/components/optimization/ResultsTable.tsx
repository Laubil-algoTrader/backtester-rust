import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { OptimizationResult, ParameterRange } from "@/lib/types";
import { useAppStore } from "@/stores/useAppStore";
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from "@/components/ui/Table";
import { Button } from "@/components/ui/Button";
import { ArrowUpDown, Check } from "lucide-react";
import { SparklineChart } from "@/components/backtest/SparklineChart";

type SortKey = string;
type SortDir = "asc" | "desc";

/** Heatmap background — maps a normalized 0..1 value to a subtle bg color.
 *  0 = deep red, 0.5 = dark yellow/neutral, 1 = emerald green.
 *  Similar to Bloomberg Historical Yields table styling. */
function heatmapBg(val: number, min: number, max: number): string | undefined {
  if (max === min) return undefined;
  const t = Math.max(0, Math.min(1, (val - min) / (max - min)));
  if (t < 0.33) {
    // Red zone
    const opacity = 0.08 + (0.33 - t) * 0.3;
    return `rgba(220, 38, 38, ${opacity.toFixed(3)})`;
  } else if (t < 0.66) {
    // Yellow/neutral zone
    const opacity = 0.03;
    return `rgba(202, 138, 4, ${opacity.toFixed(3)})`;
  } else {
    // Green zone
    const opacity = 0.08 + (t - 0.66) * 0.3;
    return `rgba(16, 185, 129, ${opacity.toFixed(3)})`;
  }
}

interface ResultsTableProps {
  results: OptimizationResult[];
  parameterRanges: ParameterRange[];
  onApply: (params: Record<string, number>) => void;
  isMultiObjective?: boolean;
}

export function ResultsTable({
  results,
  parameterRanges,
  onApply,
  isMultiObjective = false,
}: ResultsTableProps) {
  const { t } = useTranslation("optimization");
  const [sortKey, setSortKey] = useState<SortKey>(isMultiObjective ? "composite_score" : "objective_value");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const storedRanges = useAppStore((s) => s.optimizationParamRanges);

  // Derive param names: prefer explicit ranges, fallback to stored ranges, then extract from results
  const paramNames = useMemo(() => {
    if (parameterRanges.length > 0) return parameterRanges.map((r) => r.display_name);
    if (storedRanges.length > 0) return storedRanges.map((r) => r.display_name);
    if (results.length > 0) return Object.keys(results[0].params);
    return [];
  }, [parameterRanges, storedRanges, results]);

  // Detect OOS labels from first result
  const oosLabels = useMemo(() => {
    if (results.length === 0) return [];
    return results[0].oos_results.map((o) => o.label);
  }, [results]);

  const hasOos = oosLabels.length > 0;

  // Helper to get OOS value for sorting
  const getOosValue = (result: OptimizationResult, key: string): number => {
    // key format: "oos_0_total_return_pct", "oos_1_sharpe_ratio", etc.
    const match = key.match(/^oos_(\d+)_(.+)$/);
    if (!match) return 0;
    const idx = parseInt(match[1]);
    const field = match[2];
    const oos = result.oos_results[idx];
    if (!oos) return 0;
    return (oos as unknown as Record<string, number>)[field] ?? 0;
  };

  const sorted = useMemo(() => {
    const arr = [...results];
    arr.sort((a, b) => {
      if (sortKey === "rank") return 0;

      let va: number;
      let vb: number;

      if (sortKey.startsWith("oos_")) {
        va = getOosValue(a, sortKey);
        vb = getOosValue(b, sortKey);
      } else if (a.params[sortKey] !== undefined) {
        va = a.params[sortKey] ?? 0;
        vb = b.params[sortKey] ?? 0;
      } else {
        va = (a as unknown as Record<string, number>)[sortKey] ?? 0;
        vb = (b as unknown as Record<string, number>)[sortKey] ?? 0;
      }

      const cmp = va - vb;
      return sortDir === "asc" ? cmp : -cmp;
    });
    return arr;
  }, [results, sortKey, sortDir]);

  const handleSort = (key: SortKey) => {
    if (key === sortKey) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      setSortDir("desc");
    }
  };

  if (results.length === 0) {
    return (
      <p className="py-8 text-center text-sm text-muted-foreground">
        {t("noResults")}
      </p>
    );
  }

  const fixedColumns: { key: SortKey; label: string; format: (v: number) => string }[] = [
    ...(isMultiObjective
      ? [{ key: "composite_score" as SortKey, label: t("table.composite"), format: (v: number) => v.toFixed(3) }]
      : []),
    { key: "objective_value", label: t("table.objective"), format: (v) => v.toFixed(2) },
    { key: "total_return_pct", label: "IS RET%", format: (v) => `${v.toFixed(2)}%` },
    { key: "sharpe_ratio", label: "IS SHARPE", format: (v) => v.toFixed(2) },
    { key: "max_drawdown_pct", label: "IS DD%", format: (v) => `${v.toFixed(2)}%` },
    { key: "profit_factor", label: "IS PF", format: (v) => v.toFixed(2) },
    { key: "return_dd_ratio", label: "IS RET/DD", format: (v) => v.toFixed(2) },
    { key: "total_trades", label: "IS TRADES", format: (v) => String(v) },
    { key: "stagnation_bars", label: "STAG", format: (v) => String(v) },
    { key: "ulcer_index_pct", label: "ULCER%", format: (v) => `${v.toFixed(2)}%` },
  ];

  // OOS columns: for each OOS period, add Ret%, Sharpe, DD%
  const oosColumns: { key: string; label: string; format: (v: number) => string; oosIdx: number; field: string }[] = [];
  for (let i = 0; i < oosLabels.length; i++) {
    const short = oosLabels[i];
    oosColumns.push({ key: `oos_${i}_total_return_pct`, label: `${short} RET%`, format: (v) => `${v.toFixed(2)}%`, oosIdx: i, field: "total_return_pct" });
    oosColumns.push({ key: `oos_${i}_sharpe_ratio`, label: `${short} SHARPE`, format: (v) => v.toFixed(2), oosIdx: i, field: "sharpe_ratio" });
    oosColumns.push({ key: `oos_${i}_max_drawdown_pct`, label: `${short} DD%`, format: (v) => `${v.toFixed(2)}%`, oosIdx: i, field: "max_drawdown_pct" });
  }

  // Heatmap keys: columns that get background coloring
  const heatmapKeys = new Set(["total_return_pct", "sharpe_ratio", "profit_factor", "return_dd_ratio"]);
  // Inverted: lower is better (drawdown)
  const invertedKeys = new Set(["max_drawdown_pct", "ulcer_index_pct", "stagnation_bars"]);

  // Pre-compute min/max for heatmap columns
  const colRanges = useMemo(() => {
    const ranges: Record<string, { min: number; max: number }> = {};
    const keys = [...heatmapKeys, ...invertedKeys];
    for (const key of keys) {
      let min = Infinity;
      let max = -Infinity;
      for (const r of results) {
        const v = (r as unknown as Record<string, number>)[key] ?? 0;
        if (v < min) min = v;
        if (v > max) max = v;
      }
      ranges[key] = { min, max };
    }
    return ranges;
  }, [results]);

  const getCellBg = (key: string, val: number): string | undefined => {
    const range = colRanges[key];
    if (!range) return undefined;
    if (invertedKeys.has(key)) {
      // Invert: lower value = green, higher = red
      return heatmapBg(val, range.max, range.min);
    }
    return heatmapBg(val, range.min, range.max);
  };

  const SortableHead = ({ sortKeyVal, label }: { sortKeyVal: string; label: string }) => (
    <TableHead
      className="cursor-pointer select-none whitespace-nowrap text-right text-xs"
      onClick={() => handleSort(sortKeyVal)}
    >
      <span className="inline-flex items-center gap-1">
        {label}
        {sortKey === sortKeyVal && <ArrowUpDown className="h-3 w-3 opacity-60" />}
      </span>
    </TableHead>
  );

  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-10 text-xs">#</TableHead>
            <TableHead className="w-24 text-xs">Equity</TableHead>
            {paramNames.map((name) => (
              <SortableHead key={name} sortKeyVal={name} label={name.length > 20 ? name.slice(0, 20) + "..." : name} />
            ))}
            {fixedColumns.map((col) => (
              <SortableHead key={col.key} sortKeyVal={col.key} label={col.label} />
            ))}
            {hasOos && oosColumns.map((col) => (
              <TableHead
                key={col.key}
                className="cursor-pointer select-none whitespace-nowrap border-l border-border/30 text-right text-xs text-amber-400/70"
                onClick={() => handleSort(col.key)}
              >
                <span className="inline-flex items-center gap-1">
                  {col.label}
                  {sortKey === col.key && <ArrowUpDown className="h-3 w-3 opacity-60" />}
                </span>
              </TableHead>
            ))}
            <TableHead className="w-16 text-xs">APPLY</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sorted.map((result, idx) => (
            <TableRow
              key={idx}
              className={idx === 0 ? "bg-emerald-500/[0.03]" : ""}
            >
              <TableCell className="text-sm font-mono text-muted-foreground">
                {idx + 1}
              </TableCell>
              <TableCell className="p-1">
                {result.equity_curve && result.equity_curve.length > 2 ? (
                  <SparklineChart
                    data={result.equity_curve.map((p) => ({ value: p.equity }))}
                    color={result.total_return_pct >= 0 ? "green" : "red"}
                    height={32}
                    id={`opt-eq-${idx}`}
                  />
                ) : (
                  <span className="text-xs text-muted-foreground">—</span>
                )}
              </TableCell>
              {paramNames.map((name) => (
                <TableCell key={name} className="text-right text-sm font-mono">
                  {(result.params[name] ?? 0).toFixed(
                    Number.isInteger(result.params[name]) ? 0 : 2
                  )}
                </TableCell>
              ))}
              {fixedColumns.map((col) => {
                const val = (result as unknown as Record<string, number>)[col.key] ?? 0;
                const bg = getCellBg(col.key, val);
                return (
                  <TableCell
                    key={col.key}
                    className="text-right text-sm font-mono"
                    style={bg ? { backgroundColor: bg } : undefined}
                  >
                    {col.format(val)}
                  </TableCell>
                );
              })}
              {hasOos && oosColumns.map((col) => {
                const oos = result.oos_results[col.oosIdx];
                const val = oos ? (oos as unknown as Record<string, number>)[col.field] ?? 0 : 0;
                return (
                  <TableCell
                    key={col.key}
                    className="border-l border-border/10 text-right text-sm font-mono"
                  >
                    {col.format(val)}
                  </TableCell>
                );
              })}
              <TableCell>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 w-6 p-0"
                  onClick={() => onApply(result.params)}
                  title="Apply these parameters"
                >
                  <Check className="h-3.5 w-3.5" />
                </Button>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
