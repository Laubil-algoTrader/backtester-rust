import type { Operand, OperandType, PriceField } from "@/lib/types";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/Select";
import { Input } from "@/components/ui/Input";
import { IndicatorSelector } from "./IndicatorSelector";
import { createDefaultIndicatorConfig } from "./utils";

interface OperandSelectorProps {
  value: Operand;
  onChange: (operand: Operand) => void;
}

const OPERAND_TYPE_OPTIONS: { value: OperandType; label: string }[] = [
  { value: "Indicator", label: "Indicator" },
  { value: "Price", label: "Price" },
  { value: "Constant", label: "Constant" },
];

const PRICE_FIELD_OPTIONS: { value: PriceField; label: string }[] = [
  { value: "Open", label: "Open" },
  { value: "High", label: "High" },
  { value: "Low", label: "Low" },
  { value: "Close", label: "Close" },
];

export function OperandSelector({ value, onChange }: OperandSelectorProps) {
  const handleTypeChange = (type: string) => {
    const operandType = type as OperandType;
    if (operandType === "Indicator") {
      onChange({
        operand_type: "Indicator",
        indicator: createDefaultIndicatorConfig("SMA"),
        offset: value.offset,
      });
    } else if (operandType === "Price") {
      onChange({
        operand_type: "Price",
        price_field: "Close",
        offset: value.offset,
      });
    } else {
      onChange({
        operand_type: "Constant",
        constant_value: 0,
      });
    }
  };

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <Select value={value.operand_type} onValueChange={handleTypeChange}>
        <SelectTrigger className="h-8 w-[100px] text-xs">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {OPERAND_TYPE_OPTIONS.map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {value.operand_type === "Indicator" && value.indicator && (
        <IndicatorSelector
          value={value.indicator}
          onChange={(indicator) => onChange({ ...value, indicator })}
        />
      )}

      {value.operand_type === "Price" && (
        <Select
          value={value.price_field ?? "Close"}
          onValueChange={(pf) =>
            onChange({ ...value, price_field: pf as PriceField })
          }
        >
          <SelectTrigger className="h-8 w-[80px] text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PRICE_FIELD_OPTIONS.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}

      {value.operand_type === "Constant" && (
        <Input
          type="number"
          className="h-8 w-[80px] text-xs"
          step="any"
          value={value.constant_value ?? 0}
          onChange={(e) =>
            onChange({ ...value, constant_value: Number(e.target.value) })
          }
        />
      )}

      {value.operand_type !== "Constant" && (
        <div className="flex items-center gap-1">
          <span className="text-xs text-muted-foreground">Offset:</span>
          <Input
            type="number"
            className="h-8 w-[50px] text-xs"
            min={0}
            max={500}
            step={1}
            value={value.offset ?? 0}
            onChange={(e) =>
              onChange({
                ...value,
                offset: Number(e.target.value) || undefined,
              })
            }
          />
        </div>
      )}
    </div>
  );
}
