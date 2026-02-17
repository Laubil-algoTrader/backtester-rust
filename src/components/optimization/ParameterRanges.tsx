import { useState, useCallback, useEffect, useMemo } from "react";
import { useAppStore } from "@/stores/useAppStore";
import type { ParamSource, ParameterRange, Strategy } from "@/lib/types";
import { getIndicatorParamFields } from "@/components/strategy/utils";
import { Input } from "@/components/ui/Input";
import {
  Table,
  TableHeader,
  TableBody,
  TableRow,
  TableHead,
  TableCell,
} from "@/components/ui/Table";

interface DetectedParam {
  ruleIndex: number;
  operandSide: "left" | "right";
  ruleLabel: string;
  paramKey: string;
  paramLabel: string;
  defaultMin: number;
  defaultMax: number;
  defaultStep: number;
  currentValue: number;
  paramSource: ParamSource;
}

/** Scan strategy rules and find all optimizable indicator parameters + SL/TP/TS + time filters. */
function detectParams(strategy: Pick<Strategy, "long_entry_rules" | "short_entry_rules" | "long_exit_rules" | "short_exit_rules" | "stop_loss" | "take_profit" | "trailing_stop" | "trading_hours" | "close_trades_at">): DetectedParam[] {
  const params: DetectedParam[] = [];

  const ruleGroups: { rules: typeof strategy.long_entry_rules; label: string; source: ParamSource }[] = [
    { rules: strategy.long_entry_rules, label: "Long Entry", source: "long_entry" },
    { rules: strategy.short_entry_rules, label: "Short Entry", source: "short_entry" },
    { rules: strategy.long_exit_rules, label: "Long Exit", source: "long_exit" },
    { rules: strategy.short_exit_rules, label: "Short Exit", source: "short_exit" },
  ];

  // Detect indicator params from rules
  for (const group of ruleGroups) {
    for (let i = 0; i < group.rules.length; i++) {
      const rule = group.rules[i];
      for (const side of ["left", "right"] as const) {
        const operand =
          side === "left" ? rule.left_operand : rule.right_operand;
        if (operand.operand_type !== "Indicator" || !operand.indicator) continue;

        const ind = operand.indicator;
        const fields = getIndicatorParamFields(ind.indicator_type);

        for (const field of fields) {
          const currentValue =
            (ind.params as Record<string, number | undefined>)[field.key] ??
            field.defaultValue;
          params.push({
            ruleIndex: i,
            operandSide: side,
            ruleLabel: `${group.label} #${i + 1} (${side}) — ${ind.indicator_type}`,
            paramKey: field.key,
            paramLabel: field.label,
            defaultMin: Math.max(field.min, 1),
            defaultMax: Math.min(field.max, currentValue * 3),
            defaultStep: field.step,
            currentValue,
            paramSource: group.source,
          });
        }
      }
    }
  }

  // Detect Stop Loss params
  if (strategy.stop_loss) {
    const sl = strategy.stop_loss;
    params.push({
      ruleIndex: -1,
      operandSide: "left",
      ruleLabel: `Stop Loss (${sl.sl_type})`,
      paramKey: "value",
      paramLabel: "Value",
      defaultMin: sl.sl_type === "Percentage" ? 0.1 : sl.sl_type === "ATR" ? 0.5 : 1,
      defaultMax: Math.max(sl.value * 3, sl.sl_type === "Percentage" ? 5 : 100),
      defaultStep: sl.sl_type === "Percentage" ? 0.1 : sl.sl_type === "ATR" ? 0.1 : 1,
      currentValue: sl.value,
      paramSource: "stop_loss",
    });
    if (sl.sl_type === "ATR" && sl.atr_period) {
      params.push({
        ruleIndex: -1,
        operandSide: "left",
        ruleLabel: "Stop Loss (ATR)",
        paramKey: "atr_period",
        paramLabel: "ATR Period",
        defaultMin: 5,
        defaultMax: Math.max(sl.atr_period * 3, 50),
        defaultStep: 1,
        currentValue: sl.atr_period,
        paramSource: "stop_loss",
      });
    }
  }

  // Detect Take Profit params
  if (strategy.take_profit) {
    const tp = strategy.take_profit;
    params.push({
      ruleIndex: -1,
      operandSide: "left",
      ruleLabel: `Take Profit (${tp.tp_type})`,
      paramKey: "value",
      paramLabel: "Value",
      defaultMin: tp.tp_type === "RiskReward" ? 0.5 : tp.tp_type === "ATR" ? 0.5 : 1,
      defaultMax: Math.max(tp.value * 3, tp.tp_type === "RiskReward" ? 10 : 100),
      defaultStep: tp.tp_type === "RiskReward" ? 0.5 : tp.tp_type === "ATR" ? 0.1 : 1,
      currentValue: tp.value,
      paramSource: "take_profit",
    });
    if (tp.tp_type === "ATR" && tp.atr_period) {
      params.push({
        ruleIndex: -1,
        operandSide: "left",
        ruleLabel: "Take Profit (ATR)",
        paramKey: "atr_period",
        paramLabel: "ATR Period",
        defaultMin: 5,
        defaultMax: Math.max(tp.atr_period * 3, 50),
        defaultStep: 1,
        currentValue: tp.atr_period,
        paramSource: "take_profit",
      });
    }
  }

  // Detect Trailing Stop params
  if (strategy.trailing_stop) {
    const ts = strategy.trailing_stop;
    params.push({
      ruleIndex: -1,
      operandSide: "left",
      ruleLabel: `Trailing Stop (${ts.ts_type})`,
      paramKey: "value",
      paramLabel: "Value",
      defaultMin: ts.ts_type === "RiskReward" ? 0.5 : 0.5,
      defaultMax: Math.max(ts.value * 3, ts.ts_type === "RiskReward" ? 10 : 10),
      defaultStep: ts.ts_type === "RiskReward" ? 0.5 : 0.1,
      currentValue: ts.value,
      paramSource: "trailing_stop",
    });
    if (ts.ts_type === "ATR" && ts.atr_period) {
      params.push({
        ruleIndex: -1,
        operandSide: "left",
        ruleLabel: "Trailing Stop (ATR)",
        paramKey: "atr_period",
        paramLabel: "ATR Period",
        defaultMin: 5,
        defaultMax: Math.max(ts.atr_period * 3, 50),
        defaultStep: 1,
        currentValue: ts.atr_period,
        paramSource: "trailing_stop",
      });
    }
  }

  // Detect Trading Hours params
  if (strategy.trading_hours) {
    const th = strategy.trading_hours;
    params.push(
      {
        ruleIndex: -1, operandSide: "left",
        ruleLabel: "Trading Hours", paramKey: "start_hour", paramLabel: "Start Hour",
        defaultMin: 0, defaultMax: 23, defaultStep: 1,
        currentValue: th.start_hour, paramSource: "trading_hours",
      },
      {
        ruleIndex: -1, operandSide: "left",
        ruleLabel: "Trading Hours", paramKey: "start_minute", paramLabel: "Start Minute",
        defaultMin: 0, defaultMax: 59, defaultStep: 15,
        currentValue: th.start_minute, paramSource: "trading_hours",
      },
      {
        ruleIndex: -1, operandSide: "left",
        ruleLabel: "Trading Hours", paramKey: "end_hour", paramLabel: "End Hour",
        defaultMin: 0, defaultMax: 23, defaultStep: 1,
        currentValue: th.end_hour, paramSource: "trading_hours",
      },
      {
        ruleIndex: -1, operandSide: "left",
        ruleLabel: "Trading Hours", paramKey: "end_minute", paramLabel: "End Minute",
        defaultMin: 0, defaultMax: 59, defaultStep: 15,
        currentValue: th.end_minute, paramSource: "trading_hours",
      },
    );
  }

  // Detect Close Trades At params
  if (strategy.close_trades_at) {
    const ct = strategy.close_trades_at;
    params.push(
      {
        ruleIndex: -1, operandSide: "left",
        ruleLabel: "Close Trades At", paramKey: "hour", paramLabel: "Hour",
        defaultMin: 0, defaultMax: 23, defaultStep: 1,
        currentValue: ct.hour, paramSource: "close_trades_at",
      },
      {
        ruleIndex: -1, operandSide: "left",
        ruleLabel: "Close Trades At", paramKey: "minute", paramLabel: "Minute",
        defaultMin: 0, defaultMax: 59, defaultStep: 15,
        currentValue: ct.minute, paramSource: "close_trades_at",
      },
    );
  }

  return params;
}

