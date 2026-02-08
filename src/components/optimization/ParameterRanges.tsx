import { useState, useCallback, useEffect } from "react";
import { useAppStore } from "@/stores/useAppStore";
import type { ParameterRange, Rule } from "@/lib/types";
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
  ruleLabel: string;
  paramKey: string;
  paramLabel: string;
  defaultMin: number;
  defaultMax: number;
  defaultStep: number;
  currentValue: number;
}

/** Scan strategy rules and find all optimizable indicator parameters. */
function detectParams(entryRules: Rule[], exitRules: Rule[]): DetectedParam[] {
  const params: DetectedParam[] = [];
  const allRules = [
    ...entryRules.map((r, i) => ({ rule: r, index: i, section: "Entry" })),
    ...exitRules.map((r, i) => ({
      rule: r,
      index: i + entryRules.length,
      section: "Exit",
    })),
  ];

  for (const { rule, index, section } of allRules) {
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
          ruleIndex: index,
          ruleLabel: `${section} #${index + 1} (${side}) — ${ind.indicator_type}`,
          paramKey: field.key,
          paramLabel: field.label,
          defaultMin: Math.max(field.min, 1),
          defaultMax: Math.min(field.max, currentValue * 3),
          defaultStep: field.step,
          currentValue,
        });
      }
    }
  }

  return params;
}

interface ParameterRangesProps {
  onChange: (ranges: ParameterRange[]) => void;
}

export function ParameterRanges({ onChange }: ParameterRangesProps) {
  const { currentStrategy } = useAppStore();

  const detected = detectParams(
    currentStrategy.entry_rules,
    currentStrategy.exit_rules
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
      const key = `${p.ruleIndex}_${p.paramKey}`;
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
      const key = `${p.ruleIndex}_${p.paramKey}`;
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
          });
        }
      }
    }
    onChange(result);
  }, [enabled, ranges, detected, onChange]);

  useEffect(() => {
    notifyChange();
  }, [enabled, ranges]);

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
      <p className="py-4 text-center text-sm text-muted-foreground">
        No optimizable parameters found. Add indicator-based rules to your
        strategy first.
      </p>
    );
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead className="w-10">On</TableHead>
          <TableHead>Parameter</TableHead>
          <TableHead className="w-20">Current</TableHead>
          <TableHead className="w-20">Min</TableHead>
          <TableHead className="w-20">Max</TableHead>
          <TableHead className="w-20">Step</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {detected.map((p) => {
          const key = `${p.ruleIndex}_${p.paramKey}`;
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
