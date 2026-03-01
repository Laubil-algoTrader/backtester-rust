import { useState, useId, useRef } from "react";
import {
  Shield,
  Play,
  Loader2,
  AlertTriangle,
  TrendingUp,
  TrendingDown,
  Activity,
  Plus,
  Trash2,
  CheckCircle2,
  XCircle,
  ChevronDown,
  ChevronUp,
  Shuffle,
  SkipForward,
} from "lucide-react";
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  ReferenceLine,
  Cell,
} from "recharts";
import { useAppStore } from "@/stores/useAppStore";
import { runMonteCarlo } from "@/lib/tauri";
import type {
  MonteCarloResult,
  MonteCarloConfig,
  MonteCarloFilter,
  MonteCarloFilterMetric,
  MonteCarloFilterPercentile,
  MonteCarloFilterComparison,
  MonteCarloFilterThresholdType,
  Strategy,
} from "@/lib/types";
import { cn } from "@/lib/utils";

// ── helpers ───────────────────────────────────────────────────────────────────

function fmt(n: number, decimals = 2) {
  return n.toFixed(decimals);
}

function fmtPct(n: number) {
  return `${n >= 0 ? "+" : ""}${n.toFixed(2)}%`;
}

function riskColor(pct: number) {
  if (pct < 0.05) return "text-emerald-400";
  if (pct < 0.2) return "text-yellow-400";
  return "text-red-400";
}

/** Get the return-percentile value from a result for a given percentile key */
function getReturnPercentile(result: MonteCarloResult, p: MonteCarloFilterPercentile): number {
  switch (p) {
    case 5:  return result.p5_return_pct;
    case 25: return result.p25_return_pct;
    case 50: return result.median_return_pct;
    case 75: return result.p75_return_pct;
    case 95: return result.p95_return_pct;
  }
}

/** Get the drawdown-percentile value from a result for a given percentile key */
function getDdPercentile(result: MonteCarloResult, p: MonteCarloFilterPercentile): number {
  switch (p) {
    case 5:  return result.median_max_drawdown_pct * 0.5; // approximate P5 DD
    case 25: return result.p25_max_drawdown_pct;
    case 50: return result.median_max_drawdown_pct;
    case 75: return result.p75_max_drawdown_pct;
    case 95: return result.p95_max_drawdown_pct;
  }
}

function evaluateFilter(filter: MonteCarloFilter, result: MonteCarloResult): boolean {
  const rawValue =
    filter.metric === "net_return"
      ? getReturnPercentile(result, filter.percentile)
      : getDdPercentile(result, filter.percentile);

  let threshold = filter.threshold_value;
  if (filter.threshold_type === "pct_of_original") {
    const ref =
      filter.metric === "net_return"
        ? result.original_return_pct
        : result.original_max_drawdown_pct;
    threshold = ref * (filter.threshold_value / 100);
  }

  switch (filter.comparison) {
    case ">":  return rawValue > threshold;
    case "<":  return rawValue < threshold;
    case ">=": return rawValue >= threshold;
    case "<=": return rawValue <= threshold;
  }
}

// ── EquityFanChart ─────────────────────────────────────────────────────────────

const W = 880;
const H = 280;
const PAD = { top: 12, right: 24, bottom: 28, left: 60 };
const INNER_W = W - PAD.left - PAD.right;
const INNER_H = H - PAD.top - PAD.bottom;

