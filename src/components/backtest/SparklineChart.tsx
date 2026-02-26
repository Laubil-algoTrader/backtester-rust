import { useMemo } from "react";
import { ResponsiveContainer, AreaChart, Area } from "recharts";

interface SparklineChartProps {
  data: { value: number }[];
  color?: "green" | "red" | "blue";
  height?: number;
  id: string;
}

const COLOR_MAP = {
  green: { stroke: "hsl(152 60% 42%)", fill: "hsl(152 60% 42%)" },
  red:   { stroke: "hsl(0 72% 50%)",   fill: "hsl(0 72% 50%)" },
  blue:  { stroke: "hsl(210 80% 55%)", fill: "hsl(210 80% 55%)" },
} as const;

export function SparklineChart({ data, color = "green", height = 60, id }: SparklineChartProps) {
  const c = COLOR_MAP[color];
  const gradientId = `sparkGrad-${id}`;

  // Downsample to max 80 points for performance
  const sampled = useMemo(() => {
    if (data.length <= 80) return data;
    const step = Math.floor(data.length / 80);
    return data.filter((_, i) => i % step === 0 || i === data.length - 1);
  }, [data]);

  if (sampled.length < 2) return null;

  return (
    <ResponsiveContainer width="100%" height={height}>
      <AreaChart data={sampled} margin={{ top: 2, right: 0, bottom: 0, left: 0 }}>
        <defs>
          <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={c.fill} stopOpacity={0.3} />
            <stop offset="100%" stopColor={c.fill} stopOpacity={0.02} />
          </linearGradient>
        </defs>
        <Area
          type="monotone"
          dataKey="value"
          stroke={c.stroke}
          fill={`url(#${gradientId})`}
          strokeWidth={1.5}
          dot={false}
          isAnimationActive={false}
        />
      </AreaChart>
    </ResponsiveContainer>
  );
}
