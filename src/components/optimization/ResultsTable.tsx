import { useState, useMemo } from "react";
import type { OptimizationResult, ParameterRange } from "@/lib/types";
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

type SortKey = string;
type SortDir = "asc" | "desc";

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
  const [sortKey, setSortKey] = useState<SortKey>(isMultiObjective ? "composite_score" : "objective_value");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  const paramNames = parameterRanges.map((r) => r.display_name);

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
        No optimization results yet.
      </p>
    );
  }

  const fixedColumns: { key: SortKey; label: string; format: (v: number) => string }[] = [
    ...(isMultiObjective
      ? [{ key: "composite_score" as SortKey, label: "COMPOSITE", format: (v: number) => v.toFixed(3) }]
      : []),
    { key: "objective_value", label: "OBJECTIVE", format: (v) => v.toFixed(2) },
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

  const SortableHead = ({ sortKeyVal, label }: { sortKeyVal: string; label: string }) => (
    <TableHead
      className="cursor-pointer select-none whitespace-nowrap text-right text-[10px] tracking-widest"
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
            <TableHead className="w-10 text-[10px] tracking-widest">#</TableHead>
            {paramNames.map((name) => (
              <SortableHead key={name} sortKeyVal={name} label={name.length > 20 ? name.slice(0, 20) + "..." : name} />
            ))}
            {fixedColumns.map((col) => (
              <SortableHead key={col.key} sortKeyVal={col.key} label={col.label} />
            ))}
            {hasOos && oosColumns.map((col) => (
              <TableHead
                key={col.key}
                className="cursor-pointer select-none whitespace-nowrap border-l border-border/30 text-right text-[10px] tracking-widest text-amber-400/70"
                onClick={() => handleSort(col.key)}
              >
                <span className="inline-flex items-center gap-1">
                  {col.label}
                  {sortKey === col.key && <ArrowUpDown className="h-3 w-3 opacity-60" />}
                </span>
              </TableHead>
            ))}
            <TableHead className="w-16 text-[10px] tracking-widest">APPLY</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sorted.map((result, idx) => (
            <TableRow
              key={idx}
              className={idx === 0 ? "bg-emerald-500/[0.03]" : ""}
            >
              <TableCell className="text-xs text-muted-foreground">
                {idx + 1}
              </TableCell>
              {paramNames.map((name) => (
                <TableCell key={name} className="text-right text-xs">
                  {(result.params[name] ?? 0).toFixed(
                    Number.isInteger(result.params[name]) ? 0 : 2
                  )}
                </TableCell>
              ))}
              {fixedColumns.map((col) => {
                const val = (result as unknown as Record<string, number>)[col.key] ?? 0;
                const isPositive = col.key === "total_return_pct" ? val > 0 : col.key === "sharpe_ratio" ? val > 0 : false;
                const isNegative = col.key === "total_return_pct" ? val < 0 : false;
                return (
                  <TableCell
                    key={col.key}
                    className={`text-right text-xs ${
                      isPositive
                        ? "text-emerald-400"
                        : isNegative
                          ? "text-red-400"
                          : ""
                    }`}
                  >
                    {col.format(val)}
                  </TableCell>
                );
              })}
              {hasOos && oosColumns.map((col) => {
                const oos = result.oos_results[col.oosIdx];
                const val = oos ? (oos as unknown as Record<string, number>)[col.field] ?? 0 : 0;
                const isReturnCol = col.field === "total_return_pct";
                const isSharpeCol = col.field === "sharpe_ratio";
                return (
                  <TableCell
                    key={col.key}
                    className={`border-l border-border/10 text-right text-xs ${
                      isReturnCol && val > 0 ? "text-emerald-400" :
                      isReturnCol && val < 0 ? "text-red-400" :
                      isSharpeCol && val > 0 ? "text-emerald-400" :
                      isSharpeCol && val < 0 ? "text-red-400" :
                      ""
                    }`}
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
