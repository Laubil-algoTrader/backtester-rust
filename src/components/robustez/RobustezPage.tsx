import { useState } from "react";
import {
  Shield,
  Play,
  Loader2,
  AlertTriangle,
  Shuffle,
  SkipForward,
  FlaskConical,
  Lock,
} from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { useAppStore } from "@/stores/useAppStore";
import { runMonteCarlo } from "@/lib/tauri";
import type { MonteCarloResult, MonteCarloConfig } from "@/lib/types";
import { cn } from "@/lib/utils";

// ── Formatters ────────────────────────────────────────────────────────────────

function fmtMoney(v: number, decimals = 2): string {
  const abs = Math.abs(v);
  const sign = v < 0 ? "-" : "";
  if (abs >= 1_000_000) return `${sign}$${(abs / 1e6).toFixed(2)}M`;
  if (abs >= 1_000) return `${sign}$${(abs / 1_000).toFixed(0)}k`;
  return `${sign}$${abs.toFixed(decimals)}`;
}

function fmtRatio(v: number): string {
  return v.toFixed(2);
}

function fmtExpectancy(v: number): string {
  return fmtMoney(v, 2);
}

function riskColor(frac: number): string {
  if (frac < 0.05) return "text-emerald-400";
  if (frac < 0.2) return "text-yellow-400";
  return "text-red-400";
}

// ── EquityFanChart ────────────────────────────────────────────────────────────

const VB_W = 1000;
const VB_H = 340;
const PAD = { top: 16, right: 24, bottom: 32, left: 66 };
const IW = VB_W - PAD.left - PAD.right;
const IH = VB_H - PAD.top - PAD.bottom;

/**
 * Map a normalised rank (0 = worst outcome, 1 = best outcome) to an HSL color.
 * Spectrum: red → orange → yellow → lime → green  (mirrors SQX fan chart palette).
 */
function rankToColor(rank: number): string {
  const hue = rank * 128;                           // 0 = red, 128 = green
  const sat = 72 + 18 * Math.sin(Math.PI * rank);  // more vivid in the middle
  const lgt = 46 + 10 * Math.sin(Math.PI * rank);  // slightly brighter mid-range
  return `hsl(${hue.toFixed(0)},${sat.toFixed(0)}%,${lgt.toFixed(0)}%)`;
}

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

  // ── bounds ──────────────────────────────────────────────────────────────────
  let minV = initialCapital;
  let maxV = initialCapital;
  for (const curve of [...simCurves, originalCurve]) {
    for (const v of curve) {
      if (v < minV) minV = v;
      if (v > maxV) maxV = v;
    }
  }
  const padV = (maxV - minV || 1) * 0.06;
  const lo = minV - padV;
  const hi = maxV + padV;
  const span = hi - lo;

  const toX = (i: number, total: number) =>
    PAD.left + (i / Math.max(total - 1, 1)) * IW;
  const toY = (v: number) =>
    PAD.top + IH - ((v - lo) / span) * IH;
  const makePath = (curve: number[]) =>
    curve
      .map((v, i) => `${i === 0 ? "M" : "L"}${toX(i, curve.length).toFixed(1)},${toY(v).toFixed(1)}`)
      .join(" ");

  // ── Sort curves by final equity: worst drawn first (below), best last (on top) ──
  const sorted = [...simCurves]
    .map((c) => ({ curve: c, final: c[c.length - 1] ?? initialCapital }))
    .sort((a, b) => a.final - b.final);

  const worstFinal = sorted[0]?.final ?? initialCapital;
  const bestFinal  = sorted[sorted.length - 1]?.final ?? initialCapital;
  const finalSpan  = bestFinal - worstFinal || 1;

  // With many curves each should be slightly transparent to reveal density.
  const opacity = simCurves.length > 150 ? 0.30 : simCurves.length > 80 ? 0.38 : 0.48;

  // ── Grid ────────────────────────────────────────────────────────────────────
  const yTicks = Array.from({ length: 5 }, (_, i) => lo + (span / 4) * i);
  const numPts = Math.max(originalCurve.length, simCurves[0]?.length ?? 1);
  const xTicks = Array.from({ length: 7 }, (_, i) => Math.round((i / 6) * (numPts - 1)));

  const fmtM = (v: number) => {
    const abs = Math.abs(v);
    const s = v < 0 ? "-" : "";
    if (abs >= 1_000_000) return `${s}$${(abs / 1e6).toFixed(1)}M`;
    if (abs >= 1_000)     return `${s}$${(abs / 1_000).toFixed(0)}k`;
    return `${s}$${abs.toFixed(0)}`;
  };

  return (
    <svg
      viewBox={`0 0 ${VB_W} ${VB_H}`}
      preserveAspectRatio="none"
      style={{ width: "100%", height: "100%", display: "block" }}
    >
      {/* Horizontal grid + Y axis labels */}
      {yTicks.map((v, i) => {
        const y = toY(v);
        return (
          <g key={i}>
            <line x1={PAD.left} y1={y} x2={VB_W - PAD.right} y2={y}
              stroke="rgba(255,255,255,0.07)" strokeWidth="0.7" />
            <text x={PAD.left - 6} y={y + 4} textAnchor="end" fontSize="10"
              fill="rgba(255,255,255,0.42)" fontFamily="ui-monospace,monospace">
              {fmtM(v)}
            </text>
          </g>
        );
      })}

      {/* X axis labels */}
      {xTicks.map((idx) => (
        <text key={idx} x={toX(idx, numPts)} y={VB_H - 5}
          textAnchor="middle" fontSize="9" fill="rgba(255,255,255,0.30)"
          fontFamily="ui-monospace,monospace">
          {idx}
        </text>
      ))}

      {/* Initial-capital baseline */}
      <line
        x1={PAD.left} y1={toY(initialCapital)}
        x2={VB_W - PAD.right} y2={toY(initialCapital)}
        stroke="rgba(255,255,255,0.25)" strokeWidth="1" strokeDasharray="7,5"
      />

      {/* ── Simulation curves: worst → best, colored by rank ── */}
      {sorted.map(({ curve, final }, idx) => {
        const rank = (final - worstFinal) / finalSpan;
        return (
          <path
            key={idx}
            d={makePath(curve)}
            fill="none"
            stroke={rankToColor(rank)}
            strokeOpacity={opacity}
            strokeWidth="0.8"
          />
        );
      })}

      {/* Original equity curve — bright blue, always on top */}
      <path
        d={makePath(originalCurve)}
        fill="none"
        stroke="#3b82f6"
        strokeWidth="2.6"
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}

