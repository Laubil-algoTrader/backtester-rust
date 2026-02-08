import { useMemo } from "react";
import {
  ResponsiveContainer,
  ScatterChart,
  Scatter,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Cell,
} from "recharts";
import type { OptimizationResult, ParameterRange } from "@/lib/types";

interface HeatmapChartProps {
  results: OptimizationResult[];
  parameterRanges: ParameterRange[];
}

function interpolateColor(value: number, min: number, max: number): string {
  if (max === min) return "hsl(142 71% 45%)";
  const t = (value - min) / (max - min);
  // Red (0) → Yellow (0.5) → Green (1)
  if (t < 0.5) {
    const r = 220;
    const g = Math.round(50 + t * 2 * 170);
    const b = 50;
    return `rgb(${r},${g},${b})`;
  } else {
    const r = Math.round(220 - (t - 0.5) * 2 * 180);
    const g = 200;
    const b = Math.round(50 + (t - 0.5) * 2 * 50);
    return `rgb(${r},${g},${b})`;
  }
}

export function HeatmapChart({ results, parameterRanges }: HeatmapChartProps) {
  if (parameterRanges.length !== 2 || results.length === 0) return null;

  const xName = parameterRanges[0].display_name;
  const yName = parameterRanges[1].display_name;

  const data = useMemo(() => {
    return results.map((r) => ({
      x: r.params[xName] ?? 0,
      y: r.params[yName] ?? 0,
      objective: r.objective_value,
    }));
  }, [results, xName, yName]);

  const minObj = Math.min(...data.map((d) => d.objective));
  const maxObj = Math.max(...data.map((d) => d.objective));

  return (
    <div>
      <h3 className="mb-2 text-sm font-medium">
        Heatmap: {xName} vs {yName}
      </h3>
      <ResponsiveContainer width="100%" height={300}>
        <ScatterChart margin={{ top: 10, right: 20, bottom: 30, left: 20 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="hsl(var(--border))" />
          <XAxis
            type="number"
            dataKey="x"
            name={xName}
            tick={{ fontSize: 11, fill: "hsl(var(--muted-foreground))" }}
            label={{
              value: xName.length > 30 ? xName.slice(0, 30) + "..." : xName,
              position: "bottom",
              offset: 10,
              style: { fontSize: 11, fill: "hsl(var(--muted-foreground))" },
            }}
          />
          <YAxis
            type="number"
            dataKey="y"
            name={yName}
            tick={{ fontSize: 11, fill: "hsl(var(--muted-foreground))" }}
            label={{
              value: yName.length > 30 ? yName.slice(0, 30) + "..." : yName,
              angle: -90,
              position: "insideLeft",
              offset: -5,
              style: { fontSize: 11, fill: "hsl(var(--muted-foreground))" },
            }}
            width={60}
          />
          <Tooltip
            contentStyle={{
              backgroundColor: "hsl(var(--card))",
              border: "1px solid hsl(var(--border))",
              borderRadius: 6,
              fontSize: 12,
            }}
            formatter={(value: number, name: string) => [
              value.toFixed(2),
              name === "objective" ? "Objective" : name,
            ]}
            labelFormatter={() => ""}
          />
          <Scatter data={data} animationDuration={300}>
            {data.map((entry, i) => (
              <Cell
                key={i}
                fill={interpolateColor(entry.objective, minObj, maxObj)}
                r={8}
              />
            ))}
          </Scatter>
        </ScatterChart>
      </ResponsiveContainer>
    </div>
  );
}
