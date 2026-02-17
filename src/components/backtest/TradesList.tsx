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
  { key: "direction", label: "DIR" },
  { key: "entry_time", label: "ENTRY TIME" },
  { key: "entry_price", label: "ENTRY", align: "right" },
  { key: "exit_time", label: "EXIT TIME" },
  { key: "exit_price", label: "EXIT", align: "right" },
  { key: "lots", label: "LOTS", align: "right" },
  { key: "pnl", label: "P&L", align: "right" },
  { key: "pnl_pips", label: "PIPS", align: "right" },
  { key: "commission", label: "COMM", align: "right" },
  { key: "close_reason", label: "REASON" },
  { key: "duration_time", label: "DURATION" },
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
      <p className="py-8 text-center text-xs uppercase tracking-wider text-muted-foreground">
        No trades to display.
      </p>
    );
  }

  return (
    <div className="space-y-2">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-10 text-[10px] tracking-widest">#</TableHead>
            {COLUMNS.map((col) => (
              <TableHead
                key={col.key}
                className={`cursor-pointer select-none whitespace-nowrap text-[10px] tracking-widest ${
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
              ? "bg-emerald-500/[0.03]"
              : trade.pnl < 0
                ? "bg-red-500/[0.03]"
                : "";
            return (
              <TableRow key={trade.id} className={rowClass}>
                <TableCell className="text-[11px] tabular-nums text-muted-foreground">
                  {page * PAGE_SIZE + idx + 1}
                </TableCell>
                <TableCell className="text-[11px]">
                  <span
                    className={
                      trade.direction === "Long"
                        ? "font-medium text-emerald-400"
                        : "font-medium text-red-400"
                    }
                  >
                    {trade.direction}
                  </span>
                </TableCell>
                <TableCell className="text-[11px] tabular-nums">{trade.entry_time.slice(0, 16)}</TableCell>
                <TableCell className="text-right text-[11px] tabular-nums">{trade.entry_price.toFixed(5)}</TableCell>
                <TableCell className="text-[11px] tabular-nums">{trade.exit_time.slice(0, 16)}</TableCell>
                <TableCell className="text-right text-[11px] tabular-nums">{trade.exit_price.toFixed(5)}</TableCell>
                <TableCell className="text-right text-[11px] tabular-nums">{trade.lots.toFixed(2)}</TableCell>
                <TableCell
                  className={`text-right text-[11px] font-medium tabular-nums ${
                    isWin ? "text-emerald-400" : trade.pnl < 0 ? "text-red-400" : ""
                  }`}
                >
                  {isWin ? "+" : ""}${trade.pnl.toFixed(2)}
                </TableCell>
                <TableCell
                  className={`text-right text-[11px] tabular-nums ${
                    isWin ? "text-emerald-400" : trade.pnl < 0 ? "text-red-400" : ""
                  }`}
                >
                  {trade.pnl_pips.toFixed(1)}
                </TableCell>
                <TableCell className="text-right text-[11px] tabular-nums">${trade.commission.toFixed(2)}</TableCell>
                <TableCell className="text-[11px]">{trade.close_reason}</TableCell>
                <TableCell className="text-[11px] tabular-nums">{trade.duration_time}</TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>

      {/* Pagination */}
      <div className="flex items-center justify-between">
        <span className="text-[10px] uppercase tracking-wider text-muted-foreground">
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
          <span className="text-[11px] tabular-nums text-muted-foreground">
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
