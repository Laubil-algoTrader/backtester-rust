import { useTranslation } from "react-i18next";
import { nanoid } from "nanoid";
import { useAppStore } from "@/stores/useAppStore";
import { SpinnerInput, SectionBox } from "./WhatToBuildTab";
import { DatePicker } from "@/components/ui/DatePicker";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/Select";
import type { BuilderDataRangePart, BuilderDataRangePartType, Timeframe, BacktestPrecision } from "@/lib/types";
import { cn } from "@/lib/utils";

const TIMEFRAMES: Timeframe[] = ["m1", "m5", "m15", "m30", "h1", "h4", "d1"];

const PRECISIONS: { value: BacktestPrecision; label: string }[] = [
  { value: "SelectedTfOnly", label: "Selected TF only (fastest)" },
  { value: "M1TickSimulation", label: "M1 tick simulation" },
  { value: "RealTickCustomSpread", label: "Real tick – custom spread" },
  { value: "RealTickRealSpread", label: "Real tick – real spread" },
];

const QUICK_PRESETS: { label: string; parts: Omit<BuilderDataRangePart, "id">[] }[] = [
  {
    label: "100% IS",
    parts: [{ type: "is", percent: 100 }],
  },
  {
    label: "50/20/30",
    parts: [
      { type: "is", percent: 50 },
      { type: "oos", percent: 20 },
      { type: "is", percent: 30 },
    ],
  },
  {
    label: "30/20/50",
    parts: [
      { type: "is", percent: 30 },
      { type: "oos", percent: 20 },
      { type: "is", percent: 50 },
    ],
  },
  {
    label: "70/30",
    parts: [
      { type: "is", percent: 70 },
      { type: "oos", percent: 30 },
    ],
  },
];

function DataRangeBar({
  parts,
  onChange,
}: {
  parts: BuilderDataRangePart[];
  onChange: (parts: BuilderDataRangePart[]) => void;
}) {
  const { t } = useTranslation("builder");

  const updatePart = (id: string, partial: Partial<BuilderDataRangePart>) =>
    onChange(parts.map((p) => (p.id === id ? { ...p, ...partial } : p)));

  const removePart = (id: string) => onChange(parts.filter((p) => p.id !== id));

  const addPart = () =>
    onChange([...parts, { id: nanoid(6), type: "oos", percent: 20 }]);

  const applyPreset = (preset: Omit<BuilderDataRangePart, "id">[]) =>
    onChange(preset.map((p) => ({ ...p, id: nanoid(6) })));

  const totalPercent = parts.reduce((s, p) => s + p.percent, 0);

  return (
    <div className="space-y-3">
      {/* Visual bar */}
      <div className="flex h-8 w-full overflow-hidden rounded-md border border-border/40 text-[10px] font-semibold">
        {parts.map((p) => (
          <div
            key={p.id}
            style={{ width: `${(p.percent / Math.max(totalPercent, 1)) * 100}%` }}
            className={cn(
              "flex items-center justify-center transition-all",
              p.type === "is"
                ? "bg-blue-600/70 text-blue-100"
                : "bg-emerald-600/70 text-emerald-100"
            )}
          >
            {p.percent >= 10 ? `${p.type.toUpperCase()} ${p.percent}%` : ""}
          </div>
        ))}
      </div>

      {/* Part rows */}
      <div className="space-y-1">
        <div className="grid grid-cols-[90px_60px_1fr_24px] gap-1 px-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
          <span>Type</span>
          <span>%</span>
          <span />
          <span />
        </div>
        {parts.map((p) => (
          <div key={p.id} className="grid grid-cols-[90px_60px_1fr_24px] items-center gap-1">
            <Select
              value={p.type}
              onValueChange={(v) => updatePart(p.id, { type: v as BuilderDataRangePartType })}
            >
              <SelectTrigger
                className={cn(
                  "h-7 rounded border px-2 text-xs focus:ring-1",
                  p.type === "is"
                    ? "border-blue-600/40 bg-blue-600/10 text-blue-400"
                    : "border-emerald-600/40 bg-emerald-600/10 text-emerald-400"
                )}
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="is">{t("data.partTypes.is")} (IS)</SelectItem>
                <SelectItem value="oos">{t("data.partTypes.oos")} (OOS)</SelectItem>
              </SelectContent>
            </Select>
            <input
              type="number"
              min={5}
              max={100}
              value={p.percent}
              onChange={(e) => updatePart(p.id, { percent: Number(e.target.value) })}
              className="rounded border border-border bg-muted/30 px-2 py-1 text-center text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
            />
            <div
              className={cn(
                "h-2 rounded-full",
                p.type === "is" ? "bg-blue-600/40" : "bg-emerald-600/40"
              )}
            />
            <button
              onClick={() => removePart(p.id)}
              disabled={parts.length <= 1}
              className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground/60 hover:text-destructive disabled:opacity-30"
            >
              ×
            </button>
          </div>
        ))}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <button
          onClick={addPart}
          className="rounded border border-border/40 px-3 py-1 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary"
        >
          + {t("data.addNewPart")}
        </button>
        <span className="text-[10px] text-muted-foreground/50">{t("data.mostUsedConfigs")}:</span>
        {QUICK_PRESETS.map((preset) => (
          <button
            key={preset.label}
            onClick={() => applyPreset(preset.parts)}
            className="rounded border border-border/40 px-2 py-0.5 text-[10px] text-muted-foreground hover:border-primary/50 hover:text-primary"
          >
            {preset.label}
          </button>
        ))}
        {totalPercent !== 100 && (
          <span className="ml-auto text-[10px] text-amber-500/80">
            Total: {totalPercent}% (should be 100%)
          </span>
        )}
      </div>
    </div>
  );
}

