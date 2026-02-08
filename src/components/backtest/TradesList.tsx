import { useState, useMemo } from "react";
import type { TradeResult } from "@/lib/types";
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from "@/components/ui/Table";
import { Button } from "@/components/ui/Button";
import { ChevronLeft, ChevronRight, ArrowUpDown } from "lucide-react";

const PAGE_SIZE = 20;

type SortKey = keyof TradeResult;
type SortDir = "asc" | "desc";

const COLUMNS: { key: SortKey; label: string; align?: "right" }[] = [
  { key: "direction", label: "Dir" },
  { key: "entry_time", label: "Entry Time" },
  { key: "entry_price", label: "Entry", align: "right" },
  { key: "exit_time", label: "Exit Time" },
  { key: "exit_price", label: "Exit", align: "right" },
  { key: "lots", label: "Lots", align: "right" },
  { key: "pnl", label: "P&L", align: "right" },
  { key: "pnl_pips", label: "Pips", align: "right" },
  { key: "commission", label: "Comm", align: "right" },
  { key: "close_reason", label: "Reason" },
  { key: "duration_time", label: "Duration" },
];

interface TradesListProps {
  trades: TradeResult[];
}

export function TradesList({ trades }: TradesListProps) {
  const [page, setPage] = useState(0);
  const [sortKey, setSortKey] = useState<SortKey>("entry_time");
  const [sortDir, setSortDir] = useState<SortDir>("asc");

  const sorted = useMemo(() => {
    const arr = [...trades];
    arr.sort((a, b) => {
      const va = a[sortKey];
      const vb = b[sortKey];
      let cmp = 0;
      if (typeof va === "number" && typeof vb === "number") {
        cmp = va - vb;
      } else {
        cmp = String(va).localeCompare(String(vb));
      }
      return sortDir === "asc" ? cmp : -cmp;
    });
    return arr;
  }, [trades, sortKey, sortDir]);

  const totalPages = Math.max(1, Math.ceil(sorted.length / PAGE_SIZE));
  const paginated = sorted.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

  const handleSort = (key: SortKey) => {
    if (key === sortKey) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      setSortDir("asc");
    }
    setPage(0);
  };

  if (trades.length === 0) {
    return (
      <p className="py-8 text-center text-sm text-muted-foreground">
        No trades to display.
      </p>
    );
  }

  return (
    <div className="space-y-2">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-10">#</TableHead>
            {COLUMNS.map((col) => (
              <TableHead
                key={col.key}
                className={`cursor-pointer select-none whitespace-nowrap ${
                  col.align === "right" ? "text-right" : ""
                }`}
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
          </TableRow>
        </TableHeader>
        <TableBody>
          {paginated.map((trade, idx) => {
            const isWin = trade.pnl > 0;
            const rowClass = isWin
              ? "bg-emerald-500/5"
              : trade.pnl < 0
                ? "bg-red-500/5"
                : "";
            return (
              <TableRow key={trade.id} className={rowClass}>
                <TableCell className="text-xs text-muted-foreground">
                  {page * PAGE_SIZE + idx + 1}
                </TableCell>
                <TableCell className="text-xs">
                  <span
                    className={
                      trade.direction === "Long"
                        ? "text-emerald-500"
                        : "text-red-500"
                    }
                  >
                    {trade.direction === "Long" ? "L" : "S"}
                  </span>
                </TableCell>
                <TableCell className="text-xs">{trade.entry_time.slice(0, 16)}</TableCell>
                <TableCell className="text-right text-xs">{trade.entry_price.toFixed(5)}</TableCell>
                <TableCell className="text-xs">{trade.exit_time.slice(0, 16)}</TableCell>
                <TableCell className="text-right text-xs">{trade.exit_price.toFixed(5)}</TableCell>
                <TableCell className="text-right text-xs">{trade.lots.toFixed(2)}</TableCell>
                <TableCell
                  className={`text-right text-xs font-medium ${
                    isWin ? "text-emerald-500" : trade.pnl < 0 ? "text-red-500" : ""
                  }`}
                >
                  ${trade.pnl.toFixed(2)}
                </TableCell>
                <TableCell
                  className={`text-right text-xs ${
                    isWin ? "text-emerald-500" : trade.pnl < 0 ? "text-red-500" : ""
                  }`}
                >
                  {trade.pnl_pips.toFixed(1)}
                </TableCell>
                <TableCell className="text-right text-xs">${trade.commission.toFixed(2)}</TableCell>
                <TableCell className="text-xs">{trade.close_reason}</TableCell>
                <TableCell className="text-xs">{trade.duration_time}</TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>

      {/* Pagination */}
      <div className="flex items-center justify-between">
        <span className="text-xs text-muted-foreground">
          {trades.length} trade{trades.length !== 1 ? "s" : ""} total
        </span>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={page === 0}
            onClick={() => setPage((p) => p - 1)}
          >
            <ChevronLeft className="h-4 w-4" />
          </Button>
          <span className="text-xs text-muted-foreground">
            {page + 1} / {totalPages}
          </span>
          <Button
            variant="outline"
            size="sm"
            disabled={page >= totalPages - 1}
            onClick={() => setPage((p) => p + 1)}
          >
            <ChevronRight className="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>
  );
}