function paramKey(p: DetectedParam): string {
  if (p.paramSource === "stop_loss" || p.paramSource === "take_profit" || p.paramSource === "trailing_stop"
    || p.paramSource === "trading_hours" || p.paramSource === "close_trades_at") {
    return `${p.paramSource}_${p.paramKey}`;
  }
  return `${p.paramSource}_${p.ruleIndex}_${p.operandSide}_${p.paramKey}`;
}

interface ParameterRangesProps {
  onChange: (ranges: ParameterRange[]) => void;
}

export function ParameterRanges({ onChange }: ParameterRangesProps) {
  const { currentStrategy } = useAppStore();

  const detected = useMemo(
    () => detectParams(currentStrategy),
    [
      currentStrategy.long_entry_rules,
      currentStrategy.short_entry_rules,
      currentStrategy.long_exit_rules,
      currentStrategy.short_exit_rules,
      currentStrategy.stop_loss,
      currentStrategy.take_profit,
      currentStrategy.trailing_stop,
      currentStrategy.trading_hours,
      currentStrategy.close_trades_at,
    ],
  );

  // Track enabled state and ranges per parameter
  const [enabled, setEnabled] = useState<Record<string, boolean>>({});
  const [ranges, setRanges] = useState<
    Record<string, { min: number; max: number; step: number }>
  >({});

  // Initialize defaults when detected params change
  useEffect(() => {
    const newRanges: Record<string, { min: number; max: number; step: number }> = {};
    for (const p of detected) {
      const key = paramKey(p);
      if (!ranges[key]) {
        newRanges[key] = {
          min: p.defaultMin,
          max: p.defaultMax,
          step: p.defaultStep,
        };
      }
    }
    if (Object.keys(newRanges).length > 0) {
      setRanges((prev) => ({ ...prev, ...newRanges }));
    }
  }, [detected.length]);

  // Notify parent when enabled/ranges change
  const notifyChange = useCallback(() => {
    const result: ParameterRange[] = [];
    for (const p of detected) {
      const key = paramKey(p);
      if (enabled[key]) {
        const r = ranges[key];
        if (r) {
          result.push({
            rule_index: p.ruleIndex,
            param_name: p.paramKey,
            display_name: `${p.ruleLabel} — ${p.paramLabel}`,
            min: r.min,
            max: r.max,
            step: r.step,
            operand_side: p.operandSide,
            param_source: p.paramSource,
          });
        }
      }
    }
    onChange(result);
  }, [enabled, ranges, detected, onChange]);

  useEffect(() => {
    notifyChange();
  }, [notifyChange]);

  const toggleEnabled = (key: string) => {
    setEnabled((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  const updateRange = (
    key: string,
    field: "min" | "max" | "step",
    value: number
  ) => {
    setRanges((prev) => ({
      ...prev,
      [key]: { ...prev[key], [field]: value },
    }));
  };

  if (detected.length === 0) {
    return (
      <p className="py-4 text-center text-xs uppercase tracking-wider text-muted-foreground">
        No optimizable parameters found. Add indicator-based rules or
        configure stop loss / take profit / trailing stop first.
      </p>
    );
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead className="w-10 text-[10px] tracking-widest">ON</TableHead>
          <TableHead className="text-[10px] tracking-widest">PARAMETER</TableHead>
          <TableHead className="w-20 text-[10px] tracking-widest">CURRENT</TableHead>
          <TableHead className="w-24 text-[10px] tracking-widest">MIN</TableHead>
          <TableHead className="w-24 text-[10px] tracking-widest">MAX</TableHead>
          <TableHead className="w-24 text-[10px] tracking-widest">STEP</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {detected.map((p) => {
          const key = paramKey(p);
          const isEnabled = !!enabled[key];
          const r = ranges[key] ?? {
            min: p.defaultMin,
            max: p.defaultMax,
            step: p.defaultStep,
          };
          return (
            <TableRow key={key} className={isEnabled ? "" : "opacity-50"}>
              <TableCell>
                <input
                  type="checkbox"
                  checked={isEnabled}
                  onChange={() => toggleEnabled(key)}
                  className="h-4 w-4 accent-primary"
                />
              </TableCell>
              <TableCell className="text-xs">
                <span className="font-medium">{p.paramLabel}</span>
                <span className="ml-1 text-muted-foreground">
                  ({p.ruleLabel})
                </span>
              </TableCell>
              <TableCell className="text-xs text-muted-foreground">
                {p.currentValue}
              </TableCell>
              <TableCell>
                <Input
                  type="number"
                  className="h-7 w-full text-xs"
                  value={r.min}
                  onChange={(e) =>
                    updateRange(key, "min", Number(e.target.value))
                  }
                  disabled={!isEnabled}
                />
              </TableCell>
              <TableCell>
                <Input
                  type="number"
                  className="h-7 w-full text-xs"
                  value={r.max}
                  onChange={(e) =>
                    updateRange(key, "max", Number(e.target.value))
                  }
                  disabled={!isEnabled}
                />
              </TableCell>
              <TableCell>
                <Input
                  type="number"
                  className="h-7 w-full text-xs"
                  value={r.step}
                  step="any"
                  onChange={(e) =>
                    updateRange(key, "step", Number(e.target.value))
                  }
                  disabled={!isEnabled}
                />
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
