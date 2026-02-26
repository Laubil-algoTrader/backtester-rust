import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { EquityPoint } from "@/lib/types";

const MONTH_KEYS = ["months.jan", "months.feb", "months.mar", "months.apr", "months.may", "months.jun", "months.jul", "months.aug", "months.sep", "months.oct", "months.nov", "months.dec"];

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

/** Heatmap background for monthly/YTD cells — mirrors Bloomberg Historical Yields style.
 *  Uses the full range of values to normalize colors. */
function cellBg(value: number | null, absMax: number): string | undefined {
  if (value === null || absMax === 0) return undefined;
  const t = Math.max(-1, Math.min(1, value / absMax));
  if (t > 0) {
    // Green zone: positive returns
    const opacity = 0.08 + t * 0.25;
    return `rgba(16, 185, 129, ${opacity.toFixed(3)})`;
  } else if (t < 0) {
    // Red zone: negative returns
    const opacity = 0.08 + Math.abs(t) * 0.25;
    return `rgba(220, 38, 38, ${opacity.toFixed(3)})`;
  }
  return undefined;
}

interface MonthlyReturnsProps {
  equityCurve: EquityPoint[];
}

export function MonthlyReturns({ equityCurve }: MonthlyReturnsProps) {
  const { t } = useTranslation("common");
  const rows = useMemo(() => computeMonthlyReturns(equityCurve), [equityCurve]);

  // Compute absolute max for heatmap normalization
  const absMax = useMemo(() => {
    let max = 0;
    for (const row of rows) {
      for (const m of row.months) {
        if (m !== null && Math.abs(m) > max) max = Math.abs(m);
      }
      if (Math.abs(row.ytd) > max) max = Math.abs(row.ytd);
    }
    return max;
  }, [rows]);

  if (rows.length === 0) {
    return (
      <p className="py-4 text-center text-sm text-muted-foreground">
        {t("noData")}
      </p>
    );
  }

  return (
    <div className="overflow-x-auto">
      <table className="w-full border-collapse font-mono text-sm tabular-nums">
        <thead>
          <tr className="border-b border-border/40">
            <th className="px-2 py-2 text-left text-xs font-medium text-muted-foreground">
              {t("year")}
            </th>
            {MONTH_KEYS.map((key) => (
              <th
                key={key}
                className="px-2 py-2 text-right text-xs font-medium text-muted-foreground"
              >
                {t(key)}
              </th>
            ))}
            <th className="border-l border-border/30 px-2 py-2 text-right text-xs font-medium text-muted-foreground">
              {t("ytd")}
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.year} className="border-b border-border/20">
              <td className="px-2 py-1.5 font-semibold text-foreground">
                {row.year}
              </td>
              {row.months.map((val, i) => (
                <td
                  key={i}
                  className={`px-2 py-1.5 text-right ${cellColor(val)}`}
                  style={cellBg(val, absMax) ? { backgroundColor: cellBg(val, absMax) } : undefined}
                >
                  {formatCell(val)}
                </td>
              ))}
              <td
                className={`border-l border-border/30 px-2 py-1.5 text-right font-semibold ${cellColor(row.ytd)}`}
                style={cellBg(row.ytd, absMax) ? { backgroundColor: cellBg(row.ytd, absMax) } : undefined}
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