function EquityFanChart({
  simCurves,
  originalCurve,
  initialCapital,
}: {
  simCurves: number[][];
  originalCurve: number[];
  initialCapital: number;
}) {
  // Global min / max across all curves + original + initial capital
  let minV = initialCapital;
  let maxV = initialCapital;
  for (const curve of simCurves) {
    for (const v of curve) {
      if (v < minV) minV = v;
      if (v > maxV) maxV = v;
    }
  }
  for (const v of originalCurve) {
    if (v < minV) minV = v;
    if (v > maxV) maxV = v;
  }
  const range = maxV - minV || 1;

  const toX = (i: number, total: number) =>
    PAD.left + (i / Math.max(total - 1, 1)) * INNER_W;
  const toY = (v: number) =>
    PAD.top + INNER_H - ((v - minV) / range) * INNER_H;

  const makePath = (curve: number[]) => {
    const n = curve.length;
    return curve
      .map(
        (v, i) =>
          `${i === 0 ? "M" : "L"}${toX(i, n).toFixed(1)},${toY(v).toFixed(1)}`
      )
      .join(" ");
  };

  // Y-axis ticks (5 evenly spaced values)
  const yTicks = 5;
  const yTickValues = Array.from({ length: yTicks }, (_, i) =>
    minV + (range / (yTicks - 1)) * i
  );

  // Dollar / generic formatter for y-axis
  const fmtY = (v: number) => {
    const abs = Math.abs(v);
    if (abs >= 1_000_000) return `$${(v / 1_000_000).toFixed(1)}M`;
    if (abs >= 1_000) return `$${(v / 1_000).toFixed(0)}k`;
    return `$${v.toFixed(0)}`;
  };

  const numPts = originalCurve.length;
  const tickStep = Math.max(1, Math.floor(numPts / 6));
  const xTicks = Array.from({ length: 7 }, (_, i) =>
    Math.min(Math.round(i * tickStep), numPts - 1)
  ).filter((v, i, a) => a.indexOf(v) === i);

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="w-full" style={{ height: 240 }}>
      {/* Background */}
      <rect x={PAD.left} y={PAD.top} width={INNER_W} height={INNER_H} fill="transparent" />

      {/* Grid lines + Y labels */}
      {yTickValues.map((v, i) => {
        const y = toY(v);
        return (
          <g key={i}>
            <line
              x1={PAD.left}
              y1={y}
              x2={W - PAD.right}
              y2={y}
              stroke="rgba(255,255,255,0.06)"
              strokeWidth="0.5"
            />
            <text
              x={PAD.left - 4}
              y={y + 3.5}
              textAnchor="end"
              fontSize="9"
              fill="rgba(255,255,255,0.35)"
              fontFamily="monospace"
            >
              {fmtY(v)}
            </text>
          </g>
        );
      })}

      {/* X labels */}
      {xTicks.map((idx) => (
        <text
          key={idx}
          x={toX(idx, numPts)}
          y={H - 4}
          textAnchor="middle"
          fontSize="9"
          fill="rgba(255,255,255,0.35)"
          fontFamily="monospace"
        >
          {idx}
        </text>
      ))}

      {/* Initial-capital reference line */}
      <line
        x1={PAD.left}
        y1={toY(initialCapital)}
        x2={W - PAD.right}
        y2={toY(initialCapital)}
        stroke="rgba(255,255,255,0.18)"
        strokeWidth="0.6"
        strokeDasharray="5,4"
      />

      {/* Simulation curves — rendered first so original is on top */}
      {simCurves.map((curve, i) => (
        <path
          key={i}
          d={makePath(curve)}
          fill="none"
          stroke="rgba(139,92,246,0.13)"
          strokeWidth="0.5"
        />
      ))}

      {/* Original curve — bold blue */}
      <path
        d={makePath(originalCurve)}
        fill="none"
        stroke="#60a5fa"
        strokeWidth="1.8"
        strokeLinejoin="round"
      />
    </svg>
  );
}

// ── FilterRow ─────────────────────────────────────────────────────────────────

