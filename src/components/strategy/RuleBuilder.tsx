import type { Rule, Comparator, LogicalOperator } from "@/lib/types";
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
  return (
    <div className="space-y-2 rounded-md border border-border p-3">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1 space-y-2">
          {/* Left operand */}
          <OperandSelector
            value={rule.left_operand}
            onChange={(left_operand) => onChange({ ...rule, left_operand })}
          />

          {/* Comparator */}
          <div className="flex items-center gap-2">
            <Select
              value={rule.comparator}
              onValueChange={(c) =>
                onChange({ ...rule, comparator: c as Comparator })
              }
            >
              <SelectTrigger className="h-8 w-[130px] text-xs">
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
          </div>

          {/* Right operand */}
          <OperandSelector
            value={rule.right_operand}
            onChange={(right_operand) => onChange({ ...rule, right_operand })}
          />
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
            className="h-6 px-3 text-xs font-semibold"
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
