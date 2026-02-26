import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Operand, OperandType, PriceField, TimeField, CandlePatternType } from "@/lib/types";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/Select";
import { Input } from "@/components/ui/Input";
import { Popover, PopoverTrigger, PopoverContent } from "@/components/ui/Popover";
import { Tooltip, TooltipTrigger, TooltipContent } from "@/components/ui/Tooltip";
import { Button } from "@/components/ui/Button";
import { ChevronDown, Check } from "lucide-react";
import { IndicatorSelector } from "./IndicatorSelector";
import { createDefaultIndicatorConfig, TIME_FIELD_OPTIONS, CANDLE_PATTERN_OPTIONS } from "./utils";

interface OperandSelectorProps {
  value: Operand;
  onChange: (operand: Operand) => void;
  contextTimeField?: TimeField;
}

const OPERAND_TYPE_KEYS: { value: OperandType; key: string }[] = [
  { value: "Indicator", key: "operandTypes.indicator" },
  { value: "Price", key: "operandTypes.price" },
  { value: "Constant", key: "operandTypes.constant" },
  { value: "BarTime", key: "operandTypes.barTime" },
  { value: "CandlePattern", key: "operandTypes.candlePattern" },
];

const PRICE_FIELD_KEYS: { value: PriceField; key: string }[] = [
  { value: "Open", key: "priceFields.open" },
  { value: "High", key: "priceFields.high" },
  { value: "Low", key: "priceFields.low" },
  { value: "Close", key: "priceFields.close" },
  { value: "DailyOpen", key: "priceFields.dailyOpen" },
  { value: "DailyHigh", key: "priceFields.dailyHigh" },
  { value: "DailyLow", key: "priceFields.dailyLow" },
  { value: "DailyClose", key: "priceFields.dailyClose" },
];

const MONTH_KEYS = [
  { value: 1, key: "monthsFull.january" },
  { value: 2, key: "monthsFull.february" },
  { value: 3, key: "monthsFull.march" },
  { value: 4, key: "monthsFull.april" },
  { value: 5, key: "monthsFull.may" },
  { value: 6, key: "monthsFull.june" },
  { value: 7, key: "monthsFull.july" },
  { value: 8, key: "monthsFull.august" },
  { value: 9, key: "monthsFull.september" },
  { value: 10, key: "monthsFull.october" },
  { value: 11, key: "monthsFull.november" },
  { value: 12, key: "monthsFull.december" },
];

const DAY_KEYS = [
  { value: 0, key: "days.sunday" },
  { value: 1, key: "days.monday" },
  { value: 2, key: "days.tuesday" },
  { value: 3, key: "days.wednesday" },
  { value: 4, key: "days.thursday" },
  { value: 5, key: "days.friday" },
  { value: 6, key: "days.saturday" },
];

const HOUR_OPTIONS = Array.from({ length: 24 }, (_, i) => ({
  value: i,
  label: `${String(i).padStart(2, "0")}:00`,
}));

const MINUTE_OPTIONS = Array.from({ length: 60 }, (_, i) => ({
  value: i,
  label: String(i).padStart(2, "0"),
}));

type TimeContext = "month" | "dayOfWeek" | "hour" | "minute" | "time" | null;

function getTimeContext(field?: TimeField): TimeContext {
  if (!field) return null;
  switch (field) {
    case "CurrentMonth":
      return "month";
    case "BarDayOfWeek":
    case "CurrentDayOfWeek":
      return "dayOfWeek";
    case "BarHour":
    case "CurrentHour":
      return "hour";
    case "BarMinute":
    case "CurrentMinute":
      return "minute";
    case "BarTimeValue":
    case "CurrentTime":
      return "time";
    default:
      return null;
  }
}

