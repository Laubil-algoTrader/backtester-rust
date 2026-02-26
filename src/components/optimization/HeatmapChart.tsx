import { useMemo } from "react";
import { useTranslation } from "react-i18next";
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
import { GRID_COLOR, GRID_DASH, AXIS_TICK, AXIS_STROKE, TOOLTIP_STYLE } from "@/lib/chartTheme";

interface HeatmapChartProps {
  results: OptimizationResult[];
  parameterRanges: ParameterRange[];
}

function interpolateColor(value: number, min: number, max: number): string {
  if (max === min) return "hsl(152 60% 42%)";
  const t = (value - min) / (max - min);
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
  const { t } = useTranslation("optimization");
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
      <h3 className="mb-2 text-sm font-semibold text-foreground/70">
        {t("heatmap", { x: xName, y: yName })}
      </h3>
      <ResponsiveContainer width="100%" height={300}>
        <ScatterChart margin={{ top: 10, right: 20, bottom: 30, left: 20 }}>
          <CartesianGrid strokeDasharray={GRID_DASH} stroke={GRID_COLOR} />
          <XAxis
            type="number"
            dataKey="x"
            name={xName}
            tick={AXIS_TICK}
            stroke={AXIS_STROKE}
            label={{
              value: xName.length > 30 ? xName.slice(0, 30) + "..." : xName,
              position: "bottom",
              offset: 10,
              style: { fontSize: 12, fill: "hsl(0 0% 38%)" },
            }}
          />
          <YAxis
            type="number"
            dataKey="y"
            name={yName}
            tick={AXIS_TICK}
            stroke={AXIS_STROKE}
            label={{
              value: yName.length > 30 ? yName.slice(0, 30) + "..." : yName,
              angle: -90,
              position: "insideLeft",
              offset: -5,
              style: { fontSize: 12, fill: "hsl(0 0% 38%)" },
            }}
            width={60}
          />
          <Tooltip
            contentStyle={TOOLTIP_STYLE}
            formatter={(value: number, name: string) => [
              value.toFixed(2),
              name === "objective" ? t("objective") : name,
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
