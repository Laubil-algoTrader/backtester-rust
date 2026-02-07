import type { IndicatorConfig, IndicatorType } from "@/lib/types";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/Select";
import { Input } from "@/components/ui/Input";
import {
  INDICATOR_OPTIONS,
  getIndicatorParamFields,
  getOutputFieldOptions,
  createDefaultIndicatorConfig,
} from "./utils";

interface IndicatorSelectorProps {
  value: IndicatorConfig;
  onChange: (config: IndicatorConfig) => void;
}

export function IndicatorSelector({ value, onChange }: IndicatorSelectorProps) {
  const paramFields = getIndicatorParamFields(value.indicator_type);
  const outputFields = getOutputFieldOptions(value.indicator_type);

  const handleTypeChange = (type: string) => {
    onChange(createDefaultIndicatorConfig(type as IndicatorType));
  };

  const handleParamChange = (key: string, val: number) => {
    onChange({
      ...value,
      params: { ...value.params, [key]: val },
    });
  };

  const handleOutputFieldChange = (field: string) => {
    onChange({ ...value, output_field: field });
  };

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <Select value={value.indicator_type} onValueChange={handleTypeChange}>
        <SelectTrigger className="h-8 w-[120px] text-xs">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {INDICATOR_OPTIONS.map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {paramFields.map((field) => (
        <div key={field.key} className="flex items-center gap-1">
          <span className="text-xs text-muted-foreground">{field.label}:</span>
          <Input
            type="number"
            className="h-8 w-[60px] text-xs"
            min={field.min}
            max={field.max}
            step={field.step}
            value={(value.params as Record<string, number | undefined>)[field.key] ?? field.defaultValue}
            onChange={(e) => handleParamChange(field.key, Number(e.target.value))}
          />
        </div>
      ))}

      {outputFields && (
        <Select
          value={value.output_field ?? outputFields[0].value}
          onValueChange={handleOutputFieldChange}
        >
          <SelectTrigger className="h-8 w-[100px] text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {outputFields.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
    </div>
  );
}
