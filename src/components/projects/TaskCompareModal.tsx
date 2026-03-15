import { useState, useMemo } from "react";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";
import type { Project } from "@/lib/types";

interface TaskCompareModalProps {
  project: Project;
  onClose: () => void;
}

function MiniSparkline({ data }: { data: number[] }) {
  if (data.length < 2) return <span className="text-[10px] text-muted-foreground/30">—</span>;
  const min = Math.min(...data);
  const max = Math.max(...data);
  const range = max - min || 1;
  const w = 80; const h = 30;
  const pts = data
    .map((v, i) => `${(i / (data.length - 1)) * w},${h - ((v - min) / range) * (h - 4) - 2}`)
    .join(" ");
  const isPos = data[data.length - 1] >= data[0];
  return (
    <svg width={w} height={h} className="overflow-visible">
      <polyline points={pts} fill="none"
        stroke={isPos ? "rgb(34 197 94)" : "rgb(239 68 68)"}
        strokeWidth={1.5} strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}

interface MetricRowProps {
  label: string;
  a: number | null;
  b: number | null;
  format?: (v: number) => string;
  higherIsBetter?: boolean;
}

function MetricRow({ label, a, b, format = (v) => v.toFixed(2), higherIsBetter = true }: MetricRowProps) {
  const aWins = a !== null && b !== null && (higherIsBetter ? a > b : a < b);
  const bWins = a !== null && b !== null && (higherIsBetter ? b > a : b < a);
  return (
    <div className="flex items-center gap-2 py-0.5 text-[11px] border-b border-border/10 last:border-0">
      <span className="w-28 shrink-0 text-muted-foreground/60">{label}</span>
      <span className={cn("flex-1 text-right tabular-nums", aWins ? "text-emerald-400 font-bold" : "text-foreground")}>
        {a !== null ? format(a) : "—"}
      </span>
      <span className={cn("flex-1 text-right tabular-nums", bWins ? "text-emerald-400 font-bold" : "text-foreground")}>
        {b !== null ? format(b) : "—"}
      </span>
    </div>
  );
}

export function TaskCompareModal({ project, onClose }: TaskCompareModalProps) {
  const [slotA, setSlotA] = useState(project.tasks[0]?.id ?? "");
  const [slotB, setSlotB] = useState(project.tasks[1]?.id ?? "");

  const bestA = useMemo(() => {
    const task = project.tasks.find((t) => t.id === slotA);
    if (!task) return null;
    const all = task.databanks.flatMap((d) => d.strategies);
    if (all.length === 0) return null;
    return all.reduce((best, s) => s.fitness > best.fitness ? s : best);
  }, [slotA, project.tasks]);

  const bestB = useMemo(() => {
    const task = project.tasks.find((t) => t.id === slotB);
    if (!task) return null;
    const all = task.databanks.flatMap((d) => d.strategies);
    if (all.length === 0) return null;
    return all.reduce((best, s) => s.fitness > best.fitness ? s : best);
  }, [slotB, project.tasks]);

  const taskA = project.tasks.find((t) => t.id === slotA);
  const taskB = project.tasks.find((t) => t.id === slotB);

  const fmt$ = (v: number) => (v >= 0 ? "+" : "") + "$" + Math.abs(v).toLocaleString(undefined, { maximumFractionDigits: 0 });

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      onClick={onClose}
    >
      <div
        className="w-[700px] max-w-[96vw] rounded-xl border border-border bg-card shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border/40 px-5 py-3">
          <h2 className="text-sm font-semibold text-foreground">Compare Tasks</h2>
          <button onClick={onClose} className="rounded p-1 hover:bg-muted/40">
            <X className="h-4 w-4 text-muted-foreground" />
          </button>
        </div>

        <div className="p-5 space-y-4">
          {/* Slot selectors */}
          <div className="grid grid-cols-2 gap-4">
            {[
              { label: "Task A", value: slotA, onChange: setSlotA },
              { label: "Task B", value: slotB, onChange: setSlotB },
            ].map(({ label, value, onChange }) => (
              <div key={label} className="space-y-1">
                <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">{label}</p>
                <select
                  value={value}
                  onChange={(e) => onChange(e.target.value)}
                  className="w-full rounded border border-border bg-background px-2 py-1.5 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary/50"
                >
                  {project.tasks.map((t) => (
                    <option key={t.id} value={t.id}>{t.name}</option>
                  ))}
                </select>
              </div>
            ))}
          </div>

          {/* Task summaries */}
          <div className="grid grid-cols-2 gap-4">
            {[
              { task: taskA, best: bestA },
              { task: taskB, best: bestB },
            ].map(({ task, best }, idx) => (
              <div key={idx} className="rounded border border-border/30 bg-muted/5 p-3 space-y-2">
                {task ? (
                  <>
                    <div className="flex items-center gap-2">
                      <span className="truncate text-sm font-semibold text-foreground">{task.name}</span>
                      <span className={cn(
                        "shrink-0 rounded px-1.5 py-0.5 text-[9px] font-bold uppercase",
                        task.status === "running" ? "bg-emerald-500/20 text-emerald-400" :
                        task.status === "paused" ? "bg-yellow-500/20 text-yellow-400" :
                        "bg-muted/40 text-muted-foreground/60"
                      )}>{task.status}</span>
                    </div>
                    <p className="text-[10px] text-muted-foreground/60">
                      {task.databanks.reduce((n, d) => n + d.strategies.length, 0)} strategies across {task.databanks.length} databank{task.databanks.length !== 1 ? "s" : ""}
                    </p>
                    {best ? (
                      <div className="space-y-1">
                        <p className="text-[9px] font-semibold uppercase tracking-wider text-muted-foreground/40">Best strategy</p>
                        <MiniSparkline data={best.miniEquityCurve} />
                        <p className="text-[10px] text-muted-foreground/60 truncate">{best.name}</p>
                      </div>
                    ) : (
                      <p className="text-[10px] text-muted-foreground/40">No strategies yet</p>
                    )}
                  </>
                ) : (
                  <p className="text-xs text-muted-foreground/40">Select a task</p>
                )}
              </div>
            ))}
          </div>

          {/* Metric comparison */}
          {(bestA || bestB) && (
            <div className="rounded border border-border/30 bg-muted/5 p-3">
              <p className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">Metrics (bold = better)</p>
              <MetricRow label="Fitness"       a={bestA?.fitness ?? null}        b={bestB?.fitness ?? null} />
              <MetricRow label="Net Profit"    a={bestA?.netProfit ?? null}      b={bestB?.netProfit ?? null} format={fmt$} />
              <MetricRow label="Profit Factor" a={bestA?.profitFactor ?? null}   b={bestB?.profitFactor ?? null} />
              <MetricRow label="Sharpe Ratio"  a={bestA?.sharpeRatio ?? null}    b={bestB?.sharpeRatio ?? null} />
              <MetricRow label="Max Drawdown"  a={bestA?.maxDrawdownAbs ?? null} b={bestB?.maxDrawdownAbs ?? null} format={(v) => "$" + Math.abs(v).toLocaleString()} higherIsBetter={false} />
              <MetricRow label="# Trades"      a={bestA?.trades ?? null}         b={bestB?.trades ?? null} format={(v) => v.toLocaleString()} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
