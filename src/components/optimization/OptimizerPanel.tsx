import { useEffect, useRef, useState, useMemo } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "@/stores/useAppStore";
import { runOptimization, cancelOptimization } from "@/lib/tauri";
import { sortTimeframes, PRECISION_LABELS } from "@/lib/types";
import type {
  BacktestConfig,
  BacktestPrecision,
  Strategy,
  Timeframe,
  OptimizationConfig,
  OptimizationMethod,
  ObjectiveFunction,
  ParameterRange,
  GeneticAlgorithmConfig,
} from "@/lib/types";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/Select";
import { Input } from "@/components/ui/Input";
import { Button } from "@/components/ui/Button";
import { Progress } from "@/components/ui/Progress";
import { DatePicker } from "@/components/ui/DatePicker";
import { Play, Square, AlertCircle, AlertTriangle, Plus, X } from "lucide-react";

function formatEta(seconds: number): string {
  if (seconds < 1) return "<1s";
  if (seconds < 60) return `${Math.round(seconds)}s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  if (m < 60) return s > 0 ? `${m}m ${s}s` : `${m}m`;
  const h = Math.floor(m / 60);
  const rm = m % 60;
  return rm > 0 ? `${h}h ${rm}m` : `${h}h`;
}

const TIMEFRAME_LABELS: Record<string, string> = {
  tick: "Tick",
  m1: "M1",
  m5: "M5",
  m15: "M15",
  m30: "M30",
  h1: "H1",
  h4: "H4",
  d1: "D1",
};

const OBJECTIVE_OPTIONS: { value: ObjectiveFunction; label: string }[] = [
  { value: "TotalProfit", label: "Total Profit" },
  { value: "SharpeRatio", label: "Sharpe Ratio" },
  { value: "ProfitFactor", label: "Profit Factor" },
  { value: "WinRate", label: "Win Rate" },
  { value: "ReturnDdRatio", label: "Return/DD Ratio" },
  { value: "MinStagnation", label: "Min Stagnation" },
  { value: "MinUlcerIndex", label: "Min Ulcer Index" },
];

interface OptimizerPanelProps {
  parameterRanges: ParameterRange[];
}

export function OptimizerPanel({ parameterRanges }: OptimizerPanelProps) {
  const {
    symbols,
    selectedSymbolId,
    setSelectedSymbolId,
    selectedTimeframe,
    setSelectedTimeframe,
    backtestStartDate,
    setBacktestStartDate,
    backtestEndDate,
    setBacktestEndDate,
    initialCapital,
    setInitialCapital,
    leverage,
    setLeverage,
    backtestPrecision,
    setBacktestPrecision,
    currentStrategy,
    isLoading,
    setLoading,
    progressPercent,
    setProgress,
    setOptimizationResults,
    optimizationOosPeriods: oosPeriods,
    setOptimizationOosPeriods: setOosPeriods,
  } = useAppStore();

  const [method, setMethod] = useState<OptimizationMethod>("GridSearch");
  const [objectives, setObjectives] = useState<ObjectiveFunction[]>(["SharpeRatio"]);
  const [error, setError] = useState<string | null>(null);
  const [bestSoFar, setBestSoFar] = useState<number | null>(null);
  const [etaDisplay, setEtaDisplay] = useState<string>("");
  const unlistenRef = useRef<(() => void) | null>(null);

  // GA config
  const [populationSize, setPopulationSize] = useState(50);
  const [generations, setGenerations] = useState(20);
  const [mutationRate, setMutationRate] = useState(0.1);
  const [crossoverRate, setCrossoverRate] = useState(0.7);

  const selectedSymbol = symbols.find((s) => s.id === selectedSymbolId);
  const availableTimeframes = selectedSymbol
    ? sortTimeframes(Object.keys(selectedSymbol.timeframe_paths))
    : [];

  // Available precision modes depend on symbol base timeframe
  const availablePrecisions: BacktestPrecision[] = (() => {
    if (!selectedSymbol) return ["SelectedTfOnly"];
    const base = selectedSymbol.base_timeframe;
    const hasTick = !!selectedSymbol.timeframe_paths["tick"];
    const hasM1 = !!selectedSymbol.timeframe_paths["m1"];
    const hasTickRaw = Object.keys(selectedSymbol.timeframe_paths).some((k) => k === "tick_raw");

    if (base === "tick") {
      const modes: BacktestPrecision[] = ["SelectedTfOnly"];
      if (hasM1) modes.push("M1TickSimulation");
      if (hasTick) modes.push("RealTickCustomSpread");
      if (hasTickRaw) modes.push("RealTickRealSpread");
      return modes;
    }
    if (base === "m1") {
      const modes: BacktestPrecision[] = ["SelectedTfOnly"];
      if (hasM1) modes.push("M1TickSimulation");
      return modes;
    }
    return ["SelectedTfOnly"];
  })();

  // Auto-fill dates
  useEffect(() => {
    if (selectedSymbol) {
      if (!backtestStartDate) setBacktestStartDate(selectedSymbol.start_date);
      if (!backtestEndDate) setBacktestEndDate(selectedSymbol.end_date);
    }
  }, [selectedSymbolId]);

  // Cleanup
  useEffect(() => {
    return () => {
      if (unlistenRef.current) unlistenRef.current();
    };
  }, []);

  // Combination count for Grid Search
  const combinationCount = useMemo(() => {
    if (method !== "GridSearch" || parameterRanges.length === 0) return 0;
    let total = 1;
    for (const r of parameterRanges) {
      if (r.step <= 0) return Infinity;
      const count = Math.floor((r.max - r.min) / r.step) + 1;
      total *= count;
    }
    return total;
  }, [method, parameterRanges]);

  const canRun =
    selectedSymbolId &&
    (currentStrategy.long_entry_rules.length > 0 || currentStrategy.short_entry_rules.length > 0) &&
    parameterRanges.length > 0 &&
    !isLoading;

  // Ctrl+Enter shortcut listener
  useEffect(() => {
    const handler = () => {
      if (canRun) handleRun();
    };
    document.addEventListener("shortcut:run-optimization", handler);
    return () => document.removeEventListener("shortcut:run-optimization", handler);
  });

  const validate = (): string | null => {
    if (!selectedSymbolId) return "Select a symbol first.";
    if ((currentStrategy.long_entry_rules.length === 0 && currentStrategy.short_entry_rules.length === 0))
      return "Add at least one entry rule.";
    if (parameterRanges.length === 0)
      return "Enable at least one parameter range.";
    if (initialCapital <= 0) return "Capital must be greater than 0.";
    if (leverage < 1) return "Leverage must be at least 1.";
    if (
      backtestStartDate &&
      backtestEndDate &&
      backtestStartDate >= backtestEndDate
    )
      return "Start date must be before end date.";
    for (const r of parameterRanges) {
      if (r.min >= r.max)
        return `Parameter "${r.display_name}": min must be less than max.`;
      if (r.step <= 0)
        return `Parameter "${r.display_name}": step must be greater than 0.`;
    }
    if (method === "GeneticAlgorithm") {
      if (populationSize < 2) return "Population size must be at least 2.";
      if (generations < 1) return "Generations must be at least 1.";
      if (mutationRate < 0 || mutationRate > 1)
        return "Mutation rate must be between 0 and 1.";
      if (crossoverRate < 0 || crossoverRate > 1)
        return "Crossover rate must be between 0 and 1.";
    }
    return null;
  };

  const handleRun = async () => {
    const validationError = validate();
    if (validationError) {
      setError(validationError);
      return;
    }
    if (!selectedSymbolId) return;
    setError(null);
    setBestSoFar(null);
    setEtaDisplay("");
    setLoading(true, "Running optimization...");
    setOptimizationResults([]);

    unlistenRef.current = await listen<{
      percent: number;
      current: number;
      total: number;
      best_so_far: number;
      eta_seconds: number;
    }>("optimization-progress", (event) => {
      setProgress(event.payload.percent);
      if (event.payload.best_so_far > -Infinity) {
        setBestSoFar(event.payload.best_so_far);
      }
      if (event.payload.eta_seconds > 0) {
        setEtaDisplay(formatEta(event.payload.eta_seconds));
      }
    });

    try {
      const strategy: Strategy = {
        id: currentStrategy.id ?? "",
        name: currentStrategy.name,
        created_at: currentStrategy.created_at ?? "",
        updated_at: currentStrategy.updated_at ?? "",
        long_entry_rules: currentStrategy.long_entry_rules,
        short_entry_rules: currentStrategy.short_entry_rules,
        long_exit_rules: currentStrategy.long_exit_rules,
        short_exit_rules: currentStrategy.short_exit_rules,
        position_sizing: currentStrategy.position_sizing,
        stop_loss: currentStrategy.stop_loss,
        take_profit: currentStrategy.take_profit,
        trailing_stop: currentStrategy.trailing_stop,
        trading_costs: currentStrategy.trading_costs,
        trade_direction: currentStrategy.trade_direction,
        trading_hours: currentStrategy.trading_hours,
        max_daily_trades: currentStrategy.max_daily_trades,
        close_trades_at: currentStrategy.close_trades_at,
      };

      const btConfig: BacktestConfig = {
        symbol_id: selectedSymbolId,
        timeframe: selectedTimeframe,
        start_date: backtestStartDate,
        end_date: backtestEndDate,
        initial_capital: initialCapital,
        leverage,
        precision: backtestPrecision,
      };

      const gaConfig: GeneticAlgorithmConfig | undefined =
        method === "GeneticAlgorithm"
          ? {
              population_size: populationSize,
              generations,
              mutation_rate: mutationRate,
              crossover_rate: crossoverRate,
            }
          : undefined;

      // Filter out OOS periods with empty dates
      const validOos = oosPeriods.filter((o) => o.start_date && o.end_date);

      const optConfig: OptimizationConfig = {
        method,
        parameter_ranges: parameterRanges,
        objectives,
        backtest_config: btConfig,
        ga_config: gaConfig,
        oos_periods: validOos,
      };

      const results = await runOptimization(strategy, optConfig);
      setOptimizationResults(results);
    } catch (err) {
      const msg = typeof err === "string" ? err : String(err);
      if (!msg.includes("Cancelled")) {
        setError(msg);
      }
    } finally {
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
      setLoading(false);
    }
  };

  const handleCancel = async () => {
    try {
      await cancelOptimization();
    } catch {
      // ignore
    }
  };

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-[11px] uppercase tracking-[0.15em]">Optimization Configuration</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Row 1: Method + Objective + Symbol + Timeframe + Precision */}
        <div className="grid grid-cols-2 gap-3 md:grid-cols-6">
          <div className="space-y-1">
            <label className="text-[10px] uppercase tracking-wider text-muted-foreground">Method</label>
            <Select
              value={method}
              onValueChange={(v) => setMethod(v as OptimizationMethod)}
            >
              <SelectTrigger className="h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="GridSearch">Grid Search</SelectItem>
                <SelectItem value="GeneticAlgorithm">
                  Genetic Algorithm
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1 md:col-span-2">
            <label className="text-[10px] uppercase tracking-wider text-muted-foreground">
              Objectives {objectives.length > 1 && <span className="text-primary">({objectives.length})</span>}
            </label>
            <div className="flex flex-wrap gap-1.5">
              {OBJECTIVE_OPTIONS.map((o) => {
                const selected = objectives.includes(o.value);
                return (
                  <button
                    key={o.value}
                    type="button"
                    onClick={() => {
                      if (selected) {
                        // Don't allow deselecting the last objective
                        if (objectives.length > 1) {
                          setObjectives(objectives.filter((v) => v !== o.value));
                        }
                      } else {
                        setObjectives([...objectives, o.value]);
                      }
                    }}
                    className={`rounded-md border px-2 py-1 text-[10px] font-medium transition-colors ${
                      selected
                        ? "border-primary bg-primary/15 text-primary"
                        : "border-border bg-card text-muted-foreground hover:border-primary/50"
                    }`}
                  >
                    {o.label}
                  </button>
                );
              })}
            </div>
          </div>

          <div className="space-y-1">
            <label className="text-[10px] uppercase tracking-wider text-muted-foreground">Symbol</label>
            <Select
              value={selectedSymbolId ?? ""}
              onValueChange={setSelectedSymbolId}
            >
              <SelectTrigger className="h-8 text-xs">
                <SelectValue placeholder="Select symbol" />
              </SelectTrigger>
              <SelectContent>
                {symbols.map((s) => (
                  <SelectItem key={s.id} value={s.id}>
                    {s.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1">
            <label className="text-[10px] uppercase tracking-wider text-muted-foreground">Timeframe</label>
            <Select
              value={selectedTimeframe}
              onValueChange={(v) => setSelectedTimeframe(v as Timeframe)}
            >
              <SelectTrigger className="h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {availableTimeframes.map((tf) => (
                  <SelectItem key={tf} value={tf}>
                    {TIMEFRAME_LABELS[tf] ?? tf}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-1">
            <label className="text-[10px] uppercase tracking-wider text-muted-foreground">Precision</label>
            <Select
              value={backtestPrecision}
              onValueChange={(v) => setBacktestPrecision(v as BacktestPrecision)}
            >
              <SelectTrigger className="h-8 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {availablePrecisions.map((p) => (
                  <SelectItem key={p} value={p}>
                    {PRECISION_LABELS[p]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>

        {/* Row 2: IS Dates + Capital + Leverage */}
        <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
          <div className="space-y-1">
            <label className="text-[10px] uppercase tracking-wider text-muted-foreground">IS Start</label>
            <DatePicker
              value={backtestStartDate.slice(0, 10)}
              onChange={(v) => setBacktestStartDate(v)}
            />
          </div>
          <div className="space-y-1">
            <label className="text-[10px] uppercase tracking-wider text-muted-foreground">IS End</label>
            <DatePicker
              value={backtestEndDate.slice(0, 10)}
              onChange={(v) => setBacktestEndDate(v)}
            />
          </div>
          <div className="space-y-1">
            <label className="text-[10px] uppercase tracking-wider text-muted-foreground">Capital ($)</label>
            <Input
              type="number"
              className="h-8 text-xs"
              min={0}
              step="any"
              value={initialCapital}
              onChange={(e) => setInitialCapital(Number(e.target.value))}
            />
          </div>
          <div className="space-y-1">
            <label className="text-[10px] uppercase tracking-wider text-muted-foreground">Leverage</label>
            <Input
              type="number"
              className="h-8 text-xs"
              min={1}
              step={1}
              value={leverage}
              onChange={(e) => setLeverage(Number(e.target.value))}
            />
          </div>
        </div>

        {/* OOS Periods */}
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <label className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
              Out-of-Sample Periods {oosPeriods.length > 0 && <span className="text-primary">({oosPeriods.length})</span>}
            </label>
            <button
              type="button"
              onClick={() => {
                const num = oosPeriods.length + 1;
                setOosPeriods([...oosPeriods, { label: `OOS ${num}`, start_date: "", end_date: "" }]);
              }}
              className="flex items-center gap-1 rounded px-2 py-1 text-[10px] font-medium text-primary transition-colors hover:bg-primary/10"
            >
              <Plus className="h-3 w-3" />
              Add OOS
            </button>
          </div>
          {oosPeriods.map((oos, idx) => (
            <div key={idx} className="grid grid-cols-[auto_1fr_1fr_auto] items-end gap-2">
              <span className="pb-1.5 text-[10px] font-medium text-muted-foreground">{oos.label}</span>
              <div className="space-y-1">
                <label className="text-[10px] uppercase tracking-wider text-muted-foreground">Start</label>
                <DatePicker
                  value={oos.start_date}
                  onChange={(v) => {
                    const updated = [...oosPeriods];
                    updated[idx] = { ...updated[idx], start_date: v };
                    setOosPeriods(updated);
                  }}
                />
              </div>
              <div className="space-y-1">
                <label className="text-[10px] uppercase tracking-wider text-muted-foreground">End</label>
                <DatePicker
                  value={oos.end_date}
                  onChange={(v) => {
                    const updated = [...oosPeriods];
                    updated[idx] = { ...updated[idx], end_date: v };
                    setOosPeriods(updated);
                  }}
                />
              </div>
              <button
                type="button"
                onClick={() => setOosPeriods(oosPeriods.filter((_, i) => i !== idx))}
                className="mb-0.5 rounded p-1.5 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            </div>
          ))}
        </div>

        {/* GA Config (only for Genetic Algorithm) */}
        {method === "GeneticAlgorithm" && (
          <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
            <div className="space-y-1">
              <label className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Population Size
              </label>
              <Input
                type="number"
                className="h-8 text-xs"
                min={10}
                step={10}
                value={populationSize}
                onChange={(e) => setPopulationSize(Number(e.target.value))}
              />
            </div>
            <div className="space-y-1">
              <label className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Generations
              </label>
              <Input
                type="number"
                className="h-8 text-xs"
                min={1}
                step={1}
                value={generations}
                onChange={(e) => setGenerations(Number(e.target.value))}
              />
            </div>
            <div className="space-y-1">
              <label className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Mutation Rate
              </label>
              <Input
                type="number"
                className="h-8 text-xs"
                min={0}
                max={1}
                step={0.01}
                value={mutationRate}
                onChange={(e) => setMutationRate(Number(e.target.value))}
              />
            </div>
            <div className="space-y-1">
              <label className="text-[10px] uppercase tracking-wider text-muted-foreground">
                Crossover Rate
              </label>
              <Input
                type="number"
                className="h-8 text-xs"
                min={0}
                max={1}
                step={0.01}
                value={crossoverRate}
                onChange={(e) => setCrossoverRate(Number(e.target.value))}
              />
            </div>
          </div>
        )}

        {/* Combination count warning for Grid Search */}
        {method === "GridSearch" && parameterRanges.length > 0 && (
          <div className="flex items-center gap-2 text-xs">
            <span className="text-muted-foreground">
              Combinations: {combinationCount.toLocaleString()}
            </span>
            {combinationCount > 500000 && (
              <span className="flex items-center gap-1 text-amber-500">
                <AlertTriangle className="h-3 w-3" />
                Exceeds 500,000 limit
              </span>
            )}
          </div>
        )}

        {/* Actions */}
        <div className="flex items-center gap-3">
          {!isLoading ? (
            <Button size="sm" onClick={handleRun} disabled={!canRun}>
              <Play className="mr-1.5 h-4 w-4" />
              Run Optimization
            </Button>
          ) : (
            <Button
              size="sm"
              variant="destructive"
              onClick={handleCancel}
            >
              <Square className="mr-1.5 h-4 w-4" />
              Cancel
            </Button>
          )}

          {isLoading && (
            <div className="flex flex-1 items-center gap-2">
              <Progress value={progressPercent} className="flex-1" />
              <span className="text-xs text-muted-foreground">
                {progressPercent}%
                {bestSoFar !== null && (
                  <> | Best: {bestSoFar.toFixed(2)}</>
                )}
                {etaDisplay && <> | ETA: {etaDisplay}</>}
              </span>
            </div>
          )}
        </div>

        {/* Error */}
        {error && (
          <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3">
            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
            <p className="text-xs text-destructive">{error}</p>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
