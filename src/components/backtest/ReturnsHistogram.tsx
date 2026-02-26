import { useTranslation } from "react-i18next";
import {
  ResponsiveContainer,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Cell,
} from "recharts";
import { GRID_COLOR, GRID_DASH, AXIS_TICK, AXIS_STROKE, TOOLTIP_STYLE, CHART_COLORS } from "@/lib/chartTheme";

interface HistogramBin {
  label: string;
  count: number;
  midpoint: number;
}

function createHistogramBins(
  returns: number[],
  binCount: number
): HistogramBin[] {
  if (returns.length === 0) return [];

  const min = Math.min(...returns);
  const max = Math.max(...returns);

  if (min === max) {
    return [{ label: `$${min.toFixed(0)}`, count: returns.length, midpoint: min }];
  }

  const binWidth = (max - min) / binCount;
  const bins: HistogramBin[] = [];

  for (let i = 0; i < binCount; i++) {
    const lo = min + i * binWidth;
    const hi = lo + binWidth;
    const mid = (lo + hi) / 2;
    bins.push({
      label: `$${lo.toFixed(0)}`,
      count: 0,
      midpoint: mid,
    });
  }

  for (const r of returns) {
    let idx = Math.floor((r - min) / binWidth);
    if (idx >= binCount) idx = binCount - 1;
    bins[idx].count++;
  }

  return bins;
}

interface ReturnsHistogramProps {
  returns: number[];
}

export function ReturnsHistogram({ returns }: ReturnsHistogramProps) {
  const { t } = useTranslation("backtest");

  if (returns.length === 0) return null;

  const bins = createHistogramBins(returns, Math.min(25, Math.max(5, Math.round(returns.length / 3))));

  return (
    <ResponsiveContainer width="100%" height={250}>
      <BarChart data={bins} margin={{ top: 5, right: 20, bottom: 5, left: 10 }}>
        <CartesianGrid strokeDasharray={GRID_DASH} stroke={GRID_COLOR} />
        <XAxis
          dataKey="label"
          tick={AXIS_TICK}
          stroke={AXIS_STROKE}
          interval="preserveStartEnd"
        />
        <YAxis
          tick={AXIS_TICK}
          stroke={AXIS_STROKE}
          allowDecimals={false}
          width={40}
        />
        <Tooltip
          contentStyle={TOOLTIP_STYLE}
          formatter={(value: number) => [value, t("trades")]}
          labelFormatter={(label: string) => `P&L: ${label}`}
        />
        <Bar dataKey="count" animationDuration={500}>
          {bins.map((bin, i) => (
            <Cell
              key={i}
              fill={bin.midpoint >= 0 ? CHART_COLORS.green : CHART_COLORS.red}
              fillOpacity={0.85}
            />
          ))}
        </Bar>
      </BarChart>
    </ResponsiveContainer>
  );
}
