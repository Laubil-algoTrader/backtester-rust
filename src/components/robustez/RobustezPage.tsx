import { useState, useRef } from "react";
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
  Shuffle,
  SkipForward,
  BarChart3,
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
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/Tabs";
import { Button } from "@/components/ui/Button";
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
import { TOOLTIP_STYLE } from "@/lib/chartTheme";

// ── helpers ───────────────────────────────────────────────────────────────────

function fmt(n: number, decimals = 2) {
  return n.toFixed(decimals);
}

function fmtPct(n: number) {
  return `${n >= 0 ? "+" : ""}${n.toFixed(2)}%`;
}

function riskColor(pct: number): string {
  if (pct < 0.05) return "text-emerald-400";
  if (pct < 0.2) return "text-yellow-400";
  return "text-red-400";
}

function getReturnPercentile(result: MonteCarloResult, p: MonteCarloFilterPercentile): number {
  switch (p) {
    case 5:  return result.p5_return_pct;
    case 25: return result.p25_return_pct;
    case 50: return result.median_return_pct;
    case 75: return result.p75_return_pct;
    case 95: return result.p95_return_pct;
  }
}

function getDdPercentile(result: MonteCarloResult, p: MonteCarloFilterPercentile): number {
  switch (p) {
    case 5:  return result.median_max_drawdown_pct * 0.4;
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

// ── EquityFanChart (SVG) ──────────────────────────────────────────────────────

const VB_W = 1000;
const VB_H = 320;
const PAD = { top: 16, right: 20, bottom: 32, left: 62 };
const IW = VB_W - PAD.left - PAD.right;
const IH = VB_H - PAD.top - PAD.bottom;

function EquityFanChart({
  simCurves,
  originalCurve,
  initialCapital,
}: {
  simCurves: number[][];
  originalCurve: number[];
  initialCapital: number;
}) {
  if (simCurves.length === 0 && originalCurve.length === 0) return null;

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
  // Add 5% padding top/bottom
  const padV = range * 0.05;
  const lo = minV - padV;
  const hi = maxV + padV;
  const span = hi - lo;

  const toX = (i: number, total: number) =>
    PAD.left + (i / Math.max(total - 1, 1)) * IW;
  const toY = (v: number) =>
    PAD.top + IH - ((v - lo) / span) * IH;

  const makePath = (curve: number[]) => {
    const n = curve.length;
    return curve
      .map((v, i) => `${i === 0 ? "M" : "L"}${toX(i, n).toFixed(1)},${toY(v).toFixed(1)}`)
      .join(" ");
  };

  const Y_TICKS = 5;
  const yTickValues = Array.from({ length: Y_TICKS }, (_, i) =>
    lo + (span / (Y_TICKS - 1)) * i
  );

  const fmtMoney = (v: number) => {
    const abs = Math.abs(v);
    if (abs >= 1_000_000) return `$${(v / 1_000_000).toFixed(1)}M`;
    if (abs >= 1_000) return `$${(v / 1_000).toFixed(0)}k`;
    return `$${v.toFixed(0)}`;
  };

  const numPts = originalCurve.length;
  const xTickCount = 7;
  const xTicks = Array.from({ length: xTickCount }, (_, i) =>
    Math.round((i / (xTickCount - 1)) * (numPts - 1))
  );

  return (
    <svg
      viewBox={`0 0 ${VB_W} ${VB_H}`}
      preserveAspectRatio="none"
      style={{ width: "100%", height: "100%", display: "block" }}
    >
      {/* Y grid + labels */}
      {yTickValues.map((v, i) => {
        const y = toY(v);
        return (
          <g key={i}>
            <line
              x1={PAD.left}
              y1={y}
              x2={VB_W - PAD.right}
              y2={y}
              stroke="rgba(255,255,255,0.05)"
              strokeWidth="0.6"
            />
            <text
              x={PAD.left - 5}
              y={y + 4}
              textAnchor="end"
              fontSize="11"
              fill="rgba(255,255,255,0.38)"
              fontFamily="ui-monospace,monospace"
            >
              {fmtMoney(v)}
            </text>
          </g>
        );
      })}

      {/* X labels */}
      {xTicks.map((idx) => (
        <text
          key={idx}
          x={toX(idx, numPts)}
          y={VB_H - 6}
          textAnchor="middle"
          fontSize="10"
          fill="rgba(255,255,255,0.3)"
          fontFamily="ui-monospace,monospace"
        >
          {idx}
        </text>
      ))}

      {/* Initial-capital line */}
      <line
        x1={PAD.left}
        y1={toY(initialCapital)}
        x2={VB_W - PAD.right}
        y2={toY(initialCapital)}
        stroke="rgba(255,255,255,0.2)"
        strokeWidth="0.7"
        strokeDasharray="6,4"
      />

      {/* Simulation curves */}
      {simCurves.map((curve, i) => (
        <path
          key={i}
          d={makePath(curve)}
          fill="none"
          stroke="rgba(139,92,246,0.12)"
          strokeWidth="0.55"
        />
      ))}

      {/* Original (historical) curve */}
      <path
        d={makePath(originalCurve)}
        fill="none"
        stroke="#60a5fa"
        strokeWidth="2"
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
  const sel = "rounded border border-border bg-background px-2 py-1 text-xs focus:outline-none focus:ring-1 focus:ring-primary";

  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-2 rounded-md border px-3 py-2 text-sm",
        passes === null
          ? "border-border/40 bg-background"
          : passes
          ? "border-emerald-500/30 bg-emerald-500/5"
          : "border-red-500/30 bg-red-500/5"
      )}
    >
      {passes !== null &&
        (passes ? (
          <CheckCircle2 className="h-4 w-4 shrink-0 text-emerald-400" />
        ) : (
          <XCircle className="h-4 w-4 shrink-0 text-red-400" />
        ))}

      <select
        value={filter.metric}
        onChange={(e) => onChange({ ...filter, metric: e.target.value as MonteCarloFilterMetric })}
        className={sel}
      >
        <option value="net_return">Net Return</option>
        <option value="max_drawdown">Max Drawdown</option>
      </select>

      <select
        value={filter.percentile}
        onChange={(e) =>
          onChange({ ...filter, percentile: Number(e.target.value) as MonteCarloFilterPercentile })
        }
        className={sel}
      >
        {([5, 25, 50, 75, 95] as MonteCarloFilterPercentile[]).map((p) => (
          <option key={p} value={p}>P{p}</option>
        ))}
      </select>

      <select
        value={filter.comparison}
        onChange={(e) =>
          onChange({ ...filter, comparison: e.target.value as MonteCarloFilterComparison })
        }
        className={sel}
      >
        <option value=">">{">"}</option>
        <option value="<">{"<"}</option>
        <option value=">=">{">="}</option>
        <option value="<=">{"<="}</option>
      </select>

      <input
        type="number"
        value={filter.threshold_value}
        onChange={(e) => onChange({ ...filter, threshold_value: Number(e.target.value) })}
        className="w-20 rounded border border-border bg-background px-2 py-1 text-xs focus:outline-none focus:ring-1 focus:ring-primary"
        step="any"
      />

      <select
        value={filter.threshold_type}
        onChange={(e) =>
          onChange({ ...filter, threshold_type: e.target.value as MonteCarloFilterThresholdType })
        }
        className={sel}
      >
        <option value="absolute">%</option>
        <option value="pct_of_original">% del original</option>
      </select>

      {result && (
        <span className="ml-1 font-mono text-[11px] text-muted-foreground">
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

// ── ResultView (shared view for a single method result) ───────────────────────

function ResultView({
  result,
  filters,
  initialCapital,
  methodLabel,
}: {
  result: MonteCarloResult;
  filters: MonteCarloFilter[];
  initialCapital: number;
  methodLabel: string;
}) {
  const allPass =
    filters.length > 0 ? filters.every((f) => evaluateFilter(f, result)) : null;

  const returnBars = [
    { label: "P5",    value: result.p5_return_pct },
    { label: "P25",   value: result.p25_return_pct },
    { label: "Med",   value: result.median_return_pct },
    { label: "P75",   value: result.p75_return_pct },
    { label: "P95",   value: result.p95_return_pct },
  ];

  const ddBars = [
    { label: "P25",   value: result.p25_max_drawdown_pct },
    { label: "Med",   value: result.median_max_drawdown_pct },
    { label: "P75",   value: result.p75_max_drawdown_pct },
    { label: "P95",   value: result.p95_max_drawdown_pct },
  ];

  return (
    <div className="space-y-4">
      {/* Equity fan chart */}
      <Card>
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between">
            <CardTitle>
              Curvas de Equity — {result.sim_equity_curves.length} simulaciones · {methodLabel}
            </CardTitle>
            <div className="flex items-center gap-3 text-xs text-muted-foreground">
              <span className="flex items-center gap-1.5">
                <span className="inline-block h-2 w-5 rounded-sm" style={{ background: "rgba(139,92,246,0.5)" }} />
                Simulaciones
              </span>
              <span className="flex items-center gap-1.5">
                <span className="inline-block h-2 w-5 rounded-sm bg-blue-400" />
                Original
              </span>
            </div>
          </div>
        </CardHeader>
        <CardContent className="pb-4">
          <div style={{ height: 320 }}>
            <EquityFanChart
              simCurves={result.sim_equity_curves}
              originalCurve={result.original_equity_curve}
              initialCapital={initialCapital}
            />
          </div>
        </CardContent>
      </Card>

      {/* Stat cards + risk badge */}
      <Card>
        <CardContent className="pt-4 space-y-4">
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            {[
              {
                label: "Retorno Mediano",
                value: fmtPct(result.median_return_pct),
                valueClass: result.median_return_pct >= 0 ? "text-emerald-400" : "text-red-400",
                sub: "P50",
              },
              {
                label: "Rango P5 – P95",
                value: `${fmtPct(result.p5_return_pct)} / ${fmtPct(result.p95_return_pct)}`,
                valueClass: "text-foreground",
                sub: "90% de resultados",
              },
              {
                label: "Prob. de Pérdida",
                value: `${(result.ruin_probability * 100).toFixed(1)}%`,
                valueClass: riskColor(result.ruin_probability),
                sub: "Equity < capital inicial",
              },
              {
                label: "DD Máx. P95",
                value: `-${fmt(result.p95_max_drawdown_pct)}%`,
                valueClass: "text-red-400",
                sub: "Peor 5% de escenarios",
              },
            ].map((c) => (
              <div
                key={c.label}
                className="flex flex-col gap-1 rounded-lg border border-border/40 bg-background p-3"
              >
                <span className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
                  {c.label}
                </span>
                <span className={cn("text-xl font-bold tabular-nums", c.valueClass)}>
                  {c.value}
                </span>
                <span className="text-xs text-muted-foreground">{c.sub}</span>
              </div>
            ))}
          </div>

          {/* Risk + filters badge */}
          <div className="flex flex-wrap items-center gap-3">
            <div
              className={cn(
                "flex flex-1 items-center gap-3 rounded-lg border px-4 py-3",
                result.ruin_probability < 0.05
                  ? "border-emerald-500/30 bg-emerald-500/10"
                  : result.ruin_probability < 0.2
                  ? "border-yellow-500/30 bg-yellow-500/10"
                  : "border-red-500/30 bg-red-500/10"
              )}
            >
              <Shield
                className={cn(
                  "h-5 w-5 shrink-0",
                  result.ruin_probability < 0.05
                    ? "text-emerald-400"
                    : result.ruin_probability < 0.2
                    ? "text-yellow-400"
                    : "text-red-400"
                )}
              />
              <div>
                <p
                  className={cn(
                    "text-sm font-semibold",
                    result.ruin_probability < 0.05
                      ? "text-emerald-400"
                      : result.ruin_probability < 0.2
                      ? "text-yellow-400"
                      : "text-red-400"
                  )}
                >
                  {result.ruin_probability < 0.05
                    ? "Estrategia robusta"
                    : result.ruin_probability < 0.2
                    ? "Riesgo moderado"
                    : "Riesgo elevado"}
                </p>
                <p className="text-xs text-muted-foreground">
                  {result.n_simulations.toLocaleString()} simulaciones ·{" "}
                  {result.p25_return_pct >= 0
                    ? "El 75% de los escenarios es positivo"
                    : "Más del 25% termina en pérdida"}
                </p>
              </div>
            </div>

            {allPass !== null && (
              <span
                className={cn(
                  "flex items-center gap-1.5 rounded-full px-4 py-2.5 text-sm font-bold",
                  allPass
                    ? "bg-emerald-500/15 text-emerald-400"
                    : "bg-red-500/15 text-red-400"
                )}
              >
                {allPass ? (
                  <CheckCircle2 className="h-4 w-4" />
                ) : (
                  <XCircle className="h-4 w-4" />
                )}
                {allPass ? "APROBADO" : "REPROBADO"}
              </span>
            )}
          </div>
        </CardContent>
      </Card>

      {/* Distribution charts */}
      <Card>
        <CardContent className="pt-4">
          <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
            {/* Return distribution */}
            <div>
              <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground/70">
                <TrendingUp className="h-4 w-4 text-primary" />
                Distribución de Retornos
              </h3>
              <div className="h-52">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={returnBars} margin={{ top: 4, right: 4, left: 0, bottom: 4 }}>
                    <CartesianGrid strokeDasharray="3 3" stroke="currentColor" className="opacity-10" />
                    <XAxis dataKey="label" tick={{ fontSize: 11, fill: "currentColor" }} className="text-muted-foreground" />
                    <YAxis tick={{ fontSize: 11, fill: "currentColor" }} className="text-muted-foreground" tickFormatter={(v) => `${v.toFixed(0)}%`} />
                    <Tooltip
                      formatter={(v: number) => [fmtPct(v), "Retorno"]}
                      contentStyle={TOOLTIP_STYLE}
                    />
                    <ReferenceLine y={0} stroke="hsl(var(--muted-foreground))" strokeDasharray="4 4" />
                    <Bar dataKey="value" radius={[4, 4, 0, 0]}>
                      {returnBars.map((b) => (
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

            {/* Drawdown distribution */}
            <div>
              <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-foreground/70">
                <TrendingDown className="h-4 w-4 text-destructive" />
                Drawdown Máximo Simulado
              </h3>
              <div className="h-52">
                <ResponsiveContainer width="100%" height="100%">
                  <BarChart data={ddBars} margin={{ top: 4, right: 4, left: 0, bottom: 4 }}>
                    <CartesianGrid strokeDasharray="3 3" stroke="currentColor" className="opacity-10" />
                    <XAxis dataKey="label" tick={{ fontSize: 11, fill: "currentColor" }} className="text-muted-foreground" />
                    <YAxis tick={{ fontSize: 11, fill: "currentColor" }} className="text-muted-foreground" tickFormatter={(v) => `${v.toFixed(0)}%`} />
                    <Tooltip
                      formatter={(v: number) => [`-${v.toFixed(2)}%`, "Max DD"]}
                      contentStyle={TOOLTIP_STYLE}
                    />
                    <Bar dataKey="value" radius={[4, 4, 0, 0]} fill="hsl(var(--destructive))" fillOpacity={0.8} />
                  </BarChart>
                </ResponsiveContainer>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Percentile table */}
      <Card>
        <CardHeader className="pb-2">
          <CardTitle>Tabla de Percentiles</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border/40 text-left text-xs text-muted-foreground">
                  <th className="pb-2 pr-6 font-medium">Percentil</th>
                  <th className="pb-2 pr-6 font-medium">Retorno</th>
                  <th className="pb-2 pr-6 font-medium">Max Drawdown</th>
                  <th className="pb-2 font-medium">Interpretación</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border/20">
                {[
                  { label: "P5 (peor 5%)", ret: result.p5_return_pct, dd: null, desc: "Escenario muy adverso" },
                  { label: "P25", ret: result.p25_return_pct, dd: result.p25_max_drawdown_pct, desc: "1 de cada 4 corre peor" },
                  { label: "P50 (mediana)", ret: result.median_return_pct, dd: result.median_max_drawdown_pct, desc: "Resultado más probable" },
                  { label: "P75", ret: result.p75_return_pct, dd: result.p75_max_drawdown_pct, desc: "1 de cada 4 corre mejor" },
                  { label: "P95 (mejor 5%)", ret: result.p95_return_pct, dd: result.p95_max_drawdown_pct, desc: "Escenario muy favorable" },
                ].map((row) => (
                  <tr key={row.label}>
                    <td className="py-2 pr-6 text-muted-foreground">{row.label}</td>
                    <td className={cn("py-2 pr-6 font-mono font-semibold", row.ret >= 0 ? "text-emerald-400" : "text-red-400")}>
                      {fmtPct(row.ret)}
                    </td>
                    <td className="py-2 pr-6 font-mono font-semibold text-red-400">
                      {row.dd !== null ? `-${fmt(row.dd)}%` : "–"}
                    </td>
                    <td className="py-2 text-xs text-muted-foreground">{row.desc}</td>
                  </tr>
                ))}
                <tr className="border-t border-border/30 bg-foreground/[0.02]">
                  <td className="py-2 pr-6 font-medium">Original</td>
                  <td className={cn("py-2 pr-6 font-mono font-semibold", result.original_return_pct >= 0 ? "text-blue-400" : "text-red-400")}>
                    {fmtPct(result.original_return_pct)}
                  </td>
                  <td className="py-2 pr-6 font-mono font-semibold text-blue-400">
                    -{fmt(result.original_max_drawdown_pct)}%
                  </td>
                  <td className="py-2 text-xs text-muted-foreground">Backtest histórico real</td>
                </tr>
              </tbody>
            </table>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

// ── main page ─────────────────────────────────────────────────────────────────

export function RobustezPage() {
  const backtestResults = useAppStore((s) => s.backtestResults);
  const savedStrategies = useAppStore((s) => s.savedStrategies);
  const currentStrategy = useAppStore((s) => s.currentStrategy);
  const initialCapital = useAppStore((s) => s.initialCapital);
  const setMonteCarloResults = useAppStore((s) => s.setMonteCarloResults);

  // ── method checkboxes ─────────────────────────────────────────────────────
  const [useResampling, setUseResampling] = useState(true);
  const [useSkipTrades, setUseSkipTrades] = useState(false);
  const [skipProbability, setSkipProbability] = useState(10);
  const [nSimulations, setNSimulations] = useState(1000);
  const [selectedStrategyId, setSelectedStrategyId] = useState<string>("__current__");

  // ── results (one per method) ──────────────────────────────────────────────
  const [resamplingResult, setResamplingResult] = useState<MonteCarloResult | null>(null);
  const [skipTradesResult, setSkipTradesResult] = useState<MonteCarloResult | null>(null);
  const [activeTab, setActiveTab] = useState<"Resampling" | "SkipTrades">("Resampling");

  // ── filters ───────────────────────────────────────────────────────────────
  const [filters, setFilters] = useState<MonteCarloFilter[]>([]);
  const filterIdRef = useRef(0);

  // ── run state ─────────────────────────────────────────────────────────────
  const [isRunning, setIsRunning] = useState(false);
  const [runningLabel, setRunningLabel] = useState("");
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef(false);

  // ── derived ───────────────────────────────────────────────────────────────
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
  const canRun = trades.length >= 5 && (useResampling || useSkipTrades);
  const hasBothResults = resamplingResult !== null && skipTradesResult !== null;
  const hasAnyResult = resamplingResult !== null || skipTradesResult !== null;

  // active result for the filter display
  const activeResult =
    activeTab === "Resampling" ? resamplingResult : skipTradesResult;

  // ── filter helpers ────────────────────────────────────────────────────────
  const addFilter = () => {
    filterIdRef.current += 1;
    setFilters((prev) => [
      ...prev,
      {
        id: String(filterIdRef.current),
        metric: "net_return",
        percentile: 25,
        comparison: ">",
        threshold_type: "absolute",
        threshold_value: 0,
      },
    ]);
  };

  // ── run handler ───────────────────────────────────────────────────────────
  const handleRun = async () => {
    if (!canRun || isRunning) return;
    setIsRunning(true);
    setError(null);
    abortRef.current = false;
    setResamplingResult(null);
    setSkipTradesResult(null);

    try {
      if (useResampling && !abortRef.current) {
        setRunningLabel("Resampling…");
        const config: MonteCarloConfig = {
          n_simulations: nSimulations,
          method: "Resampling",
          skip_probability: 0,
        };
        const r = await runMonteCarlo(trades, initialCapital, config);
        if (!abortRef.current) {
          setResamplingResult(r);
          setMonteCarloResults(r);
          setActiveTab("Resampling");
        }
      }

      if (useSkipTrades && !abortRef.current) {
        setRunningLabel(`Skip Trades ${skipProbability}%…`);
        const config: MonteCarloConfig = {
          n_simulations: nSimulations,
          method: "SkipTrades",
          skip_probability: skipProbability / 100,
        };
        const r = await runMonteCarlo(trades, initialCapital, config);
        if (!abortRef.current) {
          setSkipTradesResult(r);
          if (!useResampling) {
            setMonteCarloResults(r);
            setActiveTab("SkipTrades");
          }
        }
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setIsRunning(false);
      setRunningLabel("");
    }
  };

  // ── render ────────────────────────────────────────────────────────────────
  return (
    <div className="mx-auto max-w-[1400px] space-y-4">

      {/* ── Config card ── */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <Shield className="h-4 w-4 text-primary" />
            <CardTitle className="text-sm font-semibold text-foreground">
              Simulación Monte Carlo
            </CardTitle>
          </div>
        </CardHeader>
        <CardContent className="space-y-5">

          {/* Row 1: strategy + simulations */}
          <div className="grid grid-cols-2 gap-4 sm:grid-cols-3">
            <div className="flex flex-col gap-1.5">
              <label className="text-xs font-medium text-muted-foreground">Estrategia</label>
              <select
                value={selectedStrategyId}
                onChange={(e) => setSelectedStrategyId(e.target.value)}
                className="rounded-md border border-border bg-background px-3 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-primary"
              >
                {strategyOptions.map((opt) => (
                  <option key={opt.id} value={opt.id}>{opt.label}</option>
                ))}
              </select>
              <span className="text-[11px] text-muted-foreground">
                Trades del último backtest
              </span>
            </div>

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
              <span className="text-[11px] text-muted-foreground">
                {trades.length} trades disponibles
              </span>
            </div>
          </div>

          {/* Row 2: methods */}
          <div>
            <label className="mb-2 block text-xs font-medium text-muted-foreground">
              Métodos de simulación
            </label>
            <div className="space-y-2">
              {/* Resampling */}
              <label
                className={cn(
                  "flex cursor-pointer items-center gap-3 rounded-lg border px-4 py-3 transition-colors",
                  useResampling
                    ? "border-primary/40 bg-primary/5"
                    : "border-border/40 bg-background hover:bg-foreground/[0.02]"
                )}
              >
                <input
                  type="checkbox"
                  checked={useResampling}
                  onChange={(e) => setUseResampling(e.target.checked)}
                  className="accent-primary h-4 w-4"
                />
                <Shuffle className={cn("h-4 w-4 shrink-0", useResampling ? "text-primary" : "text-muted-foreground")} />
                <div>
                  <p className="text-sm font-medium">Resampling</p>
                  <p className="text-[11px] text-muted-foreground">
                    Elige trades al azar con repetición — evalúa si el edge es estadísticamente estable.
                  </p>
                </div>
              </label>

              {/* Skip Trades */}
              <label
                className={cn(
                  "flex cursor-pointer items-center gap-3 rounded-lg border px-4 py-3 transition-colors",
                  useSkipTrades
                    ? "border-primary/40 bg-primary/5"
                    : "border-border/40 bg-background hover:bg-foreground/[0.02]"
                )}
              >
                <input
                  type="checkbox"
                  checked={useSkipTrades}
                  onChange={(e) => setUseSkipTrades(e.target.checked)}
                  className="accent-primary h-4 w-4"
                />
                <SkipForward className={cn("h-4 w-4 shrink-0", useSkipTrades ? "text-primary" : "text-muted-foreground")} />
                <div className="flex-1">
                  <p className="text-sm font-medium">Skip Trades</p>
                  <p className="text-[11px] text-muted-foreground">
                    Salta trades aleatoriamente — modela errores de ejecución o filtros selectivos.
                  </p>
                </div>
                {useSkipTrades && (
                  <div
                    className="ml-auto flex shrink-0 items-center gap-3"
                    onClick={(e) => e.preventDefault()}
                  >
                    <span className="text-xs text-muted-foreground">Prob. de skip:</span>
                    <input
                      type="range"
                      min={1}
                      max={50}
                      step={1}
                      value={skipProbability}
                      onChange={(e) => setSkipProbability(Number(e.target.value))}
                      className="w-28 accent-primary"
                    />
                    <span className="w-10 font-mono text-sm font-bold">{skipProbability}%</span>
                  </div>
                )}
              </label>
            </div>
          </div>

          {/* Row 3: filters */}
          <div>
            <div className="mb-2 flex items-center gap-3">
              <label className="text-xs font-medium text-muted-foreground">
                Filtros de aprobación
              </label>
              {hasAnyResult && filters.length > 0 && (
                <span
                  className={cn(
                    "flex items-center gap-1 rounded-full px-2.5 py-0.5 text-xs font-bold",
                    filters.every((f) => activeResult && evaluateFilter(f, activeResult))
                      ? "bg-emerald-500/15 text-emerald-400"
                      : "bg-red-500/15 text-red-400"
                  )}
                >
                  {filters.every((f) => activeResult && evaluateFilter(f, activeResult)) ? (
                    <><CheckCircle2 className="h-3 w-3" /> APROBADO</>
                  ) : (
                    <><XCircle className="h-3 w-3" /> REPROBADO</>
                  )}
                </span>
              )}
            </div>

            {filters.length === 0 && (
              <p className="text-xs italic text-muted-foreground">Sin filtros. Agregá criterios mínimos para evaluar si la estrategia pasa el test.</p>
            )}

            <div className="space-y-2">
              {filters.map((f) => (
                <FilterRow
                  key={f.id}
                  filter={f}
                  result={activeResult ?? null}
                  onChange={(upd) =>
                    setFilters((prev) => prev.map((x) => (x.id === f.id ? upd : x)))
                  }
                  onRemove={() =>
                    setFilters((prev) => prev.filter((x) => x.id !== f.id))
                  }
                />
              ))}
            </div>

            <button
              onClick={addFilter}
              className="mt-2 flex items-center gap-1.5 rounded-md border border-dashed border-border/60 px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:border-primary/50 hover:text-primary"
            >
              <Plus className="h-3.5 w-3.5" />
              Agregar filtro
            </button>
          </div>

          {/* Warnings */}
          {!canRun && trades.length < 5 && (
            <div className="flex items-center gap-2 rounded-md border border-yellow-500/30 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-400">
              <AlertTriangle className="h-4 w-4 shrink-0" />
              Ejecutá un backtest primero. Se necesitan al menos 5 trades.
            </div>
          )}
          {!canRun && trades.length >= 5 && !useResampling && !useSkipTrades && (
            <div className="flex items-center gap-2 rounded-md border border-yellow-500/30 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-400">
              <AlertTriangle className="h-4 w-4 shrink-0" />
              Seleccioná al menos un método de simulación.
            </div>
          )}

          {/* Run button */}
          <div className="flex items-center gap-3">
            <Button
              onClick={handleRun}
              disabled={!canRun || isRunning}
              size="sm"
            >
              {isRunning ? (
                <>
                  <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
                  {runningLabel || "Simulando…"}
                </>
              ) : (
                <>
                  <Play className="mr-1.5 h-4 w-4" />
                  Ejecutar Monte Carlo
                  {useResampling && useSkipTrades && " (2 métodos)"}
                </>
              )}
            </Button>
            {isRunning && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => { abortRef.current = true; setIsRunning(false); }}
              >
                Cancelar
              </Button>
            )}
          </div>

          {error && (
            <div className="flex items-start gap-2 rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />
              {error}
            </div>
          )}
        </CardContent>
      </Card>

      {/* ── Empty state ── */}
      {!hasAnyResult && !isRunning && (
        <div className="py-14 text-center">
          <BarChart3 className="mx-auto mb-3 h-10 w-10 text-muted-foreground/30" />
          <p className="text-sm text-muted-foreground">
            Configurá la simulación y presioná <strong>Ejecutar Monte Carlo</strong>
          </p>
        </div>
      )}

      {/* ── Results ── */}
      {hasAnyResult && (
        hasBothResults ? (
          /* Both methods ran → show tabs */
          <Tabs
            value={activeTab}
            onValueChange={(v) => setActiveTab(v as "Resampling" | "SkipTrades")}
          >
            <TabsList className="mb-1">
              <TabsTrigger value="Resampling">
                <Shuffle className="mr-1.5 h-3.5 w-3.5" />
                Resampling
              </TabsTrigger>
              <TabsTrigger value="SkipTrades">
                <SkipForward className="mr-1.5 h-3.5 w-3.5" />
                Skip Trades {skipProbability}%
              </TabsTrigger>
            </TabsList>

            <TabsContent value="Resampling">
              {resamplingResult && (
                <ResultView
                  result={resamplingResult}
                  filters={filters}
                  initialCapital={initialCapital}
                  methodLabel="Resampling"
                />
              )}
            </TabsContent>

            <TabsContent value="SkipTrades">
              {skipTradesResult && (
                <ResultView
                  result={skipTradesResult}
                  filters={filters}
                  initialCapital={initialCapital}
                  methodLabel={`Skip Trades ${skipProbability}%`}
                />
              )}
            </TabsContent>
          </Tabs>
        ) : (
          /* Single method */
          <ResultView
            result={(resamplingResult ?? skipTradesResult)!}
            filters={filters}
            initialCapital={initialCapital}
            methodLabel={
              resamplingResult ? "Resampling" : `Skip Trades ${skipProbability}%`
            }
          />
        )
      )}
    </div>
  );
}
