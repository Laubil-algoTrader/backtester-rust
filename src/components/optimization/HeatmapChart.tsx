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
  if (max === min) return "hsl(152 60% 42%)";
  const t = (value - min) / (max - min);
  // Red (0) → Yellow (0.5) → Green (1)
  if (t < 0.5) {
    const r = 200;
    const g = Math.round(50 + t * 2 * 150);
    const b = 50;
    return `rgb(${r},${g},${b})`;
  } else {
    const r = Math.round(200 - (t - 0.5) * 2 * 160);
    const g = 180;
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
      <h3 className="mb-2 text-[10px] font-semibold uppercase tracking-[0.15em] text-muted-foreground">
        Heatmap: {xName} vs {yName}
      </h3>
      <ResponsiveContainer width="100%" height={300}>
        <ScatterChart margin={{ top: 10, right: 20, bottom: 30, left: 20 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="hsl(220 10% 14%)" />
          <XAxis
            type="number"
            dataKey="x"
            name={xName}
            tick={{ fontSize: 10, fill: "hsl(45 5% 40%)" }}
            stroke="hsl(220 10% 14%)"
            label={{
              value: xName.length > 30 ? xName.slice(0, 30) + "..." : xName,
              position: "bottom",
              offset: 10,
              style: { fontSize: 10, fill: "hsl(45 5% 40%)" },
            }}
          />
          <YAxis
            type="number"
            dataKey="y"
            name={yName}
            tick={{ fontSize: 10, fill: "hsl(45 5% 40%)" }}
            stroke="hsl(220 10% 14%)"
            label={{
              value: yName.length > 30 ? yName.slice(0, 30) + "..." : yName,
              angle: -90,
              position: "insideLeft",
              offset: -5,
              style: { fontSize: 10, fill: "hsl(45 5% 40%)" },
            }}
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