export function DataTab() {
  const { t } = useTranslation("builder");
  const { builderConfig, updateBuilderConfig, symbols } = useAppStore();
  const dc = builderConfig.dataConfig;
  const update = (partial: Partial<typeof dc>) =>
    updateBuilderConfig({ dataConfig: { ...dc, ...partial } });

  const selectedSymbol = symbols.find((s) => s.id === dc.symbolId);
  const availableTimeframes = selectedSymbol
    ? TIMEFRAMES.filter((tf) => tf in (selectedSymbol.timeframe_paths ?? {}))
    : TIMEFRAMES;

  return (
    <div className="grid grid-cols-2 gap-4 p-4">
      {/* Left column */}
      <div className="space-y-4">
        <SectionBox title={t("data.backtestDataSettings")}>
          <div className="space-y-2.5">
            {/* Symbol */}
            <div className="flex items-center justify-between gap-2">
              <span className="w-28 shrink-0 text-sm text-muted-foreground">{t("data.symbol")}</span>
              <Select
                value={dc.symbolId ?? ""}
                onValueChange={(v) => update({ symbolId: v || null })}
              >
                <SelectTrigger className="h-8 flex-1 text-sm">
                  <SelectValue placeholder="— Select symbol —" />
                </SelectTrigger>
                <SelectContent>
                  {symbols.map((s) => (
                    <SelectItem key={s.id} value={s.id}>{s.name}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Timeframe */}
            <div className="flex items-center justify-between gap-2">
              <span className="w-28 shrink-0 text-sm text-muted-foreground">{t("data.timeframe")}</span>
              <Select
                value={dc.timeframe}
                onValueChange={(v) => update({ timeframe: v as Timeframe })}
              >
                <SelectTrigger className="h-8 flex-1 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {availableTimeframes.map((tf) => (
                    <SelectItem key={tf} value={tf}>{tf.toUpperCase()}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Date range */}
            <div className="flex items-center justify-between gap-2">
              <span className="w-28 shrink-0 text-sm text-muted-foreground">{t("data.startDay")}</span>
              <div className="flex-1">
                <DatePicker
                  value={dc.startDate}
                  onChange={(v) => update({ startDate: v })}
                  className="h-8 text-sm"
                />
              </div>
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="w-28 shrink-0 text-sm text-muted-foreground">{t("data.endDay")}</span>
              <div className="flex-1">
                <DatePicker
                  value={dc.endDate}
                  onChange={(v) => update({ endDate: v })}
                  className="h-8 text-sm"
                />
              </div>
            </div>
            {selectedSymbol && (
              <p className="text-[10px] text-muted-foreground/50">
                {t("data.availableFrom")} {selectedSymbol.start_date} {t("data.to")} {selectedSymbol.end_date}
              </p>
            )}
          </div>
        </SectionBox>

        <SectionBox title={t("data.testParameters")}>
          <div className="space-y-2.5">
            {/* Precision */}
            <div className="flex items-center justify-between gap-2">
              <span className="w-28 shrink-0 text-sm text-muted-foreground">{t("data.precision")}</span>
              <Select
                value={dc.precision}
                onValueChange={(v) => update({ precision: v as BacktestPrecision })}
              >
                <SelectTrigger className="h-8 flex-1 text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PRECISIONS.map((p) => (
                    <SelectItem key={p.value} value={p.value}>{p.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* Spread */}
            <div className="flex items-center justify-between gap-2">
              <span className="w-28 shrink-0 text-sm text-muted-foreground">
                {t("data.spread")}
              </span>
              <div className="flex items-center gap-1.5">
                <SpinnerInput
                  value={dc.spreadPips}
                  onChange={(v) => update({ spreadPips: v })}
                  min={0}
                  max={100}
                  step={0.1}
                />
                <span className="text-[10px] text-muted-foreground/60">{t("data.pips")}</span>
              </div>
            </div>

            {/* Slippage */}
            <div className="flex items-center justify-between gap-2">
              <span className="w-28 shrink-0 text-sm text-muted-foreground">
                {t("data.slippage")}
              </span>
              <div className="flex items-center gap-1.5">
                <SpinnerInput
                  value={dc.slippagePips}
                  onChange={(v) => update({ slippagePips: v })}
                  min={0}
                  max={100}
                  step={0.1}
                />
                <span className="text-[10px] text-muted-foreground/60">{t("data.pips")}</span>
              </div>
            </div>

            {/* Min distance */}
            <div className="flex items-center justify-between gap-2">
              <span className="w-28 shrink-0 text-sm text-muted-foreground">
                {t("data.minDistance")}
              </span>
              <div className="flex items-center gap-1.5">
                <SpinnerInput
                  value={dc.minDistancePips}
                  onChange={(v) => update({ minDistancePips: v })}
                  min={0}
                  max={100}
                  step={0.1}
                />
                <span className="text-[10px] text-muted-foreground/60">{t("data.pips")}</span>
              </div>
            </div>
          </div>
        </SectionBox>
      </div>

      {/* Right column — Data range parts */}
      <div className="space-y-4">
        <SectionBox title={t("data.dataRangeParts")}>
          <p className="mb-3 text-[10px] text-muted-foreground/60">
            Split the data range into In-sample (IS) and Out-of-sample (OOS) periods for walk-forward validation.
          </p>
          <DataRangeBar
            parts={dc.dataRangeParts}
            onChange={(parts) => update({ dataRangeParts: parts })}
          />
        </SectionBox>
      </div>
    </div>
  );
}
