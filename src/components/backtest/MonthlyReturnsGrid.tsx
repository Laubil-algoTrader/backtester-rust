import { useMemo } from "react";
import { cn } from "@/lib/utils";
import type { TradeResult } from "@/lib/types";

interface MonthlyReturnsGridProps {
  trades: TradeResult[];
  initialCapital: number;
}

const MONTHS = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];

function cellColor(pct: number): string {
  if (pct === 0) return "";
  const abs = Math.min(Math.abs(pct) / 5, 1);
  const opacity = (abs * 0.55 + 0.1).toFixed(2);
  return pct > 0
    ? `rgba(34,197,94,${opacity})`
    : `rgba(239,68,68,${opacity})`;
}

export function MonthlyReturnsGrid({ trades, initialCapital }: MonthlyReturnsGridProps) {
  const data = useMemo(() => {
    if (trades.length === 0) return [];

    // Group PnL by YYYY-MM
    const monthly: Record<string, number> = {};
    for (const t of trades) {
      const key = t.exit_time.slice(0, 7); // "YYYY-MM"
      monthly[key] = (monthly[key] ?? 0) + t.pnl;
    }

    // Build sorted list of months
    const keys = Object.keys(monthly).sort();
    if (keys.length === 0) return [];

    // Compute running capital & return pct per month
    const byYear: Record<number, (number | null)[]> = {};
    const byYearPnl: Record<number, number> = {};
    const yearStartCapital: Record<number, number> = {};

    let capital = initialCapital;
    for (const key of keys) {
      const pnl = monthly[key];
      const pct = capital > 0 ? (pnl / capital) * 100 : 0;
      const [yr, mo] = key.split("-").map(Number);
      if (!byYear[yr]) {
        byYear[yr] = Array(12).fill(null);
        yearStartCapital[yr] = capital;
        byYearPnl[yr] = 0;
      }
      byYear[yr][mo - 1] = pct;
      byYearPnl[yr] += pnl;
      capital += pnl;
    }

    return Object.entries(byYear)
      .sort(([a], [b]) => Number(a) - Number(b))
      .map(([year, months]) => {
        const yr = Number(year);
        const startCap = yearStartCapital[yr] ?? initialCapital;
        const ytd = startCap > 0 ? (byYearPnl[yr] / startCap) * 100 : 0;
        return { year: yr, months, ytd };
      });
  }, [trades, initialCapital]);

  if (data.length === 0) return null;

  return (
    <div className="rounded border border-border/30 bg-muted/5 p-3">
      <p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
        Monthly Returns
      </p>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-[10px]">
          <thead>
            <tr>
              <th className="w-12 py-1 pr-2 text-left text-[9px] font-medium uppercase tracking-wider text-muted-foreground/50">
                Year
              </th>
              {MONTHS.map((m) => (
                <th
                  key={m}
                  className="px-1 py-1 text-center text-[9px] font-medium uppercase tracking-wider text-muted-foreground/50"
                >
                  {m}
                </th>
              ))}
              <th className="px-2 py-1 text-right text-[9px] font-medium uppercase tracking-wider text-muted-foreground/50">
                YTD
              </th>
            </tr>
          </thead>
          <tbody>
            {data.map(({ year, months, ytd }) => (
              <tr key={year} className="border-t border-border/10">
                <td className="py-1 pr-2 text-left font-medium text-foreground/70">
                  {year}
                </td>
                {months.map((pct, i) => (
                  <td
                    key={i}
                    className="px-0.5 py-0.5 text-center tabular-nums"
                    style={{ backgroundColor: pct !== null ? cellColor(pct) : undefined }}
                  >
                    {pct !== null ? (
                      <span className={cn(
                        "font-medium",
                        pct > 0 ? "text-emerald-400" : pct < 0 ? "text-red-400" : "text-muted-foreground/60"
                      )}>
                        {pct >= 0 ? "+" : ""}{pct.toFixed(1)}%
                      </span>
                    ) : (
                      <span className="text-muted-foreground/20">—</span>
                    )}
                  </td>
                ))}
                <td
                  className="px-2 py-0.5 text-right tabular-nums font-bold"
                  style={{ backgroundColor: cellColor(ytd) }}
                >
                  <span className={cn(
                    ytd > 0 ? "text-emerald-400" : ytd < 0 ? "text-red-400" : "text-muted-foreground/60"
                  )}>
                    {ytd >= 0 ? "+" : ""}{ytd.toFixed(1)}%
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
