import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "@/stores/useAppStore";
import { runBacktest, cancelBacktest } from "@/lib/tauri";
import type { BacktestConfig, Strategy, Timeframe } from "@/lib/types";
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
import { Play, Square, AlertCircle } from "lucide-react";

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

export function BacktestPanel() {
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
    currentStrategy,
    isLoading,
    setLoading,
    progressPercent,
    setProgress,
    setBacktestResults,
  } = useAppStore();

  const [error, setError] = useState<string | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);

  const selectedSymbol = symbols.find((s) => s.id === selectedSymbolId);
  const availableTimeframes = selectedSymbol
    ? Object.keys(selectedSymbol.timeframe_paths)
    : [];

  // Auto-fill dates when symbol changes
  useEffect(() => {
    if (selectedSymbol) {
      if (!backtestStartDate) setBacktestStartDate(selectedSymbol.start_date);
      if (!backtestEndDate) setBacktestEndDate(selectedSymbol.end_date);
    }
  }, [selectedSymbolId]);

  // Cleanup event listener on unmount
  useEffect(() => {
    return () => {
      if (unlistenRef.current) unlistenRef.current();
    };
  }, []);

  const canRun =
    selectedSymbolId &&
    currentStrategy.entry_rules.length > 0 &&
    !isLoading;

  const handleRun = async () => {
    if (!selectedSymbolId) return;
    setError(null);
    setLoading(true, "Running backtest...");
    setBacktestResults(null);

    // Listen to progress events
    unlistenRef.current = await listen<{
      percent: number;
      current_bar: number;
      total_bars: number;
    }>("backtest-progress", (event) => {
      setProgress(event.payload.percent);
    });

    try {
      const strategy: Strategy = {
        id: currentStrategy.id ?? "",
        name: currentStrategy.name,
        created_at: currentStrategy.created_at ?? "",
        updated_at: currentStrategy.updated_at ?? "",
        entry_rules: currentStrategy.entry_rules,
        exit_rules: currentStrategy.exit_rules,
        position_sizing: currentStrategy.position_sizing,
        stop_loss: currentStrategy.stop_loss,
        take_profit: currentStrategy.take_profit,
        trailing_stop: currentStrategy.trailing_stop,
        trading_costs: currentStrategy.trading_costs,
        trade_direction: currentStrategy.trade_direction,
      };

      const config: BacktestConfig = {
        symbol_id: selectedSymbolId,
        timeframe: selectedTimeframe,
        start_date: backtestStartDate,
        end_date: backtestEndDate,
        initial_capital: initialCapital,
        leverage,
      };

      const results = await runBacktest(strategy, config);
      setBacktestResults(results);
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
      await cancelBacktest();
    } catch {
      // ignore
    }
  };

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="text-base">Backtest Configuration</CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid grid-cols-2 gap-3 md:grid-cols-3 lg:grid-cols-6">
          {/* Symbol */}
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">Symbol</label>
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

          {/* Timeframe */}
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">Timeframe</label>
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

          {/* Start Date */}
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">Start Date</label>
            <Input
              type="date"
              className="h-8 text-xs"
              value={backtestStartDate.slice(0, 10)}
              onChange={(e) => setBacktestStartDate(e.target.value)}
            />
          </div>

          {/* End Date */}
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">End Date</label>
            <Input
              type="date"
              className="h-8 text-xs"
              value={backtestEndDate.slice(0, 10)}
              onChange={(e) => setBacktestEndDate(e.target.value)}
            />
          </div>

          {/* Capital */}
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">Capital ($)</label>
            <Input
              type="number"
              className="h-8 text-xs"
              min={0}
              step="any"
              value={initialCapital}
              onChange={(e) => setInitialCapital(Number(e.target.value))}
            />
          </div>

          {/* Leverage */}
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">Leverage</label>
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

        {/* Actions */}
        <div className="flex items-center gap-3">
          {!isLoading ? (
            <Button size="sm" onClick={handleRun} disabled={!canRun}>
              <Play className="mr-1.5 h-4 w-4" />
              Run Backtest
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
