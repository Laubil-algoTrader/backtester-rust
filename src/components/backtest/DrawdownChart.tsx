import { useTranslation } from "react-i18next";
import {
  ResponsiveContainer,
  AreaChart,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
} from "recharts";
import type { DrawdownPoint } from "@/lib/types";
import { GRID_COLOR, GRID_DASH, AXIS_TICK, AXIS_STROKE, TOOLTIP_STYLE, CHART_COLORS } from "@/lib/chartTheme";

interface DrawdownChartProps {
  data: DrawdownPoint[];
}

function formatTimestamp(ts: string): string {
  return ts.slice(0, 10);
}

export function DrawdownChart({ data }: DrawdownChartProps) {
  const { t } = useTranslation("backtest");

  if (data.length === 0) return null;

  const maxPoints = 1000;
  const step = Math.max(1, Math.floor(data.length / maxPoints));
  const sampled = (step > 1 ? data.filter((_, i) => i % step === 0 || i === data.length - 1) : data)
    .map((p) => ({ ...p, drawdown_pct: -p.drawdown_pct }));

  return (
    <ResponsiveContainer width="100%" height={160}>
      <AreaChart data={sampled} margin={{ top: 5, right: 20, bottom: 5, left: 10 }}>
        <CartesianGrid strokeDasharray={GRID_DASH} stroke={GRID_COLOR} />
        <XAxis
          dataKey="timestamp"
          tickFormatter={formatTimestamp}
          tick={AXIS_TICK}
          stroke={AXIS_STROKE}
          interval="preserveStartEnd"
        />
        <YAxis
          tickFormatter={(v: number) => `${v.toFixed(1)}%`}
          tick={AXIS_TICK}
          stroke={AXIS_STROKE}
          width={60}
        />
        <Tooltip
          contentStyle={TOOLTIP_STYLE}
          labelFormatter={(label: string) => label}
          formatter={(value: number) => [`${value.toFixed(2)}%`, t("drawdown")]}
        />
        <Area
          type="monotone"
          dataKey="drawdown_pct"
          stroke={CHART_COLORS.red}
          fill={CHART_COLORS.red}
          fillOpacity={0.12}
          strokeWidth={1.5}
          animationDuration={500}
        />
      </AreaChart>
    </ResponsiveContainer>
  );
}
