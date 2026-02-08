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

type SortKey = "rank" | "objective_value" | "total_return_pct" | "sharpe_ratio" | "max_drawdown_pct" | "total_trades" | "profit_factor" | string;
type SortDir = "asc" | "desc";

interface ResultsTableProps {
  results: OptimizationResult[];
  parameterRanges: ParameterRange[];
  onApply: (params: Record<string, number>) => void;
}

export function ResultsTable({
  results,
  parameterRanges,
  onApply,
}: ResultsTableProps) {
  const [sortKey, setSortKey] = useState<SortKey>("objective_value");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  // Dynamic param column names from the ranges
  const paramNames = parameterRanges.map((r) => r.display_name);

  const sorted = useMemo(() => {
    const arr = [...results];
    arr.sort((a, b) => {
      let va: number;
      let vb: number;

      if (sortKey === "rank") return 0; // no re-sort on rank

      // Check if it's a param column
      if (a.params[sortKey] !== undefined) {
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
    { key: "objective_value", label: "Objective", format: (v) => v.toFixed(2) },
    { key: "total_return_pct", label: "Return %", format: (v) => `${v.toFixed(2)}%` },
    { key: "sharpe_ratio", label: "Sharpe", format: (v) => v.toFixed(2) },
    { key: "max_drawdown_pct", label: "Max DD %", format: (v) => `${v.toFixed(2)}%` },
    { key: "total_trades", label: "Trades", format: (v) => String(v) },
    { key: "profit_factor", label: "PF", format: (v) => v.toFixed(2) },
  ];

  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-10">#</TableHead>
            {paramNames.map((name) => (
              <TableHead
                key={name}
                className="cursor-pointer select-none whitespace-nowrap text-right"
                onClick={() => handleSort(name)}
              >
                <span className="inline-flex items-center gap-1">
                  {name.length > 20 ? name.slice(0, 20) + "..." : name}
                  {sortKey === name && (
                    <ArrowUpDown className="h-3 w-3 opacity-60" />
                  )}
                </span>
              </TableHead>
            ))}
            {fixedColumns.map((col) => (
              <TableHead
                key={col.key}
                className="cursor-pointer select-none whitespace-nowrap text-right"
                onClick={() => handleSort(col.key)}
              >
                <span className="inline-flex items-center gap-1">
                  {col.label}
                  {sortKey === col.key && (
                    <ArrowUpDown className="h-3 w-3 opacity-60" />
                  )}
                </span>
              </TableHead>
            ))}
            <TableHead className="w-16">Apply</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sorted.map((result, idx) => (
            <TableRow
              key={idx}
              className={idx === 0 ? "bg-emerald-500/5" : ""}
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
                        ? "text-emerald-500"
                        : isNegative
                          ? "text-red-500"
                          : ""
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
