import { useState, useMemo } from "react";
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
  /** Equity value at new-high points, undefined otherwise (so Recharts skips the dot) */
  newHigh: number | undefined;
}

function extractYear(ts: string): string {
  return ts.slice(0, 4);
}

/** Custom green dot for new-high markers */
function GreenDot(props: Record<string, unknown>) {
  const { cx, cy, value } = props as { cx?: number; cy?: number; value?: number };
  if (cx == null || cy == null || value == null) return null;
  return <circle cx={cx} cy={cy} r={3.5} fill="hsl(142 70% 50%)" stroke="hsl(142 70% 35%)" strokeWidth={1} />;
}

export function EquityCurve({ data, initialCapital, markers = [] }: EquityCurveProps) {
  const [showStagnation, setShowStagnation] = useState(false);
  const [showNewHighs, setShowNewHighs] = useState(false);

  // Downsample for performance
  const maxPoints = 1000;
  const step = Math.max(1, Math.floor(data.length / maxPoints));
  const sampled = step > 1 ? data.filter((_, i) => i % step === 0 || i === data.length - 1) : data;

  // Enrich data: detect new highs and stagnation periods
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
    // Close final stagnation if still open
    if (stagnationStartIdx >= 0 && stagnationStartIdx < sampled.length - 1) {
      ranges.push({
        start: sampled[stagnationStartIdx].timestamp,
        end: sampled[sampled.length - 1].timestamp,
        length: sampled.length - 1 - stagnationStartIdx,
      });
    }

    // Find longest stagnation range
    let longest = { start: "", end: "", length: 0 };
    for (const r of ranges) {
      if (r.length > longest.length) longest = r;
    }

    // Compute days for longest stagnation
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

  // Compute gradient split: offset where initialCapital sits (0=top, 1=bottom)
  const range = maxEquity - minEquity;
  const splitOffset = range > 0
    ? Math.max(0, Math.min(1, (maxEquity - initialCapital) / range))
    : 0.5;

  // All equity below initial capital → full red; all above → full blue
  const allAbove = minEquity >= initialCapital;
  const allBelow = maxEquity <= initialCapital;

  // Year-based ticks
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

  // Resolve markers to nearest chart timestamps
  const resolvedMarkers = useMemo(() => {
    if (markers.length === 0 || chartData.length === 0) return [];
    return markers.map((m) => {
      // Find first chart point whose timestamp starts with (or is >=) the marker date
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
      {/* Toggles */}
      <div className="mb-2 flex items-center gap-4">
        <label className="flex cursor-pointer items-center gap-1.5 text-[10px] uppercase tracking-wider text-muted-foreground">
          <input
            type="checkbox"
            checked={showStagnation}
            onChange={(e) => setShowStagnation(e.target.checked)}
            className="h-3 w-3 rounded border-border accent-red-500"
          />
          Max Stagnation
        </label>
        <label className="flex cursor-pointer items-center gap-1.5 text-[10px] uppercase tracking-wider text-muted-foreground">
          <input
            type="checkbox"
            checked={showNewHighs}
            onChange={(e) => setShowNewHighs(e.target.checked)}
            className="h-3 w-3 rounded border-border accent-emerald-500"
          />
          New Highs
        </label>
      </div>

      <ResponsiveContainer width="100%" height={400}>
        <ComposedChart data={chartData} margin={{ top: 5, right: 20, bottom: 5, left: 10 }}>
          <defs>
            {/* Stroke gradient: blue above initial capital, red below */}
            <linearGradient id="equityStrokeGrad" x1="0" y1="0" x2="0" y2="1">
              {allBelow ? (
                <>
                  <stop offset="0%" stopColor="hsl(0 70% 55%)" />
                  <stop offset="100%" stopColor="hsl(0 70% 55%)" />
                </>
              ) : allAbove ? (
                <>
                  <stop offset="0%" stopColor="hsl(210 80% 55%)" />
                  <stop offset="100%" stopColor="hsl(210 80% 55%)" />
                </>
              ) : (
                <>
                  <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor="hsl(210 80% 55%)" />
                  <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor="hsl(0 70% 55%)" />
                </>
              )}
            </linearGradient>

            {/* Fill gradient: blue tint above, red tint below */}
            <linearGradient id="equityFillGrad" x1="0" y1="0" x2="0" y2="1">
              {allBelow ? (
                <>
                  <stop offset="0%" stopColor="hsl(0 70% 55%)" stopOpacity={0.25} />
                  <stop offset="100%" stopColor="hsl(0 70% 55%)" stopOpacity={0.03} />
                </>
              ) : allAbove ? (
                <>
                  <stop offset="0%" stopColor="hsl(210 80% 55%)" stopOpacity={0.25} />
                  <stop offset="100%" stopColor="hsl(210 80% 55%)" stopOpacity={0.03} />
                </>
              ) : (
                <>
                  <stop offset="0%" stopColor="hsl(210 80% 55%)" stopOpacity={0.25} />
                  <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor="hsl(210 80% 55%)" stopOpacity={0.1} />
                  <stop offset={`${(splitOffset * 100).toFixed(1)}%`} stopColor="hsl(0 70% 55%)" stopOpacity={0.1} />
                  <stop offset="100%" stopColor="hsl(0 70% 55%)" stopOpacity={0.03} />
                </>
              )}
            </linearGradient>
          </defs>

          <CartesianGrid strokeDasharray="3 3" stroke="hsl(220 10% 14%)" />
          <XAxis
            dataKey="timestamp"
            ticks={yearTicks}
            tickFormatter={(ts: string) => extractYear(ts)}
            tick={{ fontSize: 10, fill: "hsl(45 5% 40%)" }}
            stroke="hsl(220 10% 14%)"
          />
          <YAxis
            domain={["auto", "auto"]}
            tickFormatter={(v: number) => `$${v.toLocaleString()}`}
            tick={{ fontSize: 10, fill: "hsl(45 5% 40%)" }}
            stroke="hsl(220 10% 14%)"
            width={80}
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
            formatter={(value: number, name: string) => {
              if (name === "newHigh") return [null, null];
              return [`$${value.toLocaleString(undefined, { maximumFractionDigits: 2 })}`, "Equity"];
            }}
          />

          {/* Max stagnation band — dark red */}
          {showStagnation && longestStag.length > 0 && (
            <ReferenceArea
              x1={longestStag.start}
              x2={longestStag.end}
              fill="hsl(0 50% 25%)"
              fillOpacity={0.35}
              stroke="none"
            />
          )}

          {/* Initial capital reference line */}
          {!allAbove && !allBelow && (
            <ReferenceLine
              y={initialCapital}
              stroke="hsl(45 5% 30%)"
              strokeDasharray="4 4"
              strokeWidth={1}
            />
          )}

          {/* Equity area with conditional coloring */}
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

          {/* IS/OOS boundary markers */}
          {resolvedMarkers.map((m, i) => (
            <ReferenceLine
              key={`marker-${i}`}
              x={m.timestamp}
              stroke="hsl(38 80% 55%)"
              strokeDasharray="6 3"
              strokeWidth={1.5}
              label={{
                value: m.label,
                position: "top",
                fill: "hsl(38 80% 55%)",
                fontSize: 10,
                fontWeight: 600,
              }}
            />
          ))}

          {/* Green dots at new equity highs — rendered as a Line with dots, no connecting stroke */}
          {showNewHighs && (
            <Line
              type="monotone"
              dataKey="newHigh"
              stroke="none"
              dot={<GreenDot />}
              activeDot={false}
              isAnimationActive={false}
              connectNulls={false}
            />
          )}
        </ComposedChart>
      </ResponsiveContainer>

      {/* Max stagnation label */}
      {showStagnation && longestStag.length > 0 && (
        <p className="mt-1 text-center text-[10px] text-red-400/70">
          Max stagnation: {longestStagDays} days ({longestStag.start.slice(0, 10)} — {longestStag.end.slice(0, 10)})
        </p>
      )}
    </div>
  );
}