// ── Confidence Table ──────────────────────────────────────────────────────────

const HIGHLIGHTED_LEVEL = 95;

function ConfidenceTable({
  result,
}: {
  result: MonteCarloResult;
}) {
  const headCls = "px-3 py-2 text-left text-[11px] font-semibold uppercase tracking-wider text-muted-foreground";
  const cellCls = "px-3 py-2 font-mono text-sm tabular-nums";

  return (
    <div className="overflow-hidden rounded-lg border border-border/40">
      <table className="w-full border-collapse text-sm">
        <thead>
          <tr className="border-b border-border/40 bg-card/60">
            <th className={headCls}>Confidence</th>
            <th className={cn(headCls, "text-right")}>Net Profit</th>
            <th className={cn(headCls, "text-right")}>Drawdown</th>
            <th className={cn(headCls, "text-right")}>Ret/DD</th>
            <th className={cn(headCls, "text-right")}>Expectancy</th>
          </tr>
        </thead>
        <tbody>
          {/* Original row */}
          <tr className="border-b border-border/40 bg-white/[0.03]">
            <td className={cn(cellCls, "font-semibold text-white/80")}>Original</td>
            <td className={cn(cellCls, "text-right", result.original_net_profit >= 0 ? "text-emerald-400" : "text-red-400")}>
              {fmtMoney(result.original_net_profit)}
            </td>
            <td className={cn(cellCls, "text-right text-orange-300")}>
              {fmtMoney(result.original_max_drawdown_abs)}
            </td>
            <td className={cn(cellCls, "text-right text-white/70")}>
              {fmtRatio(result.original_ret_dd_ratio)}
            </td>
            <td className={cn(cellCls, "text-right", result.original_expectancy >= 0 ? "text-emerald-400/80" : "text-red-400/80")}>
              {fmtExpectancy(result.original_expectancy)}
            </td>
          </tr>

          {/* Confidence level rows */}
          {result.confidence_table.map((row) => {
            const isHighlighted = row.level === HIGHLIGHTED_LEVEL;
            return (
              <tr
                key={row.level}
                className={cn(
                  "border-b border-border/20 transition-colors",
                  isHighlighted
                    ? "bg-blue-500/[0.12] outline outline-1 outline-blue-500/30"
                    : "hover:bg-white/[0.02]"
                )}
              >
                <td className={cn(
                  cellCls,
                  isHighlighted ? "font-bold text-blue-300" : "text-muted-foreground"
                )}>
                  {row.level}%
                  {isHighlighted && (
                    <span className="ml-2 text-[10px] font-normal text-blue-400/70">← recomendado</span>
                  )}
                </td>
                <td className={cn(
                  cellCls, "text-right",
                  row.net_profit >= 0 ? "text-emerald-400/80" : "text-red-400/80",
                  isHighlighted && "font-semibold"
                )}>
                  {fmtMoney(row.net_profit)}
                </td>
                <td className={cn(
                  cellCls, "text-right text-orange-300/70",
                  isHighlighted && "font-semibold text-orange-300"
                )}>
                  {fmtMoney(row.max_drawdown_abs)}
                </td>
                <td className={cn(
                  cellCls, "text-right text-white/50",
                  isHighlighted && "font-semibold text-white/80"
                )}>
                  {fmtRatio(row.ret_dd_ratio)}
                </td>
                <td className={cn(
                  cellCls, "text-right",
                  row.expectancy >= 0 ? "text-emerald-400/60" : "text-red-400/60",
                  isHighlighted && "font-semibold opacity-100"
                )}>
                  {fmtExpectancy(row.expectancy)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// ── Main Page ─────────────────────────────────────────────────────────────────

type MCTab = "manipulation" | "retest";

const N_SIM_OPTIONS = [100, 250, 500, 1000, 2000];

export function RobustezPage() {
  const lastBacktest = useAppStore((s) => s.backtestResults);
  const symbols = useAppStore((s) => s.symbols);
  const selectedSymbolId = useAppStore((s) => s.selectedSymbolId);
  const selectedSymbol = symbols.find((s) => s.id === selectedSymbolId) ?? null;

  // MC type tab
  const [mcTab, setMcTab] = useState<MCTab>("manipulation");

  // Config state
  const [nSimulations, setNSimulations] = useState(500);
  const [useResampling, setUseResampling] = useState(true);
  const [useSkipTrades, setUseSkipTrades] = useState(true);
  const [skipPct, setSkipPct] = useState(10); // % (0-50)
  const [ruinThresholdPct, setRuinThresholdPct] = useState(20); // % loss = ruin (5-80)

  // Result state
  const [result, setResult] = useState<MonteCarloResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Remember last-run config for the "methods used" footer
  const [lastConfig, setLastConfig] = useState<{
    useResampling: boolean;
    useSkipTrades: boolean;
    skipPct: number;
    ruinThresholdPct: number;
    nSim: number;
  } | null>(null);

  const canRun =
    !loading &&
    (useResampling || useSkipTrades) &&
    !!lastBacktest &&
    lastBacktest.trades.length > 0;

  // Source of truth: the config used for the backtest that generated these trades.
  // Fall back to the store's current capital if backtest_config is not yet populated.
  const storeCapital = useAppStore((s) => s.initialCapital);
  const derivedInitialCapital = lastBacktest?.backtest_config?.initial_capital ?? storeCapital;

  async function handleRun() {
    if (!lastBacktest) return;
    setLoading(true);
    setError(null);
    setResult(null);

    const config: MonteCarloConfig = {
      n_simulations: nSimulations,
      use_resampling: useResampling,
      use_skip_trades: useSkipTrades,
      skip_probability: skipPct / 100,
      ruin_threshold_pct: ruinThresholdPct,
    };

    try {
      const res = await runMonteCarlo(
        lastBacktest.trades,
        derivedInitialCapital,
        config
      );
      setResult(res);
      setLastConfig({ useResampling, useSkipTrades, skipPct, ruinThresholdPct, nSim: nSimulations });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }

  const noData = !lastBacktest || lastBacktest.trades.length === 0;


  return (
    <div className="mx-auto max-w-[1400px] space-y-4 p-6">
      {/* Header */}
      <div className="flex items-center gap-3">
        <Shield className="h-6 w-6 text-primary" />
        <div>
          <h1 className="text-xl font-semibold">Análisis de Robustez</h1>
          <p className="text-sm text-muted-foreground">
            Stress-test de la estrategia mediante simulaciones Monte Carlo
          </p>
        </div>
      </div>

      {/* MC type tabs */}
      <div className="flex gap-1 rounded-lg border border-border/40 bg-card/40 p-1 w-fit">
        <button
          onClick={() => setMcTab("manipulation")}
          className={cn(
            "flex items-center gap-2 rounded-md px-4 py-2 text-sm font-medium transition-colors",
            mcTab === "manipulation"
              ? "bg-primary text-primary-foreground shadow"
              : "text-muted-foreground hover:text-foreground"
          )}
        >
          <Shuffle className="h-4 w-4" />
          Trade Manipulation
        </button>
        <button
          disabled
          className="flex items-center gap-2 rounded-md px-4 py-2 text-sm font-medium text-muted-foreground/40 cursor-not-allowed"
        >
          <FlaskConical className="h-4 w-4" />
          Retest (parámetros / spread)
          <Lock className="h-3 w-3 ml-1" />
        </button>
      </div>

      {noData && (
        <div className="flex items-center gap-2 rounded-lg border border-yellow-500/30 bg-yellow-500/5 p-4 text-sm text-yellow-300">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          Primero ejecutá un backtest para poder correr el Monte Carlo.
        </div>
      )}

      {/* Config card */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">Configuración</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Top row: simulations + run button */}
          <div className="flex flex-wrap items-center gap-6">
            <div className="flex items-center gap-3">
              <label className="text-sm text-muted-foreground whitespace-nowrap">
                Número de simulaciones
              </label>
              <select
                value={nSimulations}
                onChange={(e) => setNSimulations(Number(e.target.value))}
                disabled={loading}
                className="rounded border border-border bg-background px-3 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-primary"
              >
                {N_SIM_OPTIONS.map((n) => (
                  <option key={n} value={n}>{n.toLocaleString()}</option>
                ))}
              </select>
            </div>

            <Button
              onClick={handleRun}
              disabled={!canRun}
              className="ml-auto"
            >
              {loading ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Simulando…
                </>
              ) : (
                <>
                  <Play className="mr-2 h-4 w-4" />
                  Correr Monte Carlo
                </>
              )}
            </Button>
          </div>

          {/* Method checkboxes */}
          <div className="space-y-2">
            {/* Resampling */}
            <label className={cn(
              "flex items-center gap-3 rounded-md border px-3 py-2.5 cursor-pointer transition-colors",
              useResampling
                ? "border-primary/40 bg-primary/5"
                : "border-border/40 bg-card/40 hover:border-border"
            )}>
              <input
                type="checkbox"
                checked={useResampling}
                onChange={(e) => setUseResampling(e.target.checked)}
                className="h-4 w-4 rounded accent-primary"
                disabled={loading}
              />
              <Shuffle className="h-4 w-4 text-muted-foreground shrink-0" />
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium">Randomize trades order, with method Resampling</p>
                <p className="text-xs text-muted-foreground">
                  Bootstrap con reemplazo — cada simulación sortea N trades del pool con repetición
                </p>
              </div>
            </label>

            {/* Skip Trades */}
            <label className={cn(
              "flex items-center gap-3 rounded-md border px-3 py-2.5 cursor-pointer transition-colors",
              useSkipTrades
                ? "border-primary/40 bg-primary/5"
                : "border-border/40 bg-card/40 hover:border-border"
            )}>
              <input
                type="checkbox"
                checked={useSkipTrades}
                onChange={(e) => setUseSkipTrades(e.target.checked)}
                className="h-4 w-4 rounded accent-primary"
                disabled={loading}
              />
              <SkipForward className="h-4 w-4 text-muted-foreground shrink-0" />
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium">
                  Randomly skip trades, with probability{" "}
                  <span className="text-primary font-semibold">{skipPct} %</span>
                </p>
                <p className="text-xs text-muted-foreground">
                  Simula trades perdidos por fallo de plataforma o internet
                </p>
              </div>
              {/* Slider inline */}
              {useSkipTrades && (
                <div className="flex items-center gap-2 ml-2 shrink-0" onClick={(e) => e.preventDefault()}>
                  <input
                    type="range"
                    min={1}
                    max={50}
                    value={skipPct}
                    onChange={(e) => setSkipPct(Number(e.target.value))}
                    disabled={loading}
                    className="w-28 accent-primary"
                  />
                  <span className="w-8 text-right text-xs tabular-nums text-muted-foreground">
                    {skipPct}%
                  </span>
                </div>
              )}
            </label>
          </div>

          {/* Ruin threshold */}
          {(() => {
            const ruinLevelDollar = derivedInitialCapital * (1 - ruinThresholdPct / 100);
            const ruinLossDollar  = derivedInitialCapital * ruinThresholdPct / 100;
            const fmtDollar = (v: number) => {
              if (v >= 1_000_000) return `$${(v / 1e6).toFixed(2)}M`;
              if (v >= 1_000)     return `$${(v / 1_000).toFixed(1)}k`;
              return `$${v.toFixed(0)}`;
            };
            return (
              <div className="flex items-center gap-3 rounded-md border border-border/40 bg-card/40 px-3 py-2.5">
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium">
                    Umbral de ruina:{" "}
                    <span className="text-red-400 font-semibold">{ruinThresholdPct}% de pérdida</span>
                    <span className="ml-2 text-xs font-normal text-muted-foreground">
                      (−{fmtDollar(ruinLossDollar)} → equity ≤ {fmtDollar(ruinLevelDollar)})
                    </span>
                  </p>
                  <p className="text-xs text-muted-foreground">
                    Una simulación se considera "ruinada" si el equity cae por debajo de{" "}
                    <span className="text-red-400/80">{fmtDollar(ruinLevelDollar)}</span>
                  </p>
                </div>
                <div className="flex items-center gap-2 shrink-0">
                  <input
                    type="range"
                    min={1}
                    max={80}
                    step={1}
                    value={ruinThresholdPct}
                    onChange={(e) => setRuinThresholdPct(Number(e.target.value))}
                    disabled={loading}
                    className="w-28 accent-red-500"
                  />
                  <span className="w-10 text-right text-xs tabular-nums text-muted-foreground">
                    -{ruinThresholdPct}%
                  </span>
                </div>
              </div>
            );
          })()}

          {/* Combined note */}
          {useResampling && useSkipTrades && (
            <p className="text-xs text-muted-foreground/70 pl-1">
              ✓ Ambos activos: cada simulación aplica primero Resampling y luego Skip Trades —
              comportamiento idéntico a StrategyQuant X con ambos métodos marcados.
            </p>
          )}
        </CardContent>
      </Card>

      {/* Error */}
      {error && (
        <div className="flex items-center gap-2 rounded-lg border border-red-500/30 bg-red-500/5 p-4 text-sm text-red-300">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          {error}
        </div>
      )}

      {/* Results */}
      {result && (
        <>
          {/* Table + Chart side by side */}
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(500px,auto)_1fr]">
            {/* Confidence Table */}
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="text-base">Tabla de Confianza</CardTitle>
                <p className="text-xs text-muted-foreground">
                  C% confianza = solo (100−C)% de chance de que los resultados sean peores
                </p>
              </CardHeader>
              <CardContent className="p-0 pb-4">
                <ConfidenceTable result={result} />

                {/* Ruin probability */}
                <div className="mt-4 flex flex-wrap items-center gap-2 px-4">
                  <span className="text-xs text-muted-foreground">Probabilidad de ruina:</span>
                  <span className={cn("text-sm font-semibold", riskColor(result.ruin_probability))}>
                    {(result.ruin_probability * 100).toFixed(1)}%
                  </span>
                  <span className="text-xs text-muted-foreground">
                    ({Math.round(result.ruin_probability * result.n_simulations)} / {result.n_simulations} simulaciones)
                  </span>
                  {lastConfig && (
                    <span className="text-xs text-muted-foreground/60">
                      · umbral −{lastConfig.ruinThresholdPct}% (equity ≤ {
                        fmtMoney(derivedInitialCapital * (1 - lastConfig.ruinThresholdPct / 100), 0)
                      })
                    </span>
                  )}
                </div>
              </CardContent>
            </Card>

            {/* Equity Fan Chart */}
            <Card>
              <CardHeader className="pb-3">
                <CardTitle className="text-base">Equity Curves</CardTitle>
                <p className="text-xs text-muted-foreground">
                  {result.sim_equity_curves.length} simulaciones —{" "}
                  <span className="text-blue-400">azul = estrategia original</span>
                </p>
              </CardHeader>
              <CardContent className="p-2">
                <div style={{ height: 340 }}>
                  <EquityFanChart
                    simCurves={result.sim_equity_curves}
                    originalCurve={result.original_equity_curve}
                    initialCapital={derivedInitialCapital}
                  />
                </div>
              </CardContent>
            </Card>
          </div>

          {/* Simulation methods used footer */}
          {lastConfig && (
            <Card className="border-border/30 bg-card/30">
              <CardContent className="py-3 px-4">
                <p className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                  Simulation methods used
                </p>
                <div className="space-y-0.5 text-xs text-muted-foreground">
                  {selectedSymbol && (
                    <p>{selectedSymbol.name}</p>
                  )}
                  <p>Simulations: {result.n_simulations.toLocaleString()}</p>
                  {lastConfig.useResampling && (
                    <p>Randomize trades order, with method Resampling</p>
                  )}
                  {lastConfig.useSkipTrades && (
                    <p>Randomly skip trades, with probability {lastConfig.skipPct} %</p>
                  )}
                  <p>Ruin threshold: -{lastConfig.ruinThresholdPct}% of initial capital</p>
                </div>
              </CardContent>
            </Card>
          )}
        </>
      )}
    </div>
  );
}
