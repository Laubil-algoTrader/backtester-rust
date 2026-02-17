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

interface DrawdownChartProps {
  data: DrawdownPoint[];
}

function formatTimestamp(ts: string): string {
  return ts.slice(0, 10);
}

export function DrawdownChart({ data }: DrawdownChartProps) {
  if (data.length === 0) return null;

  // Negate values so drawdown shows below zero
  const maxPoints = 1000;
  const step = Math.max(1, Math.floor(data.length / maxPoints));
  const sampled = (step > 1 ? data.filter((_, i) => i % step === 0 || i === data.length - 1) : data)
    .map((p) => ({ ...p, drawdown_pct: -p.drawdown_pct }));

  return (
    <ResponsiveContainer width="100%" height={160}>
      <AreaChart data={sampled} margin={{ top: 5, right: 20, bottom: 5, left: 10 }}>
        <CartesianGrid strokeDasharray="3 3" stroke="hsl(220 10% 14%)" />
        <XAxis
          dataKey="timestamp"
          tickFormatter={formatTimestamp}
          tick={{ fontSize: 10, fill: "hsl(45 5% 40%)" }}
          stroke="hsl(220 10% 14%)"
          interval="preserveStartEnd"
        />
        <YAxis
          tickFormatter={(v: number) => `${v.toFixed(1)}%`}
          tick={{ fontSize: 10, fill: "hsl(45 5% 40%)" }}
          stroke="hsl(220 10% 14%)"
          width={60}
        />
        <Tooltip
          contentStyle={{
            backgroundColor: "hsl(220 15% 7%)",
            border: "1px solid hsl(43 20% 18%)",
            borderRadius: 4,
            fontSize: 11,
            color: "hsl(45 10% 85%)",
          }}
          labelFormatter={(label: string) => label}
          formatter={(value: number) => [`${value.toFixed(2)}%`, "Drawdown"]}
        />
        <Area
          type="monotone"
          dataKey="drawdown_pct"
          stroke="hsl(0 72% 50%)"
          fill="hsl(0 72% 50%)"
          fillOpacity={0.15}
          strokeWidth={1.5}
          animationDuration={500}
        />
      </AreaChart>
    </ResponsiveContainer>
  );
}
