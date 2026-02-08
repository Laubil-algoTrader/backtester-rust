import {
  ResponsiveContainer,
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
} from "recharts";
import type { EquityPoint } from "@/lib/types";

interface EquityCurveProps {
  data: EquityPoint[];
}

function formatTimestamp(ts: string): string {
  // Show date portion only for tick labels
  return ts.slice(0, 10);
}

export function EquityCurve({ data }: EquityCurveProps) {
  if (data.length === 0) return null;

  // Downsample for performance if too many points
  const maxPoints = 1000;
  const step = Math.max(1, Math.floor(data.length / maxPoints));
  const sampled = step > 1 ? data.filter((_, i) => i % step === 0 || i === data.length - 1) : data;

  return (
    <ResponsiveContainer width="100%" height={300}>
      <LineChart data={sampled} margin={{ top: 5, right: 20, bottom: 5, left: 10 }}>
        <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" />
        <XAxis
          dataKey="timestamp"
          tickFormatter={formatTimestamp}
          tick={{ fontSize: 11, fill: "hsl(var(--muted-foreground))" }}
          interval="preserveStartEnd"
        />
        <YAxis
          tickFormatter={(v: number) => `$${v.toLocaleString()}`}
          tick={{ fontSize: 11, fill: "hsl(var(--muted-foreground))" }}
          width={80}
        />
        <Tooltip
          contentStyle={{
            backgroundColor: "hsl(var(--card))",
            border: "1px solid hsl(var(--border))",
            borderRadius: 6,
            fontSize: 12,
          }}
          labelFormatter={(label: string) => label}
          formatter={(value: number) => [`$${value.toLocaleString(undefined, { maximumFractionDigits: 2 })}`, "Equity"]}
        />
        <Line
          type="monotone"
          dataKey="equity"
          stroke="hsl(var(--primary))"
          strokeWidth={1.5}
          dot={false}
          animationDuration={500}
        />
      </LineChart>
    </ResponsiveContainer>
  );
}