function FilterRow({
  filter,
  result,
  onChange,
  onRemove,
}: {
  filter: MonteCarloFilter;
  result: MonteCarloResult | null;
  onChange: (f: MonteCarloFilter) => void;
  onRemove: () => void;
}) {
  const passes = result ? evaluateFilter(filter, result) : null;

  const metricLabel: Record<MonteCarloFilterMetric, string> = {
    net_return: "Net Return",
    max_drawdown: "Max Drawdown",
  };
  const compLabel: Record<MonteCarloFilterComparison, string> = {
    ">": ">",
    "<": "<",
    ">=": "≥",
    "<=": "≤",
  };

  const selectClass =
    "rounded border border-border bg-background px-2 py-1 text-xs focus:outline-none focus:ring-1 focus:ring-primary";

  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-2 rounded-md border px-3 py-2",
        passes === null
          ? "border-border/40 bg-card"
          : passes
          ? "border-emerald-500/30 bg-emerald-500/5"
          : "border-red-500/30 bg-red-500/5"
      )}
    >
      {/* Pass/fail badge */}
      {passes !== null &&
        (passes ? (
          <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-400" />
        ) : (
          <XCircle className="h-4 w-4 shrink-0 text-red-400" />
        ))}

      {/* Metric */}
      <select
        value={filter.metric}
        onChange={(e) =>
          onChange({ ...filter, metric: e.target.value as MonteCarloFilterMetric })
        }
        className={selectClass}
      >
        {(["net_return", "max_drawdown"] as MonteCarloFilterMetric[]).map((m) => (
          <option key={m} value={m}>
            {metricLabel[m]}
          </option>
        ))}
      </select>

      {/* Percentile */}
      <select
        value={filter.percentile}
        onChange={(e) =>
          onChange({
            ...filter,
            percentile: Number(e.target.value) as MonteCarloFilterPercentile,
          })
        }
        className={selectClass}
      >
        {([5, 25, 50, 75, 95] as MonteCarloFilterPercentile[]).map((p) => (
          <option key={p} value={p}>
            P{p}
          </option>
        ))}
      </select>

      {/* Comparison */}
      <select
        value={filter.comparison}
        onChange={(e) =>
          onChange({ ...filter, comparison: e.target.value as MonteCarloFilterComparison })
        }
        className={selectClass}
      >
        {([">", "<", ">=", "<="] as MonteCarloFilterComparison[]).map((c) => (
          <option key={c} value={c}>
            {compLabel[c]}
          </option>
        ))}
      </select>

      {/* Threshold value */}
      <input
        type="number"
        value={filter.threshold_value}
        onChange={(e) =>
          onChange({ ...filter, threshold_value: Number(e.target.value) })
        }
        className="w-20 rounded border border-border bg-background px-2 py-1 text-xs focus:outline-none focus:ring-1 focus:ring-primary"
        step="any"
      />

      {/* Threshold type */}
      <select
        value={filter.threshold_type}
        onChange={(e) =>
          onChange({
            ...filter,
            threshold_type: e.target.value as MonteCarloFilterThresholdType,
          })
        }
        className={selectClass}
      >
        <option value="absolute">%</option>
        <option value="pct_of_original">% del original</option>
      </select>

      {/* Result value */}
      {result && (
        <span className="ml-1 text-[11px] font-mono text-muted-foreground">
          (actual:{" "}
          {filter.metric === "net_return"
            ? fmtPct(getReturnPercentile(result, filter.percentile))
            : `-${fmt(getDdPercentile(result, filter.percentile))}%`}
          )
        </span>
      )}

      <button
        onClick={onRemove}
        className="ml-auto rounded p-0.5 text-muted-foreground hover:text-destructive"
      >
        <Trash2 className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

// ── StatCard ──────────────────────────────────────────────────────────────────

function StatCard({
  label,
  value,
  sub,
  valueClass,
}: {
  label: string;
  value: string;
  sub?: string;
  valueClass?: string;
}) {
  return (
    <div className="flex flex-col gap-1 rounded-lg border border-border/40 bg-card p-3">
      <span className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
        {label}
      </span>
      <span className={cn("text-xl font-bold tabular-nums", valueClass ?? "text-foreground")}>
        {value}
      </span>
      {sub && <span className="text-xs text-muted-foreground">{sub}</span>}
    </div>
  );
}

// ── PercentilesBar / DrawdownRiskBar ──────────────────────────────────────────

