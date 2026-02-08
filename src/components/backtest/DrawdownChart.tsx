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
    <ResponsiveContainer width="100%" height={200}>
      <AreaChart data={sampled} margin={{ top: 5, right: 20, bottom: 5, left: 10 }}>
        <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" />
        <XAxis
          dataKey="timestamp"
          tickFormatter={formatTimestamp}
          tick={{ fontSize: 11, fill: "hsl(var(--muted-foreground))" }}
          interval="preserveStartEnd"
        />
        <YAxis
          tickFormatter={(v: number) => `${v.toFixed(1)}%`}
          tick={{ fontSize: 11, fill: "hsl(var(--muted-foreground))" }}
          width={60}
        />
        <Tooltip
          contentStyle={{
            backgroundColor: "hsl(var(--card))",
            border: "1px solid hsl(var(--border))",
            borderRadius: 6,
            fontSize: 12,
          }}
          labelFormatter={(label: string) => label}
          formatter={(value: number) => [`${value.toFixed(2)}%`, "Drawdown"]}
        />
        <Area
          type="monotone"
          dataKey="drawdown_pct"
          stroke="hsl(0 84% 60%)"
          fill="hsl(0 84% 60%)"
          fillOpacity={0.2}
          strokeWidth={1.5}
          animationDuration={500}
        />
      </AreaChart>
    </ResponsiveContainer>
  );
}
