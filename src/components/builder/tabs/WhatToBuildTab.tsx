import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/useAppStore";
import type { BuilderDirection, BuilderSLType, BuilderTPType } from "@/lib/types";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";

// ── Shared primitives (exported — used by other tabs) ─────────────────────────

export function SpinnerInput({
  value,
  onChange,
  min = 0,
  max = 999,
  step = 1,
  className,
}: {
  value: number;
  onChange: (v: number) => void;
  min?: number;
  max?: number;
  step?: number;
  className?: string;
}) {
  return (
    <div className={cn("flex items-center", className)}>
      <input
        type="number"
        value={value}
        min={min}
        max={max}
        step={step}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-16 rounded-l border border-border bg-muted/30 px-2 py-1 text-center text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
      />
      <div className="flex flex-col">
        <button
          onClick={() => onChange(Math.min(max, value + step))}
          className="flex h-[13px] w-5 items-center justify-center rounded-tr border-t border-r border-border bg-muted/40 text-[8px] text-muted-foreground hover:bg-muted"
        >
          ▲
        </button>
        <button
          onClick={() => onChange(Math.max(min, value - step))}
          className="flex h-[13px] w-5 items-center justify-center rounded-br border-b border-r border-border bg-muted/40 text-[8px] text-muted-foreground hover:bg-muted"
        >
          ▼
        </button>
      </div>
    </div>
  );
}

export function ToggleGroup<T extends string>({
  options,
  value,
  onChange,
  labels,
}: {
  options: T[];
  value: T;
  onChange: (v: T) => void;
  labels: Record<string, string>;
}) {
  return (
    <div className="flex flex-wrap gap-1">
      {options.map((opt) => (
        <button
          key={opt}
          onClick={() => onChange(opt)}
          className={cn(
            "rounded border px-3 py-1 text-xs font-medium transition-colors",
            value === opt
              ? "border-primary bg-primary/10 text-primary"
              : "border-border bg-muted/20 text-muted-foreground hover:bg-muted/50"
          )}
        >
          {labels[opt]}
        </button>
      ))}
    </div>
  );
}

export function SectionBox({ title, children }: { title: React.ReactNode; children: React.ReactNode }) {
  return (
    <Card>
      <CardHeader className="pb-2 pt-4 px-4">
        <CardTitle>{title}</CardTitle>
      </CardHeader>
      <CardContent className="px-4 pb-4">
        {children}
      </CardContent>
    </Card>
  );
}

export function ConfigRow({
  label,
  children,
  icon,
  indent,
}: {
  label: string;
  children: React.ReactNode;
  icon?: React.ReactNode;
  indent?: boolean;
}) {
  return (
    <div
      className={cn(
        "flex items-start gap-3 border-t border-border/20 py-2.5 first:border-t-0",
        indent && "ml-4 border-l border-border/20 pl-3"
      )}
    >
      <div className="flex w-48 shrink-0 items-center gap-1.5">
        {icon && <span className="text-muted-foreground/50">{icon}</span>}
        <span className="text-sm text-muted-foreground">{label}</span>
      </div>
      <div className="flex flex-1 flex-wrap items-center gap-2">{children}</div>
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={() => !disabled && onChange(!checked)}
      disabled={disabled}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full border-2 border-transparent transition-colors focus:outline-none",
        disabled ? "cursor-not-allowed opacity-40" : "cursor-pointer",
        checked ? "bg-primary" : "bg-muted"
      )}
    >
      <span
        className={cn(
          "inline-block h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform",
          checked ? "translate-x-4" : "translate-x-0.5"
        )}
      />
    </button>
  );
}

// ── Local primitives ──────────────────────────────────────────────────────────

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <span className="text-[10px] font-semibold uppercase tracking-widest text-muted-foreground/50">
      {children}
    </span>
  );
}

function RangeRow({
  label,
  value,
  onChange,
  min,
  max,
  step = 1,
  hint,
}: {
  label: string;
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
  hint?: string;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="w-8 shrink-0 text-xs text-muted-foreground/70">{label}</span>
      <SpinnerInput value={value} onChange={onChange} min={min} max={max} step={step} />
      {hint && <span className="text-[10px] text-muted-foreground/40">{hint}</span>}
    </div>
  );
}