function ContextualConstantInput({
  context,
  value,
  onChange,
  t,
  tc,
}: {
  context: TimeContext;
  value: number;
  onChange: (v: number) => void;
  t: (key: string) => string;
  tc: (key: string) => string;
}) {
  if (context === "month") {
    return (
      <Select
        value={String(value || 1)}
        onValueChange={(v) => onChange(Number(v))}
      >
        <SelectTrigger className="h-8 w-[140px] text-sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {MONTH_KEYS.map((opt) => (
            <SelectItem key={opt.value} value={String(opt.value)}>
              {tc(opt.key)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }

  if (context === "dayOfWeek") {
    return (
      <Select
        value={String(value ?? 0)}
        onValueChange={(v) => onChange(Number(v))}
      >
        <SelectTrigger className="h-8 w-[130px] text-sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {DAY_KEYS.map((opt) => (
            <SelectItem key={opt.value} value={String(opt.value)}>
              {tc(opt.key)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }

  if (context === "hour") {
    return (
      <Select
        value={String(value ?? 0)}
        onValueChange={(v) => onChange(Number(v))}
      >
        <SelectTrigger className="h-8 w-[90px] text-sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {HOUR_OPTIONS.map((opt) => (
            <SelectItem key={opt.value} value={String(opt.value)}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }

  if (context === "minute") {
    return (
      <Select
        value={String(value ?? 0)}
        onValueChange={(v) => onChange(Number(v))}
      >
        <SelectTrigger className="h-8 w-[70px] text-sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {MINUTE_OPTIONS.map((opt) => (
            <SelectItem key={opt.value} value={String(opt.value)}>
              {opt.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }

  if (context === "time") {
    const totalMinutes = value ?? 0;
    const hours = Math.floor(totalMinutes / 60);
    const minutes = totalMinutes % 60;
    return (
      <div className="flex items-center gap-1">
        <Select
          value={String(hours)}
          onValueChange={(v) => onChange(Number(v) * 60 + minutes)}
        >
          <SelectTrigger className="h-8 w-[90px] text-sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {HOUR_OPTIONS.map((opt) => (
              <SelectItem key={opt.value} value={String(opt.value)}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span className="text-sm text-muted-foreground">:</span>
        <Select
          value={String(minutes)}
          onValueChange={(v) => onChange(hours * 60 + Number(v))}
        >
          <SelectTrigger className="h-8 w-[70px] text-sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {MINUTE_OPTIONS.map((opt) => (
              <SelectItem key={opt.value} value={String(opt.value)}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    );
  }

  return null;
}

/* ── Candle pattern SVG illustrations ── */

const CANDLE_GREEN = "#22c55e";
const CANDLE_RED = "#ef4444";
const WICK_COLOR = "#94a3b8"; // slate-400, visible in both themes

interface CandleProps {
  x: number;
  bodyTop: number;
  bodyBottom: number;
  wickTop: number;
  wickBottom: number;
  fill: string;
  width?: number;
}

function Candle({ x, bodyTop, bodyBottom, wickTop, wickBottom, fill, width = 14 }: CandleProps) {
  const half = width / 2;
  return (
    <>
      <line x1={x} y1={wickTop} x2={x} y2={wickBottom} stroke={WICK_COLOR} strokeWidth={2} />
      <rect
        x={x - half}
        y={bodyTop}
        width={width}
        height={Math.max(bodyBottom - bodyTop, 2)}
        fill={fill}
        rx={1}
      />
    </>
  );
}

function PatternSvg({ pattern }: { pattern: string }) {
  const w = 90;
  const h = 64;

  const content = (() => {
    switch (pattern) {
      case "Doji":
        return (
          <Candle x={45} bodyTop={30} bodyBottom={32} wickTop={12} wickBottom={52} fill={WICK_COLOR} />
        );
      case "Hammer":
        return (
          <Candle x={45} bodyTop={16} bodyBottom={26} wickTop={12} wickBottom={52} fill={CANDLE_GREEN} />
        );
      case "ShootingStar":
        return (
          <Candle x={45} bodyTop={40} bodyBottom={50} wickTop={12} wickBottom={54} fill={CANDLE_RED} />
        );
      case "BearishEngulfing":
        return (
          <>
            <Candle x={30} bodyTop={24} bodyBottom={38} wickTop={18} wickBottom={44} fill={CANDLE_GREEN} width={12} />
            <Candle x={60} bodyTop={18} bodyBottom={44} wickTop={14} wickBottom={50} fill={CANDLE_RED} />
          </>
        );
      case "BullishEngulfing":
        return (
          <>
            <Candle x={30} bodyTop={24} bodyBottom={38} wickTop={18} wickBottom={44} fill={CANDLE_RED} width={12} />
            <Candle x={60} bodyTop={18} bodyBottom={44} wickTop={14} wickBottom={50} fill={CANDLE_GREEN} />
          </>
        );
      case "DarkCloud":
        return (
          <>
            <Candle x={30} bodyTop={28} bodyBottom={44} wickTop={22} wickBottom={50} fill={CANDLE_GREEN} width={12} />
            <Candle x={60} bodyTop={20} bodyBottom={40} wickTop={14} wickBottom={46} fill={CANDLE_RED} />
          </>
        );
      case "PiercingLine":
        return (
          <>
            <Candle x={30} bodyTop={20} bodyBottom={36} wickTop={14} wickBottom={44} fill={CANDLE_RED} width={12} />
            <Candle x={60} bodyTop={24} bodyBottom={44} wickTop={18} wickBottom={50} fill={CANDLE_GREEN} />
          </>
        );
      default:
        return null;
    }
  })();

  return (
    <svg width={w} height={h} viewBox={`0 0 ${w} ${h}`} className="block">
      {content}
    </svg>
  );
}

const PATTERN_DESC_KEYS: Record<string, string> = {
  Doji: "patternDescriptions.Doji",
  Hammer: "patternDescriptions.Hammer",
  ShootingStar: "patternDescriptions.ShootingStar",
  BearishEngulfing: "patternDescriptions.BearishEngulfing",
  BullishEngulfing: "patternDescriptions.BullishEngulfing",
  DarkCloud: "patternDescriptions.DarkCloud",
  PiercingLine: "patternDescriptions.PiercingLine",
};

function CandlePatternPicker({
  value,
  onSelect,
  t,
}: {
  value: CandlePatternType;
  onSelect: (v: CandlePatternType) => void;
  t: (key: string) => string;
}) {
  const [open, setOpen] = useState(false);
  const label = CANDLE_PATTERN_OPTIONS.find((o) => o.value === value)?.label ?? value;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          role="combobox"
          aria-expanded={open}
          className="h-8 w-[180px] justify-between px-3 text-sm font-normal"
        >
          {label}
          <ChevronDown className="ml-2 h-3.5 w-3.5 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[200px] p-1" align="start">
        {CANDLE_PATTERN_OPTIONS.map((opt) => (
          <Tooltip key={opt.value} delayDuration={400}>
            <TooltipTrigger asChild>
              <button
                type="button"
                className={
                  "flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none hover:bg-accent hover:text-accent-foreground" +
                  (value === opt.value ? " bg-accent/50" : "")
                }
                onClick={() => {
                  onSelect(opt.value as CandlePatternType);
                  setOpen(false);
                }}
              >
                {value === opt.value && (
                  <Check className="h-3.5 w-3.5 shrink-0" />
                )}
                <span className={value !== opt.value ? "pl-[22px]" : ""}>
                  {opt.label}
                </span>
              </button>
            </TooltipTrigger>
            <TooltipContent side="right" sideOffset={8} className="p-2">
              <PatternSvg pattern={opt.value} />
              <p className="mt-1 max-w-[160px] text-xs text-muted-foreground">
                {t(PATTERN_DESC_KEYS[opt.value])}
              </p>
            </TooltipContent>
          </Tooltip>
        ))}
      </PopoverContent>
    </Popover>
  );
}

export function OperandSelector({
  value,
  onChange,
  contextTimeField,
}: OperandSelectorProps) {
  const { t } = useTranslation("strategy");
  const { t: tc } = useTranslation("common");

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
    } else if (operandType === "BarTime") {
      onChange({
        operand_type: "BarTime",
        time_field: "BarHour",
        offset: value.offset,
      });
    } else if (operandType === "CandlePattern") {
      onChange({
        operand_type: "CandlePattern",
        candle_pattern: "Doji",
        offset: value.offset,
      });
    } else {
      onChange({
        operand_type: "Constant",
        constant_value: 0,
      });
    }
  };

  const timeContext = getTimeContext(contextTimeField);

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <Select value={value.operand_type} onValueChange={handleTypeChange}>
        <SelectTrigger className="h-8 w-[120px] text-sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {OPERAND_TYPE_KEYS.map((opt) => (
            <SelectItem key={opt.value} value={opt.value}>
              {t(opt.key)}
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
          <SelectTrigger className="h-8 w-[110px] text-sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PRICE_FIELD_KEYS.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {t(opt.key)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}

      {value.operand_type === "BarTime" && (
        <Select
          value={value.time_field ?? "BarHour"}
          onValueChange={(tf) =>
            onChange({ ...value, time_field: tf as TimeField })
          }
        >
          <SelectTrigger className="h-8 w-[200px] text-sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {TIME_FIELD_OPTIONS.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {opt.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}

      {value.operand_type === "CandlePattern" && (
        <CandlePatternPicker
          value={(value.candle_pattern ?? "Doji") as CandlePatternType}
          onSelect={(cp) =>
            onChange({ ...value, candle_pattern: cp })
          }
          t={t}
        />
      )}

      {value.operand_type === "Constant" && timeContext ? (
        <ContextualConstantInput
          context={timeContext}
          value={value.constant_value ?? 0}
          onChange={(v) => onChange({ ...value, constant_value: v })}
          t={t}
          tc={tc}
        />
      ) : value.operand_type === "Constant" ? (
        <Input
          type="number"
          className="h-8 w-[80px] text-sm"
          step="any"
          value={value.constant_value ?? 0}
          onChange={(e) =>
            onChange({ ...value, constant_value: Number(e.target.value) })
          }
        />
      ) : null}

      {value.operand_type !== "Constant" && (
        <div className="flex items-center gap-1">
          <span className="text-sm text-muted-foreground">{t("offset")}:</span>
          <Input
            type="number"
            className="h-8 w-[50px] text-sm"
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