function PercentilesBar({ result }: { result: MonteCarloResult }) {
  const bars = [
    { label: "P5", value: result.p5_return_pct },
    { label: "P25", value: result.p25_return_pct },
    { label: "Mediana", value: result.median_return_pct },
    { label: "P75", value: result.p75_return_pct },
    { label: "P95", value: result.p95_return_pct },
  ];

  return (
    <div className="rounded-lg border border-border/40 bg-card p-4">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold">
        <TrendingUp className="h-4 w-4 text-primary" />
        Distribución de Retornos
      </h3>
      <div className="h-48">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart data={bars} margin={{ top: 4, right: 8, left: 0, bottom: 4 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="currentColor" className="opacity-10" />
            <XAxis
              dataKey="label"
              tick={{ fontSize: 11, fill: "currentColor" }}
              className="text-muted-foreground"
            />
            <YAxis
              tick={{ fontSize: 11, fill: "currentColor" }}
              className="text-muted-foreground"
              tickFormatter={(v) => `${v.toFixed(0)}%`}
            />
            <Tooltip
              formatter={(v: number) => [`${fmtPct(v)}`, "Retorno"]}
              contentStyle={{
                background: "hsl(var(--card))",
                border: "1px solid hsl(var(--border))",
                borderRadius: 6,
                fontSize: 12,
              }}
            />
            <ReferenceLine y={0} stroke="hsl(var(--muted-foreground))" strokeDasharray="4 4" />
            <Bar dataKey="value" radius={[4, 4, 0, 0]}>
              {bars.map((b) => (
                <Cell
                  key={b.label}
                  fill={b.value >= 0 ? "hsl(var(--primary))" : "hsl(var(--destructive))"}
                  fillOpacity={0.85}
                />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}

function DrawdownRiskBar({ result }: { result: MonteCarloResult }) {
  const bars = [
    { label: "P25 DD", value: result.p25_max_drawdown_pct },
    { label: "Mediana DD", value: result.median_max_drawdown_pct },
    { label: "P75 DD", value: result.p75_max_drawdown_pct },
    { label: "P95 DD", value: result.p95_max_drawdown_pct },
  ];

  return (
    <div className="rounded-lg border border-border/40 bg-card p-4">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold">
        <TrendingDown className="h-4 w-4 text-destructive" />
        Drawdown Máximo Simulado
      </h3>
      <div className="h-48">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart data={bars} margin={{ top: 4, right: 8, left: 0, bottom: 4 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="currentColor" className="opacity-10" />
            <XAxis
              dataKey="label"
              tick={{ fontSize: 11, fill: "currentColor" }}
              className="text-muted-foreground"
            />
            <YAxis
              tick={{ fontSize: 11, fill: "currentColor" }}
              className="text-muted-foreground"
              tickFormatter={(v) => `${v.toFixed(0)}%`}
            />
            <Tooltip
              formatter={(v: number) => [`-${v.toFixed(2)}%`, "Max Drawdown"]}
              contentStyle={{
                background: "hsl(var(--card))",
                border: "1px solid hsl(var(--border))",
                borderRadius: 6,
                fontSize: 12,
              }}
            />
            <Bar
              dataKey="value"
              radius={[4, 4, 0, 0]}
              fill="hsl(var(--destructive))"
              fillOpacity={0.8}
            />
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}

// ── main page ─────────────────────────────────────────────────────────────────

export function RobustezPage() {
  const backtestResults = useAppStore((s) => s.backtestResults);
  const savedStrategies = useAppStore((s) => s.savedStrategies);
  const currentStrategy = useAppStore((s) => s.currentStrategy);
  const initialCapital = useAppStore((s) => s.initialCapital);
  const monteCarloResults = useAppStore((s) => s.monteCarloResults);
  const setMonteCarloResults = useAppStore((s) => s.setMonteCarloResults);

  // ── config state ──────────────────────────────────────────────────────────
  const [method, setMethod] = useState<"Resampling" | "SkipTrades">("Resampling");
  const [skipProbability, setSkipProbability] = useState(10); // percent
  const [nSimulations, setNSimulations] = useState(1000);
  const [selectedStrategyId, setSelectedStrategyId] = useState<string>("__current__");
  const [showDataset, setShowDataset] = useState(false);

  // ── filter state ──────────────────────────────────────────────────────────
  const [filters, setFilters] = useState<MonteCarloFilter[]>([]);
  const filterId = useRef(0);

  // ── run state ─────────────────────────────────────────────────────────────
  const [isRunning, setIsRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef(false);

  // ── helpers ───────────────────────────────────────────────────────────────
  const strategyOptions: { id: string; label: string; strategy?: Strategy }[] = [
    {
      id: "__current__",
      label: `Estrategia actual${currentStrategy.name ? `: ${currentStrategy.name}` : ""}`,
    },
    ...savedStrategies.map((s) => ({ id: s.id, label: s.name, strategy: s })),
  ];

  const activeStrategy =
    selectedStrategyId === "__current__"
      ? currentStrategy
      : savedStrategies.find((s) => s.id === selectedStrategyId);

  const trades = backtestResults?.trades ?? [];
  const canRun = trades.length >= 5;

  const allFiltersPassing =
    monteCarloResults && filters.length > 0
      ? filters.every((f) => evaluateFilter(f, monteCarloResults))
      : null;

  const addFilter = () => {
    filterId.current += 1;
    setFilters((prev) => [
      ...prev,
      {
        id: String(filterId.current),
        metric: "net_return",
        percentile: 25,
        comparison: ">",
        threshold_type: "absolute",
        threshold_value: 0,
      },
    ]);
  };

  const updateFilter = (id: string, f: MonteCarloFilter) =>
    setFilters((prev) => prev.map((x) => (x.id === id ? f : x)));

  const removeFilter = (id: string) =>
    setFilters((prev) => prev.filter((x) => x.id !== id));

  // ── run handler ───────────────────────────────────────────────────────────
  const handleRun = async () => {
    if (!canRun || isRunning) return;
    setIsRunning(true);
    setError(null);
    abortRef.current = false;

    const config: MonteCarloConfig = {
      n_simulations: nSimulations,
      method,
      skip_probability: skipProbability / 100,
    };

    try {
      const result = await runMonteCarlo(trades, initialCapital, config);
      if (!abortRef.current) {
        setMonteCarloResults(result);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setIsRunning(false);
    }
  };

  // ── render ────────────────────────────────────────────────────────────────
  return (
    <div className="space-y-5">
      {/* ── Header ── */}
      <div className="flex items-center gap-3">
        <Shield className="h-5 w-5 text-primary" />
        <div>
          <h1 className="text-lg font-bold">Robustez</h1>
          <p className="text-xs text-muted-foreground">
            Simulación Monte Carlo sobre trades históricos
          </p>
        </div>
      </div>

      {/* ── Config card ── */}
      <div className="rounded-lg border border-border/40 bg-card p-4 space-y-4">
        <h2 className="text-sm font-semibold">Configuración</h2>

        {/* Method selector */}
        <div>
          <label className="mb-1.5 block text-xs font-medium text-muted-foreground">
            Método
          </label>
          <div className="flex gap-2">
            <button
              onClick={() => setMethod("Resampling")}
              className={cn(
                "flex items-center gap-2 rounded-md border px-3 py-2 text-xs font-medium transition-colors",
                method === "Resampling"
                  ? "border-primary/60 bg-primary/10 text-primary"
                  : "border-border/40 bg-background text-muted-foreground hover:text-foreground"
              )}
            >
              <Shuffle className="h-3.5 w-3.5" />
              Resampling
            </button>
            <button
              onClick={() => setMethod("SkipTrades")}
              className={cn(
                "flex items-center gap-2 rounded-md border px-3 py-2 text-xs font-medium transition-colors",
                method === "SkipTrades"
                  ? "border-primary/60 bg-primary/10 text-primary"
                  : "border-border/40 bg-background text-muted-foreground hover:text-foreground"
              )}
            >
              <SkipForward className="h-3.5 w-3.5" />
              Skip Trades
            </button>
          </div>
          <p className="mt-1.5 text-[11px] text-muted-foreground">
            {method === "Resampling"
              ? "Elige trades al azar con repetición — cada simulación tiene una distribución de P&L diferente."
              : "Recorre los trades en orden saltando cada uno con probabilidad p — modela operaciones fallidas."}
          </p>
        </div>

        <div className="grid grid-cols-2 gap-4">
          {/* Strategy selector */}
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">Estrategia</label>
            <select
              value={selectedStrategyId}
              onChange={(e) => setSelectedStrategyId(e.target.value)}
              className="rounded-md border border-border bg-background px-3 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-primary"
            >
              {strategyOptions.map((opt) => (
                <option key={opt.id} value={opt.id}>
                  {opt.label}
                </option>
              ))}
            </select>
            <span className="text-[11px] text-muted-foreground">
              Trades del último backtest ejecutado
            </span>
          </div>

          {/* Simulations count */}
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">Nº de Simulaciones</label>
            <input
              type="number"
              min={100}
              max={100000}
              step={100}
              value={nSimulations}
              onChange={(e) =>
                setNSimulations(Math.max(100, Math.min(100000, Number(e.target.value))))
              }
              className="rounded-md border border-border bg-background px-3 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-primary"
            />
            <span className="text-[11px] text-muted-foreground">100 – 100,000</span>
          </div>
        </div>

        {/* Skip probability (only for SkipTrades) */}
        {method === "SkipTrades" && (
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-muted-foreground">
              Probabilidad de saltar trade:{" "}
              <span className="font-bold text-foreground">{skipProbability}%</span>
            </label>
            <div className="flex items-center gap-3">
              <input
                type="range"
                min={1}
                max={50}
                step={1}
                value={skipProbability}
                onChange={(e) => setSkipProbability(Number(e.target.value))}
                className="flex-1 accent-primary"
              />
              <span className="w-10 text-right text-xs font-mono text-foreground">
                {skipProbability}%
              </span>
            </div>
            <span className="text-[11px] text-muted-foreground">
              Con {skipProbability}% de probabilidad, cada trade es ignorado en la simulación.
            </span>
          </div>
        )}

        {/* Dataset details toggle */}
        <button
          onClick={() => setShowDataset((v) => !v)}
          className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
        >
          {showDataset ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
          Detalles del dataset
        </button>
        {showDataset && (
          <div className="rounded-md border border-border/30 bg-background/50 p-3 text-xs text-muted-foreground space-y-1">
            <div>
              Trades disponibles:{" "}
              <span className="font-semibold text-foreground">{trades.length}</span>
            </div>
            <div>
              Capital inicial:{" "}
              <span className="font-semibold text-foreground">
                ${initialCapital.toLocaleString()}
              </span>
            </div>
            <div>
              Estrategia seleccionada:{" "}
              <span className="font-semibold text-foreground">
                {activeStrategy?.name ?? "–"}
              </span>
            </div>
          </div>
        )}

        {/* Warning: no trades */}
        {!canRun && (
          <div className="flex items-center gap-2 rounded-md border border-yellow-500/30 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-400">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            Ejecutá un backtest primero. Se necesitan al menos 5 trades para la simulación.
          </div>
        )}

        {/* Run button */}
        <div className="flex items-center gap-3">
          <button
            onClick={handleRun}
            disabled={!canRun || isRunning}
            className={cn(
              "flex items-center gap-2 rounded-md px-4 py-2 text-sm font-medium transition-colors",
              canRun && !isRunning
                ? "bg-primary text-primary-foreground hover:bg-primary/90"
                : "cursor-not-allowed bg-muted text-muted-foreground"
            )}
          >
            {isRunning ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                Simulando…
              </>
            ) : (
              <>
                <Play className="h-4 w-4" />
                Ejecutar Monte Carlo
              </>
            )}
          </button>
          {isRunning && (
            <button
              onClick={() => {
                abortRef.current = true;
                setIsRunning(false);
              }}
              className="rounded-md border border-border px-3 py-2 text-xs text-muted-foreground hover:bg-foreground/[0.05]"
            >
              Cancelar
            </button>
          )}
        </div>

        {error && (
          <div className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
            {error}
          </div>
        )}
      </div>

      {/* ── Filters card ── */}
      <div className="rounded-lg border border-border/40 bg-card p-4 space-y-3">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-semibold">Filtros de aprobación</h2>
          {allFiltersPassing !== null && (
            <span
              className={cn(
                "flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-bold",
                allFiltersPassing
                  ? "bg-emerald-500/15 text-emerald-400"
                  : "bg-red-500/15 text-red-400"
              )}
            >
              {allFiltersPassing ? (
                <CheckCircle2 className="h-3.5 w-3.5" />
              ) : (
                <XCircle className="h-3.5 w-3.5" />
              )}
              {allFiltersPassing ? "APROBADO" : "REPROBADO"}
            </span>
          )}
        </div>

        <p className="text-[11px] text-muted-foreground">
          Definí criterios mínimos que los resultados deben cumplir. Ej: P25 Net Return ≥ 0%,
          P95 Max Drawdown ≤ 30%.
        </p>

        {filters.length === 0 && (
          <p className="text-xs italic text-muted-foreground">Sin filtros definidos.</p>
        )}

        <div className="space-y-2">
          {filters.map((f) => (
            <FilterRow
              key={f.id}
              filter={f}
              result={monteCarloResults}
              onChange={(upd) => updateFilter(f.id, upd)}
              onRemove={() => removeFilter(f.id)}
            />
          ))}
        </div>

        <button
          onClick={addFilter}
          className="flex items-center gap-1.5 rounded-md border border-dashed border-border/60 px-3 py-1.5 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary transition-colors"
        >
          <Plus className="h-3.5 w-3.5" />
          Agregar filtro
        </button>
      </div>

      {/* ── Results ── */}
      {monteCarloResults && (
        <div className="space-y-4">
          {/* Equity fan chart */}
          <div className="rounded-lg border border-border/40 bg-card p-4">
            <div className="mb-3 flex items-center justify-between">
              <h3 className="flex items-center gap-2 text-sm font-semibold">
                <Activity className="h-4 w-4 text-primary" />
                Curvas de Equity — {monteCarloResults.sim_equity_curves.length} simulaciones
              </h3>
              <div className="flex items-center gap-3 text-xs text-muted-foreground">
                <span className="flex items-center gap-1">
                  <span
                    className="inline-block h-2.5 w-5 rounded-sm"
                    style={{ background: "rgba(139,92,246,0.4)" }}
                  />
                  Simulaciones
                </span>
                <span className="flex items-center gap-1">
                  <span
                    className="inline-block h-2.5 w-5 rounded-sm"
                    style={{ background: "#60a5fa" }}
                  />
                  Original
                </span>
              </div>
            </div>
            <EquityFanChart
              simCurves={monteCarloResults.sim_equity_curves}
              originalCurve={monteCarloResults.original_equity_curve}
              initialCapital={initialCapital}
            />
          </div>

          {/* Summary stat cards */}
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <StatCard
              label="Retorno Mediano"
              value={fmtPct(monteCarloResults.median_return_pct)}
              valueClass={
                monteCarloResults.median_return_pct >= 0 ? "text-emerald-400" : "text-red-400"
              }
              sub="P50 de todas las simulaciones"
            />
            <StatCard
              label="Rango P5 – P95"
              value={`${fmtPct(monteCarloResults.p5_return_pct)} / ${fmtPct(monteCarloResults.p95_return_pct)}`}
              sub="90% de los resultados cae aquí"
            />
            <StatCard
              label="Probabilidad de Pérdida"
              value={`${(monteCarloResults.ruin_probability * 100).toFixed(1)}%`}
              valueClass={riskColor(monteCarloResults.ruin_probability)}
              sub="Equity < capital inicial"
            />
            <StatCard
              label="DD Máx. P95"
              value={`-${fmt(monteCarloResults.p95_max_drawdown_pct)}%`}
              valueClass="text-red-400"
              sub="Peor escenario (95th percentile)"
            />
          </div>

          {/* Risk badge */}
          <div
            className={cn(
              "flex items-center gap-3 rounded-lg border px-4 py-3",
              monteCarloResults.ruin_probability < 0.05
                ? "border-emerald-500/30 bg-emerald-500/10"
                : monteCarloResults.ruin_probability < 0.2
                ? "border-yellow-500/30 bg-yellow-500/10"
                : "border-red-500/30 bg-red-500/10"
            )}
          >
            <Shield
              className={cn(
                "h-5 w-5",
                monteCarloResults.ruin_probability < 0.05
                  ? "text-emerald-400"
                  : monteCarloResults.ruin_probability < 0.2
                  ? "text-yellow-400"
                  : "text-red-400"
              )}
            />
            <div>
              <p
                className={cn(
                  "text-sm font-semibold",
                  monteCarloResults.ruin_probability < 0.05
                    ? "text-emerald-400"
                    : monteCarloResults.ruin_probability < 0.2
                    ? "text-yellow-400"
                    : "text-red-400"
                )}
              >
                {monteCarloResults.ruin_probability < 0.05
                  ? "Estrategia robusta"
                  : monteCarloResults.ruin_probability < 0.2
                  ? "Riesgo moderado"
                  : "Riesgo elevado"}
              </p>
              <p className="text-xs text-muted-foreground">
                {monteCarloResults.n_simulations.toLocaleString()} simulaciones ·{" "}
                {method === "Resampling" ? "Resampling" : `Skip ${skipProbability}%`} ·{" "}
                {monteCarloResults.p25_return_pct >= 0
                  ? "El 75% de los escenarios es positivo"
                  : "Más del 25% de los escenarios termina en pérdida"}
              </p>
            </div>
          </div>

          {/* Charts */}
          <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
            <PercentilesBar result={monteCarloResults} />
            <DrawdownRiskBar result={monteCarloResults} />
          </div>

          {/* Full percentile table */}
          <div className="rounded-lg border border-border/40 bg-card p-4">
            <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold">
              <TrendingUp className="h-4 w-4 text-primary" />
              Tabla de Percentiles
            </h3>
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border/40 text-left text-xs text-muted-foreground">
                    <th className="pb-2 pr-4 font-medium">Percentil</th>
                    <th className="pb-2 pr-4 font-medium">Retorno</th>
                    <th className="pb-2 pr-4 font-medium">Max Drawdown</th>
                    <th className="pb-2 font-medium">Interpretación</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/20">
                  {[
                    {
                      label: "P5 (peor 5%)",
                      ret: monteCarloResults.p5_return_pct,
                      dd: null,
                      desc: "Escenario muy adverso",
                    },
                    {
                      label: "P25",
                      ret: monteCarloResults.p25_return_pct,
                      dd: monteCarloResults.p25_max_drawdown_pct,
                      desc: "1 de cada 4 corre peor",
                    },
                    {
                      label: "P50 (mediana)",
                      ret: monteCarloResults.median_return_pct,
                      dd: monteCarloResults.median_max_drawdown_pct,
                      desc: "Resultado más probable",
                    },
                    {
                      label: "P75",
                      ret: monteCarloResults.p75_return_pct,
                      dd: monteCarloResults.p75_max_drawdown_pct,
                      desc: "1 de cada 4 corre mejor",
                    },
                    {
                      label: "P95 (mejor 5%)",
                      ret: monteCarloResults.p95_return_pct,
                      dd: monteCarloResults.p95_max_drawdown_pct,
                      desc: "Escenario muy favorable",
                    },
                  ].map((row) => (
                    <tr key={row.label}>
                      <td className="py-2 pr-4 text-muted-foreground">{row.label}</td>
                      <td
                        className={cn(
                          "py-2 pr-4 font-mono font-semibold",
                          row.ret >= 0 ? "text-emerald-400" : "text-red-400"
                        )}
                      >
                        {fmtPct(row.ret)}
                      </td>
                      <td className="py-2 pr-4 font-mono font-semibold text-red-400">
                        {row.dd !== null ? `-${fmt(row.dd)}%` : "–"}
                      </td>
                      <td className="py-2 text-xs text-muted-foreground">{row.desc}</td>
                    </tr>
                  ))}
                  <tr className="border-t border-border/30 bg-foreground/[0.02]">
                    <td className="py-2 pr-4 font-medium">Original</td>
                    <td
                      className={cn(
                        "py-2 pr-4 font-mono font-semibold",
                        monteCarloResults.original_return_pct >= 0
                          ? "text-blue-400"
                          : "text-red-400"
                      )}
                    >
                      {fmtPct(monteCarloResults.original_return_pct)}
                    </td>
                    <td className="py-2 pr-4 font-mono font-semibold text-blue-400">
                      -{fmt(monteCarloResults.original_max_drawdown_pct)}%
                    </td>
                    <td className="py-2 text-xs text-muted-foreground">
                      Backtest histórico real
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>

          {/* Interpretation guide */}
          <div className="rounded-lg border border-border/30 bg-background/40 p-4">
            <h3 className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              <Activity className="h-3.5 w-3.5" />
              Cómo interpretar
            </h3>
            <ul className="space-y-1 text-xs text-muted-foreground">
              <li>
                <span className="font-medium text-foreground">Resampling</span> — re-sortea trades
                con repetición: cada sim puede incluir el mismo trade varias veces. Evalúa si el
                edge es estadísticamente estable.
              </li>
              <li>
                <span className="font-medium text-foreground">Skip Trades</span> — omite cada trade
                con probabilidad {skipProbability}%. Modela errores de ejecución, cortes de internet,
                o filtros adicionales.
              </li>
              <li>
                <span className="font-medium text-foreground">P25 positivo</span> — 75% de los
                escenarios termina en ganancia.
              </li>
              <li>
                <span className="font-medium text-foreground">Probabilidad de pérdida baja</span>{" "}
                (&lt;5%) — la estrategia es estadísticamente robusta.
              </li>
            </ul>
          </div>
        </div>
      )}
    </div>
  );
}
