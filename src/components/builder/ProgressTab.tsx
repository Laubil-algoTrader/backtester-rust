import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { Play, Square, Pause, Trash2, ChevronDown } from "lucide-react";
import { useAppStore } from "@/stores/useAppStore";
import { startBuilder, stopBuilder, pauseBuilder, cancelSrBuilder } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import type { BuilderRuntimeStats, BuilderSavedStrategy, BuilderIslandStats } from "@/lib/types";

// ── Running time counter ─────────────────────────────────────────────────────

function useElapsedTime(startTime: number | null, running: boolean) {
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    if (!running || startTime === null) {
      setElapsed(0);
      return;
    }
    const id = setInterval(() => setElapsed(Date.now() - startTime), 1000);
    return () => clearInterval(id);
  }, [running, startTime]);

  if (!running || startTime === null) return "—";
  const totalSec = Math.floor(elapsed / 1000);
  const h = Math.floor(totalSec / 3600).toString().padStart(2, "0");
  const m = Math.floor((totalSec % 3600) / 60).toString().padStart(2, "0");
  const s = (totalSec % 60).toString().padStart(2, "0");
  return `${h}:${m}:${s}`;
}

// ── Sparkline ────────────────────────────────────────────────────────────────

function Sparkline({ data, width = 120, height = 40 }: { data: number[]; width?: number; height?: number }) {
  if (data.length < 2) {
    return <div style={{ width, height }} className="flex items-center justify-center text-[10px] text-muted-foreground/40">Sin datos</div>;
  }
  const min = Math.min(...data);
  const max = Math.max(...data);
  const range = max - min || 1;
  const pts = data
    .map((v, i) => {
      const x = (i / (data.length - 1)) * width;
      const y = height - ((v - min) / range) * (height - 4) - 2;
      return `${x},${y}`;
    })
    .join(" ");
  const isPositive = data[data.length - 1] >= data[0];
  return (
    <svg width={width} height={height} className="overflow-visible">
      <polyline
        points={pts}
        fill="none"
        stroke={isPositive ? "rgb(34 197 94)" : "rgb(239 68 68)"}
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}

// ── Big stat counter ─────────────────────────────────────────────────────────

function BigStat({ label, value, color }: { label: string; value: string | number; color?: string }) {
  return (
    <div className="flex flex-1 flex-col items-center gap-0.5 rounded border border-border/30 bg-muted/5 px-3 py-3">
      <span className={cn("text-2xl font-bold tabular-nums tracking-tight", color ?? "text-foreground")}>
        {value}
      </span>
      <span className="text-[10px] uppercase tracking-wider text-muted-foreground/55">{label}</span>
    </div>
  );
}

// ── Time stat row item ────────────────────────────────────────────────────────

function TimeStat({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[10px] uppercase tracking-wider text-muted-foreground/50">{label}</span>
      <span className="text-sm font-semibold tabular-nums text-foreground">{value}</span>
    </div>
  );
}

// ── Best strategy mini-card ──────────────────────────────────────────────────

function BestStrategyCard({ rank, strategy }: {
  rank: 1 | 2;
  strategy: { fitness: number; netProfit: number; trades: number; profitFactor: number; miniEquityCurve: number[] } | null;
}) {
  const { t } = useTranslation("builder");
  const label = rank === 1 ? t("progress.bestStrategy") : t("progress.secondBest");

  return (
    <div className="flex flex-col rounded-md border border-border/40 bg-muted/5 p-3">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/70">
          #{rank} — {label}
        </span>
        {strategy && (
          <span className="rounded border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] font-bold text-primary">
            {strategy.fitness.toFixed(4)}
          </span>
        )}
      </div>

      {strategy ? (
        <>
          <Sparkline data={strategy.miniEquityCurve} width={140} height={36} />
          <div className="mt-2 grid grid-cols-3 gap-x-2 gap-y-0.5 text-[10px]">
            <span className="text-muted-foreground/60">{t("progress.metrics.totalProfit")}</span>
            <span className={cn("col-span-2 font-medium tabular-nums", strategy.netProfit >= 0 ? "text-emerald-400" : "text-red-400")}>
              ${strategy.netProfit.toLocaleString(undefined, { maximumFractionDigits: 0 })}
            </span>
            <span className="text-muted-foreground/60">{t("progress.metrics.trades")}</span>
            <span className="col-span-2 font-medium tabular-nums text-foreground">{strategy.trades}</span>
            <span className="text-muted-foreground/60">{t("progress.metrics.profitFactor")}</span>
            <span className="col-span-2 font-medium tabular-nums text-foreground">{strategy.profitFactor.toFixed(2)}</span>
          </div>
        </>
      ) : (
        <div className="flex h-16 items-center justify-center text-[10px] text-muted-foreground/40">
          {t("progress.noStrategiesGenerated")}
        </div>
      )}
    </div>
  );
}

// ── Settings summary ─────────────────────────────────────────────────────────

function SettingsSummary() {
  const { builderConfig } = useAppStore();
  const wtb = builderConfig.whatToBuild;
  const go = builderConfig.geneticOptions;

  const rows = [
    ["Dirección", wtb.direction.replace(/_/g, " ")],
    ["Reglas entrada", `${wtb.minEntryRules}–${wtb.maxEntryRules}`],
    ["Reglas salida", `${wtb.minExitRules}–${wtb.maxExitRules}`],
    ["Periodos", `${wtb.indicatorPeriodMin}–${wtb.indicatorPeriodMax}`],
    ["Generaciones", go.maxGenerations],
    ["Pobl./isla", go.populationPerIsland],
    ["Islas", go.islands],
    ["Cruce", `${go.crossoverProbability}%`],
    ["Mutación", `${go.mutationProbability}%`],
  ];

  return (
    <div className="space-y-1">
      {rows.map(([label, value]) => (
        <div key={String(label)} className="flex justify-between text-[10px]">
          <span className="text-muted-foreground/60">{label}</span>
          <span className="font-medium text-foreground capitalize">{String(value)}</span>
        </div>
      ))}
    </div>
  );
}

// ── SR Progress View ──────────────────────────────────────────────────────────

function SrProgressView() {
  const {
    srRunning, setSrRunning,
    srProgress, setSrProgress,
    srResults,
    setBuilderTopTab,
  } = useAppStore();

  const [startTime, setStartTime] = useState<number | null>(null);

  useEffect(() => {
    if (srRunning) {
      setStartTime((prev) => prev ?? Date.now());
    } else {
      setStartTime(null);
    }
  }, [srRunning]);

  const elapsed = useElapsedTime(startTime, srRunning);

  const handleStop = async () => {
    setSrRunning(false);
    setSrProgress(null);
    try {
      await cancelSrBuilder();
    } catch (e) {
      console.error(e);
    }
  };

  const isGeneration = srProgress?.phase === "generation";
  const isCmaes = srProgress?.phase === "cmaes";

  const fillPct = isGeneration && srProgress
    ? Math.min(100, (srProgress.databank_count / Math.max(1, srProgress.databank_limit)) * 100)
    : isCmaes && srProgress
    ? Math.min(100, (srProgress.current / Math.max(1, srProgress.total)) * 100)
    : 0;

  return (
    <div className="flex h-full flex-col gap-0 overflow-auto">
      <div className="flex flex-col gap-4 p-4">

        {/* ── Control bar ──────────────────────────────────────────────────── */}
        <div className="flex items-center gap-2">
          {srRunning ? (
            <>
              <button
                onClick={handleStop}
                className="flex items-center gap-2 rounded border border-destructive/40 bg-destructive/10 px-5 py-2.5 text-sm font-semibold text-destructive transition-colors hover:bg-destructive/20"
              >
                <Square className="h-4 w-4" />
                Detener SR Builder
              </button>
              <span className="ml-1 inline-flex items-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium bg-primary/10 text-primary">
                <span className="h-1.5 w-1.5 rounded-full animate-pulse bg-primary" />
                {isCmaes ? "Refinando con CMA-ES…" : "Ejecutando NSGA-II…"}
              </span>
            </>
          ) : (
            <button
              onClick={() => setBuilderTopTab("fullSettings")}
              className="flex items-center gap-2 rounded px-5 py-2.5 text-sm font-semibold bg-primary text-primary-foreground hover:bg-primary/90 transition-colors"
            >
              <Play className="h-4 w-4" />
              Configurar e Iniciar
            </button>
          )}
        </div>

        {/* ── Big counters (generation phase) ─────────────────────────────── */}
        {srProgress && isGeneration && (
          <div className="flex gap-2">
            <BigStat label="Evaluadas" value={srProgress.total_evaluated.toLocaleString()} />
            <BigStat label="Databank" value={`${srProgress.databank_count}/${srProgress.databank_limit}`} color="text-primary" />
            <BigStat label="Pareto" value={srProgress.pareto_size} />
            <BigStat label="Mejor Sharpe" value={srProgress.best_sharpe.toFixed(3)} color="text-emerald-400" />
          </div>
        )}

        {/* ── Big counters (CMA-ES phase) ──────────────────────────────────── */}
        {srProgress && isCmaes && (
          <div className="flex gap-2">
            <BigStat label="Refinando" value={`${srProgress.current} / ${srProgress.total}`} color="text-primary" />
            <BigStat label="Progreso" value={`${Math.round((srProgress.current / Math.max(1, srProgress.total)) * 100)}%`} />
          </div>
        )}

        {/* ── Progress bar ─────────────────────────────────────────────────── */}
        {srProgress && (
          <div className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between text-[10px] text-muted-foreground/60">
              <span>
                {isGeneration
                  ? `Generación ${srProgress.gen}${srProgress.total > 0 ? ` / ${srProgress.total}` : " — sin límite"} · Databank ${srProgress.databank_count}/${srProgress.databank_limit}`
                  : `Refinando constantes ${srProgress.current} / ${srProgress.total}`}
              </span>
              <span className="tabular-nums font-medium text-foreground/70">
                {fillPct.toFixed(0)}%
              </span>
            </div>
            <div className="h-2 w-full overflow-hidden rounded-full bg-muted/30">
              <div
                className="h-full rounded-full bg-primary transition-all duration-500"
                style={{ width: `${fillPct}%` }}
              />
            </div>
          </div>
        )}

        {/* ── Time stats (generation phase only) ──────────────────────────── */}
        {srProgress && isGeneration && (
          <div className="grid grid-cols-3 gap-4 rounded border border-border/25 bg-muted/5 px-4 py-3">
            <TimeStat
              label="ms / estrategia"
              value={
                srProgress.strategies_per_sec > 0
                  ? (1000 / srProgress.strategies_per_sec).toFixed(1)
                  : "—"
              }
            />
            <TimeStat label="Total evaluadas" value={srProgress.total_evaluated.toLocaleString()} />
            <TimeStat label="Tiempo total" value={elapsed} />
          </div>
        )}

        {/* ── Idle — no results ────────────────────────────────────────────── */}
        {!srRunning && !srProgress && srResults.length === 0 && (
          <div className="flex flex-col items-center justify-center gap-3 rounded border border-border/25 bg-muted/5 px-4 py-10 text-center">
            <span className="text-sm text-muted-foreground/60">No hay ninguna ejecución activa</span>
            <p className="text-[11px] text-muted-foreground/40 max-w-xs">
              Configurá la Regresión Simbólica en{" "}
              <strong className="text-muted-foreground/60">Configuración completa</strong>{" "}
              y presioná <strong className="text-muted-foreground/60">Iniciar SR Builder</strong>.
            </p>
            <button
              onClick={() => setBuilderTopTab("fullSettings")}
              className="mt-1 rounded border border-border/40 px-3 py-1.5 text-xs text-muted-foreground hover:text-foreground hover:border-primary/50 transition-colors"
            >
              Ir a Configuración →
            </button>
          </div>
        )}

        {/* ── Done — results ready ─────────────────────────────────────────── */}
        {!srRunning && srResults.length > 0 && (
          <div className="flex flex-col items-center justify-center gap-3 rounded border border-primary/20 bg-primary/5 px-4 py-6 text-center">
            <span className="text-sm font-medium text-foreground">
              SR Builder completado — {srResults.length} estrategia{srResults.length !== 1 ? "s" : ""} en el frente de Pareto
            </span>
            <button
              onClick={() => setBuilderTopTab("results")}
              className="rounded bg-primary/10 border border-primary/30 px-4 py-1.5 text-xs text-primary hover:bg-primary/20 transition-colors"
            >
              Ver resultados →
            </button>
          </div>
        )}

      </div>
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

export function ProgressTab() {
  const { t } = useTranslation("builder");
  const {
    builderMethod,
    builderRunning,
    builderPaused,
    setBuilderRunning,
    setBuilderPaused,
    builderLog,
    clearBuilderLog,
    addBuilderLog,
    builderStats,
    setBuilderStats,
    builderDatabanks,
    targetDatabankId,
    addToBuilderDatabank,
    builderConfig,
    builderIslandStats,
    upsertBuilderIslandStats,
    clearBuilderIslandStats,
  } = useAppStore();

  const logRef = useRef<HTMLDivElement>(null);
  const [clearOnStart, setClearOnStart] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [logOpen, setLogOpen] = useState(false);
  const unlistenRefs = useRef<Array<() => void>>([]);

  // Auto-scroll log
  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [builderLog]);

  // Cleanup listeners on unmount
  useEffect(() => {
    return () => {
      unlistenRefs.current.forEach((fn) => fn());
      unlistenRefs.current = [];
    };
  }, []);

  const elapsed = useElapsedTime(builderStats.startTime, builderRunning);

  // SR Builder uses its own progress view
  if (builderMethod === "sr") return <SrProgressView />;

  const handleStart = async () => {
    const dc = builderConfig.dataConfig;
    if (!dc.symbolId) {
      setError("No hay símbolo seleccionado. Ve a Configuración completa → Datos.");
      return;
    }
    setError(null);
    if (clearOnStart) clearBuilderLog();
    clearBuilderIslandStats();

    const cleanups: Array<() => void> = [];

    const unStats = await listen<BuilderRuntimeStats>("builder-stats", (event) => {
      const { startTime: _rustElapsed, ...rest } = event.payload;
      setBuilderStats(rest);
    });
    cleanups.push(unStats);

    const unLog = await listen<string>("builder-log", (event) => {
      addBuilderLog(`[${new Date().toLocaleTimeString()}] ${event.payload}`);
    });
    cleanups.push(unLog);

    const unStrategy = await listen<BuilderSavedStrategy>("builder-strategy-found", (event) => {
      const cfg = builderConfig.dataConfig;
      addToBuilderDatabank({
        ...event.payload,
        startDate: cfg.startDate ?? "",
        endDate: cfg.endDate ?? "",
        initialCapital: builderConfig.moneyManagement.initialCapital ?? 10000,
      });
    });
    cleanups.push(unStrategy);

    const unIslandStats = await listen<BuilderIslandStats>("builder-island-stats", (event) => {
      upsertBuilderIslandStats(event.payload);
    });
    cleanups.push(unIslandStats);

    const unFinished = await listen<void>("builder-finished", () => {
      setBuilderRunning(false);
      setBuilderPaused(false);
      addBuilderLog(`[${new Date().toLocaleTimeString()}] Builder finalizado`);
      unlistenRefs.current.forEach((fn) => fn());
      unlistenRefs.current = [];
    });
    cleanups.push(unFinished);

    unlistenRefs.current = cleanups;

    setBuilderRunning(true);
    setBuilderPaused(false);
    setBuilderStats({
      startTime: Date.now(),
      generated: 0,
      accepted: 0,
      rejected: 0,
      inDatabank: 0,
      generation: 0,
      island: 0,
      bestFitness: 0,
      strategiesPerHour: 0,
      acceptedPerHour: 0,
      timePerStrategyMs: 0,
    });
    addBuilderLog(`[${new Date().toLocaleTimeString()}] Builder iniciado`);

    try {
      await startBuilder(
        builderConfig,
        dc.symbolId,
        dc.timeframe,
        dc.startDate,
        dc.endDate,
        builderConfig.moneyManagement.initialCapital,
      );
    } catch (err) {
      const msg = typeof err === "string" ? err : err instanceof Error ? err.message : JSON.stringify(err);
      if (!msg.includes("cancel") && !msg.includes("Cancel")) {
        setError(msg);
        addBuilderLog(`[${new Date().toLocaleTimeString()}] Error: ${msg}`);
      }
      setBuilderRunning(false);
      setBuilderPaused(false);
      unlistenRefs.current.forEach((fn) => fn());
      unlistenRefs.current = [];
    }
  };

  const handleStop = async () => {
    setBuilderRunning(false);
    setBuilderPaused(false);
    addBuilderLog(`[${new Date().toLocaleTimeString()}] Deteniendo builder…`);
    unlistenRefs.current.forEach((fn) => fn());
    unlistenRefs.current = [];
    try {
      await stopBuilder();
      addBuilderLog(`[${new Date().toLocaleTimeString()}] Builder detenido`);
    } catch (err) {
      const msg = typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
      addBuilderLog(`[${new Date().toLocaleTimeString()}] Error al detener: ${msg}`);
    }
  };

  const handlePause = async () => {
    const newPaused = !builderPaused;
    setBuilderPaused(newPaused);
    addBuilderLog(`[${new Date().toLocaleTimeString()}] ${newPaused ? "Pausando…" : "Reanudando…"}`);
    try {
      await pauseBuilder(newPaused);
    } catch (err) {
      const msg = typeof err === "string" ? err : err instanceof Error ? err.message : String(err);
      addBuilderLog(`[${new Date().toLocaleTimeString()}] Error: ${msg}`);
    }
  };

  const targetStrategies = builderDatabanks.find((db) => db.id === targetDatabankId)?.strategies ?? [];
  const top2 = targetStrategies.slice().sort((a, b) => b.fitness - a.fitness).slice(0, 2);

  const maxGen = builderConfig.geneticOptions.maxGenerations;
  const currentGen = builderStats.generation;
  const genPct = maxGen > 0 && currentGen > 0 ? Math.min(100, (currentGen / maxGen) * 100) : 0;

  return (
    <div className="flex h-full flex-col gap-0 overflow-auto">
      <div className="flex flex-col gap-4 p-4">

        {/* ── Control bar ────────────────────────────────────────────────── */}
        <div className="flex items-center gap-2">
          <button
            onClick={!builderRunning ? handleStart : undefined}
            disabled={builderRunning}
            className={cn(
              "flex items-center gap-2 rounded px-5 py-2.5 text-sm font-semibold transition-colors",
              !builderRunning
                ? "bg-primary text-primary-foreground hover:bg-primary/90"
                : "cursor-default bg-muted/30 text-muted-foreground"
            )}
          >
            <Play className="h-4 w-4" />
            {builderRunning
              ? builderPaused ? "Pausado" : "Construyendo…"
              : t("progress.start") + " construcción"}
          </button>

          {builderRunning && (
            <>
              <button
                onClick={handlePause}
                title={builderPaused ? "Reanudar" : "Pausar"}
                className={cn(
                  "flex h-9 w-9 items-center justify-center rounded border transition-colors",
                  builderPaused
                    ? "border-amber-500/50 bg-amber-500/10 text-amber-400 hover:bg-amber-500/20"
                    : "border-border/40 bg-muted/20 text-muted-foreground hover:bg-muted/50 hover:text-foreground"
                )}
              >
                <Pause className="h-4 w-4" />
              </button>
              <button
                onClick={handleStop}
                title="Detener"
                className="flex h-9 w-9 items-center justify-center rounded border border-destructive/40 bg-destructive/10 text-destructive transition-colors hover:bg-destructive/20"
              >
                <Square className="h-4 w-4" />
              </button>
            </>
          )}

          {builderRunning && (
            <span className={cn(
              "ml-1 inline-flex items-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium",
              builderPaused ? "bg-amber-500/10 text-amber-400" : "bg-primary/10 text-primary"
            )}>
              <span className={cn(
                "h-1.5 w-1.5 rounded-full",
                builderPaused ? "bg-amber-400" : "animate-pulse bg-primary"
              )} />
              {builderPaused ? "Pausado" : "Ejecutando"}
            </span>
          )}
        </div>

        {/* Error */}
        {error && (
          <div className="rounded border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
            {error}
          </div>
        )}

        {/* ── Big counters ────────────────────────────────────────────────── */}
        <div className="flex gap-2">
          <BigStat label="Generadas" value={builderStats.generated.toLocaleString()} />
          <BigStat label="Aceptadas" value={builderStats.accepted.toLocaleString()} color="text-primary" />
          <BigStat label="Rechazadas" value={builderStats.rejected.toLocaleString()} color="text-muted-foreground" />
          <BigStat label="Base de datos" value={builderStats.inDatabank.toLocaleString()} color="text-primary" />
        </div>

        {/* ── Progress bar ────────────────────────────────────────────────── */}
        <div className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between text-[10px] text-muted-foreground/60">
            <span>
              {currentGen > 0
                ? `Generación ${currentGen} / ${maxGen}`
                : "Esperando inicio…"}
            </span>
            <span className="tabular-nums font-medium text-foreground/70">
              {genPct > 0 ? `${genPct.toFixed(0)}%` : ""}
            </span>
          </div>
          <div className="h-2 w-full overflow-hidden rounded-full bg-muted/30">
            <div
              className="h-full rounded-full bg-primary transition-all duration-500"
              style={{ width: `${genPct}%` }}
            />
          </div>
          {builderStats.bestFitness > 0 && (
            <div className="flex gap-4 text-[10px] text-muted-foreground/60">
              <span>
                Estrategias evaluadas:{" "}
                <span className="font-medium text-foreground/80">{builderStats.generated.toLocaleString()}</span>
              </span>
              <span>
                Mejor fitness:{" "}
                <span className="font-medium text-primary">{builderStats.bestFitness.toFixed(4)}</span>
              </span>
            </div>
          )}
        </div>

        {/* ── Time stats ──────────────────────────────────────────────────── */}
        <div className="grid grid-cols-5 gap-4 rounded border border-border/25 bg-muted/5 px-4 py-3">
          <TimeStat
            label="Vel. generación"
            value={builderStats.timePerStrategyMs > 0
              ? `${(1000 / builderStats.timePerStrategyMs).toFixed(1)} /seg`
              : "—"}
          />
          <TimeStat
            label="Tiempo por estrategia"
            value={builderStats.timePerStrategyMs > 0 ? `${builderStats.timePerStrategyMs.toFixed(0)} ms` : "—"}
          />
          <TimeStat
            label="Estrategias por hora"
            value={builderStats.strategiesPerHour > 0 ? builderStats.strategiesPerHour.toLocaleString() : "—"}
          />
          <TimeStat
            label="Aceptadas por hora"
            value={builderStats.acceptedPerHour > 0 ? builderStats.acceptedPerHour.toLocaleString() : "—"}
          />
          <TimeStat label="Tiempo total" value={elapsed} />
        </div>

        {/* ── Bottom 2-column: evolution + best strategies ─────────────── */}
        <div className="grid grid-cols-2 gap-3">

          {/* Left — Genetic evolution table */}
          <div className="flex flex-col rounded-md border border-border/40 bg-muted/5 p-3">
            <div className="mb-2 flex items-center justify-between">
              <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/70">
                Evolución genética
              </span>
              {builderRunning && (
                <span className="text-[10px] tabular-nums text-muted-foreground/60">
                  Gen {builderStats.generation} &middot; Best {builderStats.bestFitness.toFixed(4)}
                </span>
              )}
            </div>
            {builderIslandStats.length > 0 ? (
              <div className="overflow-x-auto">
                <table className="w-full text-[10px]">
                  <thead>
                    <tr className="border-b border-border/30 text-muted-foreground/60">
                      <th className="py-1 pr-3 text-left font-medium">Isla</th>
                      <th className="py-1 pr-3 text-right font-medium">Generación</th>
                      <th className="py-1 pr-3 text-right font-medium">Población</th>
                      <th className="py-1 text-right font-medium">Mejor fitness</th>
                    </tr>
                  </thead>
                  <tbody>
                    {builderIslandStats
                      .slice()
                      .sort((a, b) => a.islandId - b.islandId)
                      .map((is) => (
                        <tr key={is.islandId} className="border-b border-border/10">
                          <td className="py-1 pr-3 font-medium text-foreground">#{is.islandId}</td>
                          <td className="py-1 pr-3 text-right tabular-nums text-foreground">{is.generation}</td>
                          <td className="py-1 pr-3 text-right tabular-nums text-foreground">{is.population}</td>
                          <td className={cn(
                            "py-1 text-right tabular-nums font-medium",
                            is.bestFitness > 0 ? "text-primary" : "text-muted-foreground"
                          )}>
                            {is.bestFitness.toFixed(4)}
                          </td>
                        </tr>
                      ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="text-[10px] text-muted-foreground/50">{t("progress.noGeneticRunning")}</p>
            )}
          </div>

          {/* Right — best strategies + settings */}
          <div className="flex flex-col gap-2">
            <BestStrategyCard
              rank={1}
              strategy={top2[0] ? {
                fitness: top2[0].fitness,
                netProfit: top2[0].netProfit,
                trades: top2[0].trades,
                profitFactor: top2[0].profitFactor,
                miniEquityCurve: top2[0].miniEquityCurve,
              } : null}
            />
            <BestStrategyCard
              rank={2}
              strategy={top2[1] ? {
                fitness: top2[1].fitness,
                netProfit: top2[1].netProfit,
                trades: top2[1].trades,
                profitFactor: top2[1].profitFactor,
                miniEquityCurve: top2[1].miniEquityCurve,
              } : null}
            />
            <div className="rounded-md border border-border/40 bg-muted/5 p-3">
              <div className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/70">
                {t("progress.settingsSummary")}
              </div>
              <SettingsSummary />
            </div>
          </div>
        </div>

        {/* ── Collapsible log ─────────────────────────────────────────────── */}
        <div className="rounded-md border border-border/40">
          <button
            onClick={() => setLogOpen((v) => !v)}
            className="flex w-full items-center justify-between px-3 py-2"
          >
            <div className="flex items-center gap-2">
              <ChevronDown className={cn("h-3.5 w-3.5 text-muted-foreground/60 transition-transform", logOpen && "rotate-180")} />
              <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/70">
                Log de ejecución
              </span>
              {builderLog.length > 0 && (
                <span className="rounded bg-muted/40 px-1.5 py-0.5 text-[9px] tabular-nums text-muted-foreground/60">
                  {builderLog.length}
                </span>
              )}
            </div>
            <div className="flex items-center gap-2">
              <label
                className="flex items-center gap-1.5 text-[10px] text-muted-foreground/60 cursor-pointer"
                onClick={(e) => e.stopPropagation()}
              >
                <input
                  type="checkbox"
                  checked={clearOnStart}
                  onChange={(e) => setClearOnStart(e.target.checked)}
                  className="h-3 w-3 accent-primary"
                />
                {t("progress.clearLogOnStart")}
              </label>
              <button
                onClick={(e) => { e.stopPropagation(); clearBuilderLog(); }}
                title={t("progress.clearLog")}
                className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground/50 hover:text-foreground"
              >
                <Trash2 className="h-3 w-3" />
              </button>
            </div>
          </button>

          {logOpen && (
            <div
              ref={logRef}
              className="border-t border-border/30 p-2 font-mono text-[10px] leading-relaxed text-muted-foreground overflow-y-auto"
              style={{ maxHeight: "200px" }}
            >
              {builderLog.length === 0 ? (
                <span className="text-muted-foreground/30">Presiona Iniciar para comenzar…</span>
              ) : (
                builderLog.map((line, i) => (
                  <div key={i} className={cn(
                    "whitespace-pre-wrap",
                    line.includes("error") || line.includes("Error") ? "text-red-400" :
                    line.includes("Accept") || line.includes("accepted") ? "text-emerald-400" :
                    line.includes("iniciado") || line.includes("finalizado") ? "text-primary" : ""
                  )}>
                    {line}
                  </div>
                ))
              )}
            </div>
          )}
        </div>

      </div>
    </div>
  );
}
