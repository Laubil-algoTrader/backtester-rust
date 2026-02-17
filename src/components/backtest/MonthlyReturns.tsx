import { useMemo } from "react";
import type { EquityPoint } from "@/lib/types";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

interface MonthlyRow {
  year: number;
  months: (number | null)[]; // 12 entries, null = no data
  ytd: number;
}

function computeMonthlyReturns(equityCurve: EquityPoint[]): MonthlyRow[] {
  if (equityCurve.length < 2) return [];

  // Group equity by year-month, taking first and last equity per month
  const monthlyEquity = new Map<string, { first: number; last: number }>();

  for (const point of equityCurve) {
    const key = point.timestamp.slice(0, 7); // "YYYY-MM"
    const existing = monthlyEquity.get(key);
    if (!existing) {
      monthlyEquity.set(key, { first: point.equity, last: point.equity });
    } else {
      existing.last = point.equity;
    }
  }

  // Build sorted list of months
  const sortedKeys = [...monthlyEquity.keys()].sort();
  if (sortedKeys.length === 0) return [];

  // Calculate return for each month: use previous month's last equity as base
  const monthReturns = new Map<string, number>();
  let prevEquity = monthlyEquity.get(sortedKeys[0])!.first;

  for (const key of sortedKeys) {
    const { last } = monthlyEquity.get(key)!;
    const ret = prevEquity !== 0 ? ((last - prevEquity) / prevEquity) * 100 : 0;
    monthReturns.set(key, ret);
    prevEquity = last;
  }

  // Group by year
  const years = new Map<number, (number | null)[]>();
  for (const key of sortedKeys) {
    const year = parseInt(key.slice(0, 4));
    const month = parseInt(key.slice(5, 7)) - 1; // 0-indexed
    if (!years.has(year)) {
      years.set(year, Array(12).fill(null));
    }
    years.get(year)![month] = monthReturns.get(key)!;
  }

  // Calculate YTD for each year using compound returns
  const rows: MonthlyRow[] = [];
  for (const [year, months] of [...years.entries()].sort((a, b) => a[0] - b[0])) {
    let compound = 1;
    for (const m of months) {
      if (m !== null) {
        compound *= 1 + m / 100;
      }
    }
    const ytd = (compound - 1) * 100;
    rows.push({ year, months, ytd });
  }

  return rows;
}

function formatCell(value: number | null): string {
  if (value === null) return "";
  return value.toFixed(2);
}

function cellColor(value: number | null): string {
  if (value === null) return "";
  if (value > 0) return "text-emerald-400";
  if (value < 0) return "text-red-400";
  return "text-muted-foreground";
}

interface MonthlyReturnsProps {
  equityCurve: EquityPoint[];
}

export function MonthlyReturns({ equityCurve }: MonthlyReturnsProps) {
  const rows = useMemo(() => computeMonthlyReturns(equityCurve), [equityCurve]);

  if (rows.length === 0) {
    return (
      <p className="py-4 text-center text-xs uppercase tracking-wider text-muted-foreground">
        Not enough data for monthly breakdown.
      </p>
    );
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse text-[11px] tabular-nums">
        <thead>
          <tr className="border-b border-border/60">
            <th className="px-2 py-2 text-left text-[10px] font-medium tracking-widest text-muted-foreground">
              YEAR
            </th>
            {MONTHS.map((m) => (
              <th
                key={m}
                className="px-2 py-2 text-right text-[10px] font-medium tracking-widest text-muted-foreground"
              >
                {m.toUpperCase()}
              </th>
            ))}
            <th className="px-2 py-2 text-right text-[10px] font-medium tracking-widest text-muted-foreground">
              YTD
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.year} className="border-b border-border/30 hover:bg-card/50">
              <td className="px-2 py-1.5 font-medium text-foreground">
                {row.year}
              </td>
              {row.months.map((val, i) => (
                <td
                  key={i}
                  className={`px-2 py-1.5 text-right ${cellColor(val)}`}
                >
                  {formatCell(val)}
                </td>
              ))}
              <td
                className={`px-2 py-1.5 text-right font-medium ${cellColor(row.ytd)}`}
              >
                {row.ytd.toFixed(2)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
