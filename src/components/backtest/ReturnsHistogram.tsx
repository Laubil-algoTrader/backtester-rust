import {
  ResponsiveContainer,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Cell,
} from "recharts";

interface HistogramBin {
  label: string;
  count: number;
  midpoint: number;
}

function createHistogramBins(
  returns: number[],
  binCount: number
): HistogramBin[] {
  if (returns.length === 0) return [];

  const min = Math.min(...returns);
  const max = Math.max(...returns);

  if (min === max) {
    return [{ label: `$${min.toFixed(0)}`, count: returns.length, midpoint: min }];
  }

  const binWidth = (max - min) / binCount;
  const bins: HistogramBin[] = [];

  for (let i = 0; i < binCount; i++) {
    const lo = min + i * binWidth;
    const hi = lo + binWidth;
    const mid = (lo + hi) / 2;
    bins.push({
      label: `$${lo.toFixed(0)}`,
      count: 0,
      midpoint: mid,
    });
  }

  for (const r of returns) {
    let idx = Math.floor((r - min) / binWidth);
    if (idx >= binCount) idx = binCount - 1;
    bins[idx].count++;
  }

  return bins;
}

interface ReturnsHistogramProps {
  returns: number[];
}

export function ReturnsHistogram({ returns }: ReturnsHistogramProps) {
  if (returns.length === 0) return null;

  const bins = createHistogramBins(returns, Math.min(25, Math.max(5, Math.round(returns.length / 3))));

  return (
    <ResponsiveContainer width="100%" height={250}>
      <BarChart data={bins} margin={{ top: 5, right: 20, bottom: 5, left: 10 }}>
        <CartesianGrid strokeDasharray="3 3" stroke="hsl(220 10% 14%)" />
        <XAxis
          dataKey="label"
          tick={{ fontSize: 10, fill: "hsl(45 5% 40%)" }}
          stroke="hsl(220 10% 14%)"
          interval="preserveStartEnd"
        />
        <YAxis
          tick={{ fontSize: 10, fill: "hsl(45 5% 40%)" }}
          stroke="hsl(220 10% 14%)"
          allowDecimals={false}
          width={40}
        />
        <Tooltip
          contentStyle={{
            backgroundColor: "hsl(220 15% 7%)",
            border: "1px solid hsl(43 20% 18%)",
            borderRadius: 4,
            fontSize: 11,
            color: "hsl(45 10% 85%)",
          }}
          formatter={(value: number) => [value, "Trades"]}
          labelFormatter={(label: string) => `P&L: ${label}`}
        />
        <Bar dataKey="count" animationDuration={500}>
          {bins.map((bin, i) => (
            <Cell
              key={i}
              fill={
                bin.midpoint >= 0
                  ? "hsl(152 60% 42%)"
                  : "hsl(0 72% 50%)"
              }
              fillOpacity={0.8}
            />
          ))}
        </Bar>
      </BarChart>
    </ResponsiveContainer>
  );
}
