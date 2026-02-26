import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  ResponsiveContainer,
  ComposedChart,
  Area,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ReferenceArea,
  ReferenceLine,
} from "recharts";
import type { EquityPoint } from "@/lib/types";
import { GRID_COLOR, GRID_DASH, AXIS_TICK, AXIS_STROKE, TOOLTIP_STYLE, CHART_COLORS } from "@/lib/chartTheme";

interface EquityMarker {
  date: string;
  label: string;
}

interface EquityCurveProps {
  data: EquityPoint[];
  initialCapital: number;
  markers?: EquityMarker[];
}

interface ChartPoint {
  timestamp: string;
  equity: number;
  newHigh: number | undefined;
}

function extractYear(ts: string): string {
  return ts.slice(0, 4);
}

function HighDot(props: Record<string, unknown>) {
  const { cx, cy, value } = props as { cx?: number; cy?: number; value?: number };
  if (cx == null || cy == null || value == null) return null;
  return <circle cx={cx} cy={cy} r={3.5} fill="hsl(217 90% 60%)" stroke="hsl(217 90% 42%)" strokeWidth={1} />;
}

export function EquityCurve({ data, initialCapital, markers = [] }: EquityCurveProps) {
  const { t } = useTranslation("backtest");
  const [showStagnation, setShowStagnation] = useState(false);
  const [showNewHighs, setShowNewHighs] = useState(false);

  const maxPoints = 1000;
  const step = Math.max(1, Math.floor(data.length / maxPoints));
  const sampled = step > 1 ? data.filter((_, i) => i % step === 0 || i === data.length - 1) : data;

  const { chartData, longestStag, longestStagDays } = useMemo(() => {
    let peak = -Infinity;
    const points: ChartPoint[] = [];
    const ranges: { start: string; end: string; length: number }[] = [];
    let stagnationStartIdx = -1;

    for (let i = 0; i < sampled.length; i++) {
      const p = sampled[i];
      const isNew = p.equity > peak;
      if (isNew) {
        if (stagnationStartIdx >= 0 && i > stagnationStartIdx + 1) {
          ranges.push({
            start: sampled[stagnationStartIdx].timestamp,
            end: sampled[i - 1].timestamp,
            length: i - stagnationStartIdx - 1,
          });
        }
        peak = p.equity;
        stagnationStartIdx = i;
      }
      points.push({
        timestamp: p.timestamp,
        equity: p.equity,
        newHigh: isNew ? p.equity : undefined,
      });
    }
    if (stagnationStartIdx >= 0 && stagnationStartIdx < sampled.length - 1) {
      ranges.push({
        start: sampled[stagnationStartIdx].timestamp,
        end: sampled[sampled.length - 1].timestamp,
        length: sampled.length - 1 - stagnationStartIdx,
      });
    }

    let longest = { start: "", end: "", length: 0 };
    for (const r of ranges) {
      if (r.length > longest.length) longest = r;
    }

    let days = 0;
    if (longest.start && longest.end) {
      const ms = new Date(longest.end).getTime() - new Date(longest.start).getTime();
      days = Math.round(ms / (1000 * 60 * 60 * 24));
    }

    return { chartData: points, longestStag: longest, longestStagDays: days };
  }, [sampled]);

  if (data.length === 0) return null;

  const equities = sampled.map((p) => p.equity);
  const minEquity = Math.min(...equities);
  const maxEquity = Math.max(...equities);

  const range = maxEquity - minEquity;
  const splitOffset = range > 0
    ? Math.max(0, Math.min(1, (maxEquity - initialCapital) / range))
    : 0.5;

  const allAbove = minEquity >= initialCapital;
  const allBelow = maxEquity <= initialCapital;

  const yearTicks = useMemo(() => {
    const seen = new Set<string>();
    const ticks: string[] = [];
    for (const p of chartData) {
      const year = extractYear(p.timestamp);
      if (!seen.has(year)) {
        seen.add(year);
        ticks.push(p.timestamp);
      }
    }
    return ticks;
  }, [chartData]);

  const resolvedMarkers = useMemo(() => {
    if (markers.length === 0 || chartData.length === 0) return [];
    return markers.map((m) => {
      const target = m.date;
      let closest = chartData[0].timestamp;
      for (const p of chartData) {
        if (p.timestamp >= target) {
          closest = p.timestamp;
          break;
        }
        closest = p.timestamp;
      }
      return { timestamp: closest, label: m.label };
    });
  }, [markers, chartData]);

  return (
    <div>
      <div className="mb-2 flex items-center gap-4">
        <label className="flex cursor-pointer items-center gap-1.5 text-sm text-muted-foreground">
          <input
            type="checkbox"
            checked={showStagnation}
            onChange={(e) => setShowStagnation(e.target.checked)}
            className="h-3 w-3 rounded border-border accent-red-500"
          />
          {t("maxStagnation")}
        </label>
        <label className="flex cursor-pointer items-center gap-1.5 text-sm text-muted-foreground">
          <input
            type="checkbox"
            checked={showNewHighs}
            onChange={(e) => setShowNewHighs(e.target.checked)}
            className="h-3 w-3 rounded border-border accent-emerald-500"
          />
          {t("newHighs")}
        </label>
      </div>

      <ResponsiveContainer width="100%" height={400}>
        <ComposedChart data={chartData} margin={{ top: 5, right: 20, bottom: 5, left: 10 }}>
          <defs>
            <linearGradient id="equityStrokeGrad" x1="0" y1="0" x2="0" y2="1">
              {allBelow ? (
                <>
                  <stop offset="0%" stopColor={CHART_COLORS.red} />
                  <stop offset="100%" stopColor={CHART_COLORS.red} />
                </>
              ) : allAbove ? (
                <>
                  <stop offset="0%" stopColor={CHART_COLORS.green} />
                  <stop offset="100%" stopColor={CHART_COLORS.green} />
                </>
              ) : (
                <>
                  <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor={CHART_COLORS.green} />
                  <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor={CHART_COLORS.red} />
                </>
              )}
            </linearGradient>

            <linearGradient id="equityFillGrad" x1="0" y1="0" x2="0" y2="1">
              {allBelow ? (
                <>
                  <stop offset="0%" stopColor={CHART_COLORS.red} stopOpacity={0.25} />
                  <stop offset="100%" stopColor={CHART_COLORS.red} stopOpacity={0.03} />
                </>
              ) : allAbove ? (
                <>
                  <stop offset="0%" stopColor={CHART_COLORS.green} stopOpacity={0.3} />
                  <stop offset="100%" stopColor={CHART_COLORS.green} stopOpacity={0.03} />
                </>
              ) : (
                <>
                  <stop offset="0%" stopColor={CHART_COLORS.green} stopOpacity={0.3} />
                  <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor={CHART_COLORS.green} stopOpacity={0.1} />
                  <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor={CHART_COLORS.red} stopOpacity={0.1} />
                  <stop offset="100%" stopColor={CHART_COLORS.red} stopOpacity={0.03} />
                </>
              )}
            </linearGradient>
          </defs>

          <CartesianGrid strokeDasharray={GRID_DASH} stroke={GRID_COLOR} />
          <XAxis
            dataKey="timestamp"
            ticks={yearTicks}
            tickFormatter={(ts: string) => extractYear(ts)}
            tick={AXIS_TICK}
            stroke={AXIS_STROKE}
          />
          <YAxis
            domain={["auto", "auto"]}
            tickFormatter={(v: number) => `$${v.toLocaleString()}`}
            tick={AXIS_TICK}
            stroke={AXIS_STROKE}
            width={80}
          />
          <Tooltip
            contentStyle={TOOLTIP_STYLE}
            labelFormatter={(label: string) => label}
            formatter={(value: number, name: string) => {
              if (name === "newHigh") return [null, null];
              return [`$${value.toLocaleString(undefined, { maximumFractionDigits: 2 })}`, t("equityCurve")];
            }}
          />

          {showStagnation && longestStag.length > 0 && (
            <ReferenceArea
              x1={longestStag.start}
              x2={longestStag.end}
              fill="hsl(0 50% 25%)"
              fillOpacity={0.35}
              stroke="none"
            />
          )}

          {!allAbove && !allBelow && (
            <ReferenceLine
              y={initialCapital}
              stroke="hsl(0 0% 25%)"
              strokeDasharray="4 4"
              strokeWidth={1}
            />
          )}

          <Area
            type="monotone"
            dataKey="equity"
            stroke="url(#equityStrokeGrad)"
            fill="url(#equityFillGrad)"
            strokeWidth={1.5}
            dot={false}
            animationDuration={500}
            isAnimationActive={true}
          />

          {resolvedMarkers.map((m, i) => (
            <ReferenceLine
              key={`marker-${i}`}
              x={m.timestamp}
              stroke={CHART_COLORS.amber}
              strokeDasharray="6 3"
              strokeWidth={1.5}
              label={{
                value: m.label,
                position: "top",
                fill: CHART_COLORS.amber,
                fontSize: 12,
                fontWeight: 600,
              }}
            />
          ))}

          {showNewHighs && (
            <Line
              type="monotone"
              dataKey="newHigh"
              stroke="none"
              dot={<HighDot />}
              activeDot={false}
              isAnimationActive={false}
              connectNulls={false}
            />
          )}
        </ComposedChart>
      </ResponsiveContainer>

      {showStagnation && longestStag.length > 0 && (
        <p className="mt-1 text-center text-sm text-red-400/70">
          {t("maxStagnationInfo", { days: longestStagDays, start: longestStag.start.slice(0, 10), end: longestStag.end.slice(0, 10) })}
        </p>
      )}
    </div>
  );
}
