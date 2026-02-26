import { useTranslation } from "react-i18next";
import type { Rule, Comparator, LogicalOperator, Operand, TimeField } from "@/lib/types";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/Select";
import { Button } from "@/components/ui/Button";
import { Trash2 } from "lucide-react";
import { OperandSelector } from "./OperandSelector";
import { COMPARATOR_OPTIONS } from "./utils";

const TIME_CONTEXT_FIELDS: Set<TimeField> = new Set([
  "CurrentMonth",
  "BarDayOfWeek", "CurrentDayOfWeek",
  "BarHour", "CurrentHour",
  "BarMinute", "CurrentMinute",
  "BarTimeValue", "CurrentTime",
]);

function getDefaultConstantForTimeField(field?: TimeField): number {
  if (!field) return 0;
  if (field === "CurrentMonth") return 1;
  return 0;
}

interface RuleBuilderProps {
  rule: Rule;
  onChange: (rule: Rule) => void;
  onDelete: () => void;
  showLogicalOp: boolean;
}

export function RuleBuilder({
  rule,
  onChange,
  onDelete,
  showLogicalOp,
}: RuleBuilderProps) {
  const { t } = useTranslation("strategy");

  const handleLeftOperandChange = (left_operand: Operand) => {
    let right_operand = rule.right_operand;
    let comparator = rule.comparator;

    // Auto-switch right operand to Constant when left is BarTime with a time context
    if (
      left_operand.operand_type === "BarTime" &&
      left_operand.time_field &&
      TIME_CONTEXT_FIELDS.has(left_operand.time_field) &&
      right_operand.operand_type !== "Constant"
    ) {
      right_operand = {
        operand_type: "Constant",
        constant_value: getDefaultConstantForTimeField(left_operand.time_field),
      };
    }

    // Auto-switch right operand to Constant(1) and comparator to Equal when left is CandlePattern
    // Patterns output 0.0 or 1.0, so "Pattern == 1" is the natural rule.
    if (
      left_operand.operand_type === "CandlePattern" &&
      right_operand.operand_type !== "Constant"
    ) {
      right_operand = {
        operand_type: "Constant",
        constant_value: 1,
      };
      comparator = "Equal" as Comparator;
    }

    onChange({ ...rule, left_operand, right_operand, comparator });
  };

  const isCandlePattern = rule.left_operand.operand_type === "CandlePattern";

  return (
    <div className="space-y-2 rounded border border-border/60 p-3">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1 space-y-2">
          {/* Left operand */}
          <OperandSelector
            value={rule.left_operand}
            onChange={handleLeftOperandChange}
          />

          {/* Comparator — locked to Equal for candle patterns */}
          <div className="flex items-center gap-2">
            {isCandlePattern ? (
              <div className="flex h-8 w-[130px] items-center rounded-md border border-border/60 bg-muted/50 px-3 text-sm text-muted-foreground">
                == (Equal)
              </div>
            ) : (
              <Select
                value={rule.comparator}
                onValueChange={(c) =>
                  onChange({ ...rule, comparator: c as Comparator })
                }
              >
                <SelectTrigger className="h-8 w-[130px] text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {COMPARATOR_OPTIONS.map((opt) => (
                    <SelectItem key={opt.value} value={opt.value}>
                      {opt.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </div>

          {/* Right operand — locked to Constant(1) for candle patterns */}
          {isCandlePattern ? (
            <div className="flex h-8 items-center rounded-md border border-border/60 bg-muted/50 px-3 text-sm text-muted-foreground">
              1 ({t("patternDetected")})
            </div>
          ) : (
            <OperandSelector
              value={rule.right_operand}
              onChange={(right_operand) => onChange({ ...rule, right_operand })}
              contextTimeField={
                rule.left_operand.operand_type === "BarTime"
                  ? rule.left_operand.time_field
                  : undefined
              }
            />
          )}
        </div>

        {/* Delete button */}
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8 shrink-0 text-muted-foreground hover:text-destructive"
          onClick={onDelete}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>

      {/* Logical operator connector */}
      {showLogicalOp && (
        <div className="flex justify-center pt-1">
          <Button
            variant="outline"
            size="sm"
            className="h-6 px-3 text-sm font-semibold"
            onClick={() =>
              onChange({
                ...rule,
                logical_operator:
                  rule.logical_operator === "OR" ? "AND" : ("OR" as LogicalOperator),
              })
            }
          >
            {rule.logical_operator ?? "AND"}
          </Button>
        </div>
      )}
    </div>
  );
}
