import { useState, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/useAppStore";
import { runBacktest } from "@/lib/tauri";
import type { ParameterRange, Strategy, BacktestConfig } from "@/lib/types";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";
import { OptimizerPanel } from "./OptimizerPanel";
import { ParameterRanges } from "./ParameterRanges";
import { ResultsTable } from "./ResultsTable";
import { ProGate } from "@/components/auth/ProGate";

export function OptimizationPage() {
  return (
    <ProGate feature="optimization">
      <OptimizationPageContent />
    </ProGate>
  );
}

function OptimizationPageContent() {
  const { t } = useTranslation("optimization");
  const {
    optimizationResults,
    currentStrategy,
    setLongEntryRules,
    setShortEntryRules,
    setLongExitRules,
    setShortExitRules,
    setStopLoss,
    setTakeProfit,
    setTrailingStop,
    setTradingHours,
    setCloseTradesAt,
    setActiveSection,
    setBacktestResults,
    setEquityMarkers,
    setLoading,
    selectedSymbolId,
    selectedTimeframe,
    backtestStartDate,
    backtestEndDate,
    initialCapital,
    leverage,
    backtestPrecision,
    optimizationOosPeriods,
    optimizationParamRanges,
    setOptimizationParamRanges,
  } = useAppStore();

  const [applyError, setApplyError] = useState<string | null>(null);

  const [parameterRanges, setParameterRanges] = useState<ParameterRange[]>(optimizationParamRanges);
  // Keep a ref always in sync so handleApplyParams never has stale ranges
  const rangesRef = useRef<ParameterRange[]>(parameterRanges);
  rangesRef.current = parameterRanges;

  const handleRangesChange = useCallback((ranges: ParameterRange[]) => {
    setParameterRanges(ranges);
    setOptimizationParamRanges(ranges);
  }, [setOptimizationParamRanges]);

  /** Apply optimization result params back into the current strategy, then auto-run backtest. */
  const handleApplyParams = useCallback(
    async (params: Record<string, number>) => {
      // Read from ref to avoid stale closure issues
      const ranges = rangesRef.current;

      // Deep-copy rules so mutations don't affect shared references
      const longEntry = JSON.parse(JSON.stringify(currentStrategy.long_entry_rules));
      const shortEntry = JSON.parse(JSON.stringify(currentStrategy.short_entry_rules));
      const longExit = JSON.parse(JSON.stringify(currentStrategy.long_exit_rules));
      const shortExit = JSON.parse(JSON.stringify(currentStrategy.short_exit_rules));
      let sl = currentStrategy.stop_loss ? { ...currentStrategy.stop_loss } : undefined;
      let tp = currentStrategy.take_profit ? { ...currentStrategy.take_profit } : undefined;
      let ts = currentStrategy.trailing_stop ? { ...currentStrategy.trailing_stop } : undefined;
      let th = currentStrategy.trading_hours ? { ...currentStrategy.trading_hours } : undefined;
      let ct = currentStrategy.close_trades_at ? { ...currentStrategy.close_trades_at } : undefined;

      for (const range of ranges) {
        const value = params[range.display_name];
        if (value === undefined) continue;

        if (range.param_source === "stop_loss" && sl) {
          if (range.param_name === "value") sl.value = value;
          else if (range.param_name === "atr_period") sl.atr_period = Math.round(value);
        } else if (range.param_source === "take_profit" && tp) {
          if (range.param_name === "value") tp.value = value;
          else if (range.param_name === "atr_period") tp.atr_period = Math.round(value);
        } else if (range.param_source === "trailing_stop" && ts) {
          if (range.param_name === "value") ts.value = value;
          else if (range.param_name === "atr_period") ts.atr_period = Math.round(value);
        } else if (range.param_source === "trading_hours" && th) {
          const v = Math.round(value);
          if (range.param_name === "start_hour") th.start_hour = v;
          else if (range.param_name === "start_minute") th.start_minute = v;
          else if (range.param_name === "end_hour") th.end_hour = v;
          else if (range.param_name === "end_minute") th.end_minute = v;
        } else if (range.param_source === "close_trades_at" && ct) {
          const v = Math.round(value);
          if (range.param_name === "hour") ct.hour = v;
          else if (range.param_name === "minute") ct.minute = v;
        } else {
          // Indicator parameter — select the correct rule group
          const rules =
            range.param_source === "long_entry" ? longEntry :
            range.param_source === "short_entry" ? shortEntry :
            range.param_source === "long_exit" ? longExit :
            shortExit;
          const idx = range.rule_index;
          if (!rules[idx]) continue;

          const side = range.operand_side === "right" ? "right_operand" : "left_operand";
          const operand = rules[idx][side];
          if (range.param_name === "constant_value") {
            // Constant (nivel) optimization
            operand.constant_value = value;
          } else if (operand.operand_type === "Indicator" && operand.indicator) {
            const isInt = ["period", "fast_period", "slow_period", "signal_period", "k_period", "d_period"].includes(range.param_name);
            (operand.indicator.params as Record<string, number>)[range.param_name] = isInt ? Math.round(value) : value;
          }
        }
      }

      setLongEntryRules(longEntry);
      setShortEntryRules(shortEntry);
      setLongExitRules(longExit);
      setShortExitRules(shortExit);
      setStopLoss(sl);
      setTakeProfit(tp);
      setTrailingStop(ts);
      setTradingHours(th);
      setCloseTradesAt(ct);

      // Auto-run backtest with applied params on the FULL date range (IS + OOS)
      if (!selectedSymbolId) return;
      setApplyError(null);
      setLoading(true, t("runningWithParams"));
      setBacktestResults(null);

      // Compute full date range: IS start → last OOS end (or IS end if no OOS)
      const validOos = optimizationOosPeriods.filter((o) => o.start_date && o.end_date);
      let fullEndDate = backtestEndDate;
      for (const oos of validOos) {
        if (oos.end_date > fullEndDate) fullEndDate = oos.end_date;
      }

      // Build equity markers at IS/OOS boundaries
      const markers: { date: string; label: string }[] = [];
      if (validOos.length > 0) {
        markers.push({ date: backtestEndDate, label: "IS End" });
        for (const oos of validOos) {
          markers.push({ date: oos.end_date, label: `${oos.label} End` });
        }
      }
      setEquityMarkers(markers);

      try {
        const strategy: Strategy = {
          id: currentStrategy.id ?? "",
          name: currentStrategy.name,
          created_at: currentStrategy.created_at ?? "",
          updated_at: currentStrategy.updated_at ?? "",
          long_entry_rules: longEntry,
          short_entry_rules: shortEntry,
          long_exit_rules: longExit,
          short_exit_rules: shortExit,
          position_sizing: currentStrategy.position_sizing,
          stop_loss: sl,
          take_profit: tp,
          trailing_stop: ts,
          trading_costs: currentStrategy.trading_costs,
          trade_direction: currentStrategy.trade_direction,
          trading_hours: th,
          max_daily_trades: currentStrategy.max_daily_trades,
          close_trades_at: ct,
        };

        const config: BacktestConfig = {
          symbol_id: selectedSymbolId,
          timeframe: selectedTimeframe,
          start_date: backtestStartDate,
          end_date: fullEndDate,
          initial_capital: initialCapital,
          leverage,
          precision: backtestPrecision,
        };

        const results = await runBacktest(strategy, config);
        setBacktestResults(results);
        setActiveSection("backtest");
      } catch (err) {
        const msg = typeof err === "string" ? err : String(err);
        setApplyError(msg);
      } finally {
        setLoading(false);
      }
    },
    [currentStrategy, setLongEntryRules, setShortEntryRules, setLongExitRules, setShortExitRules, setStopLoss, setTakeProfit, setTrailingStop, setTradingHours, setCloseTradesAt, selectedSymbolId, selectedTimeframe, backtestStartDate, backtestEndDate, initialCapital, leverage, backtestPrecision, setActiveSection, setBacktestResults, setEquityMarkers, setLoading, optimizationOosPeriods]
  );

  return (
    <div className="mx-auto max-w-[1400px] space-y-4">
      <h2 className="text-2xl font-bold text-foreground">{t("title")}</h2>

      {/* Configuration panel (actions bar + 2-column cards) */}
      <OptimizerPanel parameterRanges={parameterRanges} />

      {/* Parameter ranges selection */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-sm">{t("parameterRanges")}</CardTitle>
        </CardHeader>
        <CardContent>
          <ParameterRanges onChange={handleRangesChange} />
        </CardContent>
      </Card>

      {/* Error from auto-backtest */}
      {applyError && (
        <div className="rounded border border-destructive/50 bg-destructive/10 p-3">
          <p className="text-sm text-destructive">{applyError}</p>
        </div>
      )}

      {/* Results (only shown when there are results) */}
      {optimizationResults.length > 0 && (
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-sm">
              {t("topResults")} ({optimizationResults.length})
            </CardTitle>
          </CardHeader>
          <CardContent>
            <ResultsTable
              results={optimizationResults}
              parameterRanges={parameterRanges}
              onApply={handleApplyParams}
              isMultiObjective={
                optimizationResults.length > 0 &&
                optimizationResults[0].composite_score !== optimizationResults[0].objective_value
              }
            />
          </CardContent>
        </Card>
      )}
    </div>
  );
}