export function AccordionSection({
  title,
  open,
  onToggle,
  badge,
  headerAction,
  children,
}: {
  title: string;
  open: boolean;
  onToggle: () => void;
  badge?: string;
  headerAction?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="rounded-md border border-border/40 bg-card">
      {/* Header */}
      <button
        onClick={onToggle}
        className="flex w-full items-center gap-2 px-3 py-2.5 text-left"
      >
        <ChevronDown
          className={cn(
            "h-3.5 w-3.5 shrink-0 text-muted-foreground/60 transition-transform",
            open && "rotate-180"
          )}
        />
        <span className="flex-1 text-xs font-semibold text-foreground">{title}</span>
        {!open && badge && (
          <span className="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
            {badge}
          </span>
        )}
        {headerAction && (
          <span
            onClick={(e) => e.stopPropagation()}
            className="ml-1"
          >
            {headerAction}
          </span>
        )}
      </button>

      {/* Body */}
      {open && (
        <div className="border-t border-border/30 px-3 pb-3 pt-2.5">
          {children}
        </div>
      )}
    </div>
  );
}

// ── Main component ────────────────────────────────────────────────────────────

export function WhatToBuildTab() {
  const { t } = useTranslation("builder");
  const { builderConfig, updateBuilderConfig } = useAppStore();
  const wtb = builderConfig.whatToBuild;

  const [open, setOpen] = useState<Set<string>>(
    () => new Set(["direction", "rules", "sl", "tp"])
  );

  const update = (partial: Partial<typeof wtb>) =>
    updateBuilderConfig({ whatToBuild: { ...wtb, ...partial } });

  const toggleSection = (key: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      next.has(key) ? next.delete(key) : next.add(key);
      return next;
    });

  const directionLabels: Record<string, string> = {
    long_only: t("whatToBuild.directions.longOnly"),
    short_only: t("whatToBuild.directions.shortOnly"),
    both_symmetric: t("whatToBuild.directions.bothSymmetric"),
    both_asymmetric: t("whatToBuild.directions.bothAsymmetric"),
  };

  const dirBadge = directionLabels[wtb.direction] ?? wtb.direction;
  const slBadge = wtb.slRequired ? wtb.slType.toUpperCase() : "Off";
  const tpBadge = wtb.tpRequired ? wtb.tpType.toUpperCase() : "Off";
  const rulesBadge = `${wtb.minEntryRules}–${wtb.maxEntryRules} entry`;

  return (
    <div className="flex flex-col gap-2 p-4">

      {/* ── Summary strip ─────────────────────────────────────────────────── */}
      <div className="flex flex-wrap gap-x-4 gap-y-1 rounded border border-border/30 bg-muted/10 px-3 py-1.5">
        <SummaryChip label="Direction" value={dirBadge} />
        <SummaryChip label="Entry" value={`${wtb.minEntryRules}–${wtb.maxEntryRules}`} />
        <SummaryChip label="Exit" value={`${wtb.minExitRules}–${wtb.maxExitRules}`} />
        <SummaryChip label="Periods" value={`${wtb.indicatorPeriodMin}–${wtb.indicatorPeriodMax}`} />
        <SummaryChip label="SL" value={slBadge} />
        <SummaryChip label="TP" value={tpBadge} />
      </div>

      {/* ── 1. Trading direction ──────────────────────────────────────────── */}
      <AccordionSection
        title={t("whatToBuild.tradingDirections")}
        open={open.has("direction")}
        onToggle={() => toggleSection("direction")}
        badge={dirBadge}
      >
        <ToggleGroup
          options={["long_only", "short_only", "both_symmetric", "both_asymmetric"] as BuilderDirection[]}
          value={wtb.direction}
          onChange={(v) => update({ direction: v as BuilderDirection })}
          labels={directionLabels}
        />
      </AccordionSection>

      {/* ── 2. Rules & Periods ───────────────────────────────────────────── */}
      <AccordionSection
        title={t("whatToBuild.conditionsPeriods")}
        open={open.has("rules")}
        onToggle={() => toggleSection("rules")}
        badge={rulesBadge}
      >
        <div className="grid grid-cols-2 gap-x-6 gap-y-4">
          <div className="flex flex-col gap-2">
            <SectionLabel>{t("whatToBuild.conditions.entryRules")}</SectionLabel>
            <RangeRow label="Min" value={wtb.minEntryRules} onChange={(v) => update({ minEntryRules: v })} min={1} max={10} />
            <RangeRow label="Max" value={wtb.maxEntryRules} onChange={(v) => update({ maxEntryRules: v })} min={1} max={10} />
          </div>
          <div className="flex flex-col gap-2">
            <SectionLabel>{t("whatToBuild.conditions.exitRules")}</SectionLabel>
            <RangeRow label="Min" value={wtb.minExitRules} onChange={(v) => update({ minExitRules: v })} min={0} max={10} />
            <RangeRow label="Max" value={wtb.maxExitRules} onChange={(v) => update({ maxExitRules: v })} min={0} max={10} />
          </div>
          <div className="flex flex-col gap-2">
            <SectionLabel>{t("whatToBuild.conditions.maxLookback")}</SectionLabel>
            <RangeRow
              label="Max"
              value={wtb.maxLookback}
              onChange={(v) => update({ maxLookback: v })}
              min={0}
              max={20}
              hint={t("whatToBuild.conditions.lookbackHint")}
            />
          </div>
          <div className="flex flex-col gap-2">
            <SectionLabel>{t("whatToBuild.conditions.indicatorPeriods")}</SectionLabel>
            <RangeRow label="Min" value={wtb.indicatorPeriodMin} onChange={(v) => update({ indicatorPeriodMin: v })} min={2} max={500} />
            <RangeRow label="Max" value={wtb.indicatorPeriodMax} onChange={(v) => update({ indicatorPeriodMax: v })} min={2} max={500} />
            <RangeRow label="Step" value={wtb.indicatorPeriodStep} onChange={(v) => update({ indicatorPeriodStep: v })} min={1} max={50} />
          </div>
        </div>
      </AccordionSection>

      {/* ── 3 + 4. Stop Loss & Take Profit — side by side ───────────────── */}
      <div className="grid grid-cols-2 gap-2">

        {/* Stop Loss */}
        <AccordionSection
          title={t("whatToBuild.stopLoss")}
          open={open.has("sl")}
          onToggle={() => toggleSection("sl")}
          badge={slBadge}
          headerAction={
            <Toggle checked={wtb.slRequired} onChange={(v) => update({ slRequired: v })} />
          }
        >
          {wtb.slRequired ? (
            <div className="flex flex-col gap-3">
              <div>
                <SectionLabel>{t("whatToBuild.slConfig.type")}</SectionLabel>
                <div className="mt-1.5">
                  <ToggleGroup
                    options={["atr", "pips", "percentage"] as BuilderSLType[]}
                    value={wtb.slType}
                    onChange={(v) => update({ slType: v as BuilderSLType })}
                    labels={{ atr: "ATR", pips: "Pips", percentage: "%" }}
                  />
                </div>
              </div>
              <div className="flex flex-col gap-1.5">
                <SectionLabel>{t("whatToBuild.slConfig.multiplierRange")}</SectionLabel>
                <div className="flex flex-wrap items-center gap-1.5">
                  <SpinnerInput value={wtb.slCoeffMin} onChange={(v) => update({ slCoeffMin: v })} min={0.1} max={100} step={wtb.slCoeffStep} />
                  <span className="text-xs text-muted-foreground/50">–</span>
                  <SpinnerInput value={wtb.slCoeffMax} onChange={(v) => update({ slCoeffMax: v })} min={0.1} max={100} step={wtb.slCoeffStep} />
                  <span className="text-[10px] text-muted-foreground/40">{t("whatToBuild.slConfig.step")}</span>
                  <SpinnerInput value={wtb.slCoeffStep} onChange={(v) => update({ slCoeffStep: v })} min={0.1} max={10} step={0.1} />
                </div>
              </div>
              {wtb.slType === "atr" && (
                <div className="flex flex-col gap-1.5">
                  <SectionLabel>{t("whatToBuild.slConfig.atrPeriodRange")}</SectionLabel>
                  <div className="flex flex-wrap items-center gap-1.5">
                    <SpinnerInput value={wtb.slAtrPeriodMin} onChange={(v) => update({ slAtrPeriodMin: v })} min={2} max={200} />
                    <span className="text-xs text-muted-foreground/50">–</span>
                    <SpinnerInput value={wtb.slAtrPeriodMax} onChange={(v) => update({ slAtrPeriodMax: v })} min={2} max={200} />
                    <span className="text-[10px] text-muted-foreground/40">{t("whatToBuild.slConfig.step")}</span>
                    <SpinnerInput value={wtb.slAtrPeriodStep} onChange={(v) => update({ slAtrPeriodStep: v })} min={1} max={50} />
                  </div>
                </div>
              )}
            </div>
          ) : (
            <p className="text-xs text-muted-foreground/40">Stop Loss disabled</p>
          )}
        </AccordionSection>

        {/* Take Profit */}
        <AccordionSection
          title={t("whatToBuild.profitTarget")}
          open={open.has("tp")}
          onToggle={() => toggleSection("tp")}
          badge={tpBadge}
          headerAction={
            <Toggle checked={wtb.tpRequired} onChange={(v) => update({ tpRequired: v })} />
          }
        >
          {wtb.tpRequired ? (
            <div className="flex flex-col gap-3">
              <div>
                <SectionLabel>{t("whatToBuild.tpConfig.type")}</SectionLabel>
                <div className="mt-1.5">
                  <ToggleGroup
                    options={["atr", "pips", "rr"] as BuilderTPType[]}
                    value={wtb.tpType}
                    onChange={(v) => update({ tpType: v as BuilderTPType })}
                    labels={{ atr: "ATR", pips: "Pips", rr: "R/R" }}
                  />
                </div>
              </div>
              <div className="flex flex-col gap-1.5">
                <SectionLabel>{t("whatToBuild.tpConfig.multiplierRange")}</SectionLabel>
                <div className="flex flex-wrap items-center gap-1.5">
                  <SpinnerInput value={wtb.tpCoeffMin} onChange={(v) => update({ tpCoeffMin: v })} min={0.1} max={100} step={wtb.tpCoeffStep} />
                  <span className="text-xs text-muted-foreground/50">–</span>
                  <SpinnerInput value={wtb.tpCoeffMax} onChange={(v) => update({ tpCoeffMax: v })} min={0.1} max={100} step={wtb.tpCoeffStep} />
                  <span className="text-[10px] text-muted-foreground/40">{t("whatToBuild.tpConfig.step")}</span>
                  <SpinnerInput value={wtb.tpCoeffStep} onChange={(v) => update({ tpCoeffStep: v })} min={0.1} max={10} step={0.1} />
                </div>
              </div>
              {wtb.tpType === "atr" && (
                <div className="flex flex-col gap-1.5">
                  <SectionLabel>{t("whatToBuild.tpConfig.atrPeriodRange")}</SectionLabel>
                  <div className="flex flex-wrap items-center gap-1.5">
                    <SpinnerInput value={wtb.tpAtrPeriodMin} onChange={(v) => update({ tpAtrPeriodMin: v })} min={2} max={200} />
                    <span className="text-xs text-muted-foreground/50">–</span>
                    <SpinnerInput value={wtb.tpAtrPeriodMax} onChange={(v) => update({ tpAtrPeriodMax: v })} min={2} max={200} />
                    <span className="text-[10px] text-muted-foreground/40">{t("whatToBuild.tpConfig.step")}</span>
                    <SpinnerInput value={wtb.tpAtrPeriodStep} onChange={(v) => update({ tpAtrPeriodStep: v })} min={1} max={50} />
                  </div>
                </div>
              )}
            </div>
          ) : (
            <p className="text-xs text-muted-foreground/40">Take Profit disabled</p>
          )}
        </AccordionSection>
      </div>
    </div>
  );
}

function SummaryChip({ label, value }: { label: string; value: string }) {
  return (
    <span className="text-[11px]">
      <span className="text-muted-foreground/50">{label} </span>
      <span className="font-medium text-foreground/80">{value}</span>
    </span>
  );
}
