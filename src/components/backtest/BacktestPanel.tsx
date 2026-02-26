import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "@/stores/useAppStore";
import { runBacktest, cancelBacktest } from "@/lib/tauri";
import { sortTimeframes, PRECISION_LABELS } from "@/lib/types";
import type { BacktestConfig, BacktestPrecision, Strategy, Timeframe } from "@/lib/types";
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
import { Play, Square, AlertCircle } from "lucide-react";

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

export function BacktestPanel() {
  const { t } = useTranslation("backtest");
  const { t: tc } = useTranslation("common");
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
    backtestPrecision,
    setBacktestPrecision,
    currentStrategy,
    isLoading,
    setLoading,
    progressPercent,
    setProgress,
    setBacktestResults,
    setEquityMarkers,
  } = useAppStore();

  const [error, setError] = useState<string | null>(null);
  const [eta, setEta] = useState<string>("");
  const startTimeRef = useRef<number>(0);
  const unlistenRef = useRef<(() => void) | null>(null);

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
    (currentStrategy.long_entry_rules.length > 0 || currentStrategy.short_entry_rules.length > 0) &&
    !isLoading;

  // Ctrl+Enter shortcut listener
  useEffect(() => {
    const handler = () => {
      if (canRun) handleRun();
    };
    document.addEventListener("shortcut:run-backtest", handler);
    return () => document.removeEventListener("shortcut:run-backtest", handler);
  });

  const validate = (): string | null => {
    if (!selectedSymbolId) return t("selectSymbolFirst");
    if ((currentStrategy.long_entry_rules.length === 0 && currentStrategy.short_entry_rules.length === 0))
      return t("addEntryRule");
    if (initialCapital <= 0) return t("capitalPositive");
    if (
      backtestStartDate &&
      backtestEndDate &&
      backtestStartDate >= backtestEndDate
    )
      return t("dateOrder");
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
    setLoading(true, t("runningBacktest"));
    setBacktestResults(null);
    setEquityMarkers([]);
    setEta("");
    startTimeRef.current = Date.now();

    // Listen to progress events
    unlistenRef.current = await listen<{
      percent: number;
      current_bar: number;
      total_bars: number;
    }>("backtest-progress", (event) => {
      const pct = event.payload.percent;
      setProgress(pct);
      if (pct > 2) {
        const elapsed = (Date.now() - startTimeRef.current) / 1000;
        const remaining = (elapsed / pct) * (100 - pct);
        setEta(formatEta(remaining));
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

      const config: BacktestConfig = {
        symbol_id: selectedSymbolId,
        timeframe: selectedTimeframe,
        start_date: backtestStartDate,
        end_date: backtestEndDate,
        initial_capital: initialCapital,
        leverage,
        precision: backtestPrecision,
      };

      const results = await runBacktest(strategy, config);
      setBacktestResults(results);
    } catch (err) {
      const msg = typeof err === "string" ? err : err instanceof Error ? err.message : JSON.stringify(err);
      if (msg.includes("Cancelled") || msg.includes("cancelled") || msg.includes("cancel")) {
        setError(tc("stoppedByUser"));
      } else {
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
    <>
      {/* Actions bar */}
      <div className="flex items-center gap-3">
        {!isLoading ? (
          <Button size="sm" onClick={handleRun} disabled={!canRun}>
            <Play className="mr-1.5 h-4 w-4" />
            {t("runBacktest")}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="destructive"
            onClick={handleCancel}
          >
            <Square className="mr-1.5 h-4 w-4" />
            {tc("buttons.cancel")}
          </Button>
        )}

        {isLoading && (
          <div className="flex flex-1 items-center gap-2">
            <Progress value={progressPercent} className="flex-1" />
            <span className="whitespace-nowrap text-sm text-muted-foreground">
              {progressPercent}%{eta && <> | ETA: {eta}</>}
            </span>
          </div>
        )}
      </div>

      {/* Error */}
      {error && (
        <div className="flex items-start gap-2 rounded border border-destructive/50 bg-destructive/10 p-3">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
          <p className="text-sm text-destructive">{error}</p>
        </div>
      )}

      {/* Configuration card */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm">{t("config")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          {/* Row 1: Symbol + Timeframe + Precision */}
          <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
            <div className="space-y-1">
              <label className="text-sm text-muted-foreground">{t("symbol")}</label>
              <Select
                value={selectedSymbolId ?? ""}
                onValueChange={setSelectedSymbolId}
              >
                <SelectTrigger className="h-9 text-sm">
                  <SelectValue placeholder={t("selectSymbol")} />
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
              <label className="text-sm text-muted-foreground">{t("timeframe")}</label>
              <Select
                value={selectedTimeframe}
                onValueChange={(v) => setSelectedTimeframe(v as Timeframe)}
              >
                <SelectTrigger className="h-9 text-sm">
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
              <label className="text-sm text-muted-foreground">{t("precision")}</label>
              <Select
                value={backtestPrecision}
                onValueChange={(v) => setBacktestPrecision(v as BacktestPrecision)}
              >
                <SelectTrigger className="h-9 text-sm">
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

          {/* Row 2: Start Date + End Date + Capital */}
          <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
            <div className="space-y-1">
              <label className="text-sm text-muted-foreground">{t("startDate")}</label>
              <DatePicker
                value={backtestStartDate.slice(0, 10)}
                onChange={(v) => setBacktestStartDate(v)}
              />
            </div>

            <div className="space-y-1">
              <label className="text-sm text-muted-foreground">{t("endDate")}</label>
              <DatePicker
                value={backtestEndDate.slice(0, 10)}
                onChange={(v) => setBacktestEndDate(v)}
              />
            </div>

            <div className="space-y-1">
              <label className="text-sm text-muted-foreground">{t("capital")}</label>
              <Input
                type="number"
                className="h-9 text-sm"
                min={0}
                step="any"
                value={initialCapital}
                onChange={(e) => setInitialCapital(Number(e.target.value))}
              />
            </div>
          </div>
        </CardContent>
      </Card>
    </>
  );
}
