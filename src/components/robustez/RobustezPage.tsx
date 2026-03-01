import { useState, useRef } from "react";
import {
  Shield,
  Play,
  Loader2,
  AlertTriangle,
  TrendingUp,
  TrendingDown,
  Percent,
  Activity,
  BarChart3,
  ChevronDown,
  ChevronUp,
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
import type { MonteCarloResult, Strategy } from "@/lib/types";
import { cn } from "@/lib/utils";

// ── helpers ──────────────────────────────────────────────────────────────────

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

// ── sub-components ────────────────────────────────────────────────────────────

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
    { label: "Mediana DD", value: result.median_max_drawdown_pct },
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
            <Bar dataKey="value" radius={[4, 4, 0, 0]} fill="hsl(var(--destructive))" fillOpacity={0.8} />
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}

// ── main page ────────────────────────────────────────────────────────────────

export function RobustezPage() {
  const backtestResults = useAppStore((s) => s.backtestResults);
  const savedStrategies = useAppStore((s) => s.savedStrategies);
  const currentStrategy = useAppStore((s) => s.currentStrategy);
  const initialCapital = useAppStore((s) => s.initialCapital);
  const monteCarloResults = useAppStore((s) => s.monteCarloResults);
  const setMonteCarloResults = useAppStore((s) => s.setMonteCarloResults);

  const [nSimulations, setNSimulations] = useState(1000);
  const [isRunning, setIsRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selectedStrategyId, setSelectedStrategyId] = useState<string>("__current__");
  const [showAdvanced, setShowAdvanced] = useState(false);

  const abortRef = useRef(false);

  // Compose the list of available strategies
  const strategyOptions: { id: string; label: string; strategy?: Strategy }[] = [
    {
      id: "__current__",
      label: `Estrategia actual${currentStrategy.name ? `: ${currentStrategy.name}` : ""}`,
    },
    ...savedStrategies.map((s) => ({ id: s.id, label: s.name, strategy: s })),
  ];

  // Active strategy object (current or selected saved)
  const activeStrategy =
    selectedStrategyId === "__current__"
      ? currentStrategy
      : savedStrategies.find((s) => s.id === selectedStrategyId);

  // Trades source: use backtest results if available, else nothing
  const trades = backtestResults?.trades ?? [];

  const canRun = trades.length >= 5;

  const handleRun = async () => {
    if (!canRun || isRunning) return;
    setIsRunning(true);
    setError(null);
    abortRef.current = false;

    try {
      const result = await runMonteCarlo(trades, initialCapital, nSimulations);
      if (!abortRef.current) {
        setMonteCarloResults(result);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setIsRunning(false);
    }
  };

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

        {/* Strategy selector */}
        <div className="grid grid-cols-2 gap-4">
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
              Los trades se toman del último backtest ejecutado
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
              onChange={(e) => setNSimulations(Math.max(100, Math.min(100000, Number(e.target.value))))}
              className="rounded-md border border-border bg-background px-3 py-1.5 text-sm focus:outline-none focus:ring-1 focus:ring-primary"
            />
            <span className="text-[11px] text-muted-foreground">100 – 100,000 simulaciones</span>
          </div>
        </div>

        {/* Advanced toggle */}
        <button
          onClick={() => setShowAdvanced((v) => !v)}
          className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
        >
          {showAdvanced ? <ChevronUp className="h-3 w-3" /> : <ChevronDown className="h-3 w-3" />}
          Detalles del dataset
        </button>
        {showAdvanced && (
          <div className="rounded-md border border-border/30 bg-background/50 p-3 text-xs text-muted-foreground space-y-1">
            <div>
              Trades disponibles:{" "}
              <span className="font-semibold text-foreground">{trades.length}</span>
            </div>
            <div>
              Capital inicial:{" "}
              <span className="font-semibold text-foreground">${initialCapital.toLocaleString()}</span>
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
              onClick={() => { abortRef.current = true; setIsRunning(false); }}
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

      {/* ── Results ── */}
      {monteCarloResults && (
        <div className="space-y-4">
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
              label="Probabilidad de Ruina"
              value={`${(monteCarloResults.ruin_probability * 100).toFixed(1)}%`}
              valueClass={riskColor(monteCarloResults.ruin_probability)}
              sub="Capital < inversión inicial"
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
                {monteCarloResults.n_simulations.toLocaleString()} simulaciones completadas ·{" "}
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
              <BarChart3 className="h-4 w-4 text-primary" />
              Tabla de Percentiles
            </h3>
            <div className="overflow-x-auto">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-border/40 text-left text-xs text-muted-foreground">
                    <th className="pb-2 pr-4 font-medium">Percentil</th>
                    <th className="pb-2 pr-4 font-medium">Retorno</th>
                    <th className="pb-2 font-medium">Interpretación</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/20">
                  {[
                    {
                      label: "P5 (peor 5%)",
                      value: monteCarloResults.p5_return_pct,
                      desc: "Escenario muy adverso",
                    },
                    {
                      label: "P25",
                      value: monteCarloResults.p25_return_pct,
                      desc: "1 de cada 4 corre peor",
                    },
                    {
                      label: "P50 (mediana)",
                      value: monteCarloResults.median_return_pct,
                      desc: "Resultado más probable",
                    },
                    {
                      label: "P75",
                      value: monteCarloResults.p75_return_pct,
                      desc: "1 de cada 4 corre mejor",
                    },
                    {
                      label: "P95 (mejor 5%)",
                      value: monteCarloResults.p95_return_pct,
                      desc: "Escenario muy favorable",
                    },
                  ].map((row) => (
                    <tr key={row.label}>
                      <td className="py-2 pr-4 text-muted-foreground">{row.label}</td>
                      <td
                        className={cn(
                          "py-2 pr-4 font-mono font-semibold",
                          row.value >= 0 ? "text-emerald-400" : "text-red-400"
                        )}
                      >
                        {fmtPct(row.value)}
                      </td>
                      <td className="py-2 text-xs text-muted-foreground">{row.desc}</td>
                    </tr>
                  ))}
                  <tr className="border-t border-border/30">
                    <td className="py-2 pr-4 text-muted-foreground">DD Mediana</td>
                    <td className="py-2 pr-4 font-mono font-semibold text-red-400">
                      -{fmt(monteCarloResults.median_max_drawdown_pct)}%
                    </td>
                    <td className="py-2 text-xs text-muted-foreground">Drawdown típico esperado</td>
                  </tr>
                  <tr>
                    <td className="py-2 pr-4 text-muted-foreground">DD P95</td>
                    <td className="py-2 pr-4 font-mono font-semibold text-red-400">
                      -{fmt(monteCarloResults.p95_max_drawdown_pct)}%
                    </td>
                    <td className="py-2 text-xs text-muted-foreground">Drawdown en el peor 5%</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>

          {/* Interpretation guide */}
          <div className="rounded-lg border border-border/30 bg-background/40 p-4">
            <h3 className="mb-2 flex items-center gap-2 text-xs font-semibold text-muted-foreground uppercase tracking-wider">
              <Activity className="h-3.5 w-3.5" />
              Cómo interpretar los resultados
            </h3>
            <ul className="space-y-1 text-xs text-muted-foreground">
              <li>
                <span className="font-medium text-foreground">Probabilidad de ruina &lt; 5%</span>{" "}
                — estrategia estadísticamente robusta al reorden aleatorio de trades.
              </li>
              <li>
                <span className="font-medium text-foreground">P25 positivo</span> — 75% de los
                escenarios termina en ganancia, muy buena señal de consistencia.
              </li>
              <li>
                <span className="font-medium text-foreground">P5 – P95 estrechos</span> — los
                resultados no dependen mucho del orden; la estrategia tiene edge real.
              </li>
              <li>
                <span className="font-medium text-foreground">DD P95</span> — el drawdown que
                deberías esperar en el peor 5% de los escenarios posibles.
              </li>
            </ul>
          </div>
        </div>
      )}
    </div>
  );
}
