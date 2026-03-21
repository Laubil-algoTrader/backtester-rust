import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Settings2, X } from "lucide-react";
import { useAppStore } from "@/stores/useAppStore";
import { SectionBox, AccordionSection } from "./WhatToBuildTab";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/Dialog";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/Popover";
import type { BuilderIndicatorBlock, BuilderOrderTypeBlock, BuilderExitTypeBlock, BuilderOrderPriceBlock, OrderPriceBaseField } from "@/lib/types";
import { cn } from "@/lib/utils";

const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("");

const EXIT_TYPE_LABELS: Record<BuilderExitTypeBlock["exitType"], string> = {
  profit_target: "Profit Target",
  stop_loss: "Stop Loss",
  trailing_stop: "Trailing Stop",
  exit_after_bars: "Exit After Bars",
  move_sl_be: "Move SL to BE",
  exit_rule: "Exit Rule",
};

// Exit types not yet wired to the executor — shown as pending
const EXIT_PENDING: Set<BuilderExitTypeBlock["exitType"]> = new Set<BuilderExitTypeBlock["exitType"]>();

const ORDER_TYPE_LABELS: Record<BuilderOrderTypeBlock["orderType"], string> = {
  stop: "(STOP) Enter at stop",
  limit: "(LMT) Enter at limit",
  market: "(MKT) Enter at market",
};

// Order types not yet wired to the executor
const ORDER_PENDING: Set<BuilderOrderTypeBlock["orderType"]> = new Set<BuilderOrderTypeBlock["orderType"]>();

function WeightInput({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  return (
    <input
      type="number"
      min={1}
      max={10}
      value={value}
      onChange={(e) => onChange(Math.max(1, Math.min(10, Number(e.target.value))))}
      className="w-12 rounded border border-border bg-muted/30 px-1 py-0.5 text-center text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
    />
  );
}

function PeriodInput({
  value,
  onChange,
  placeholder,
}: {
  value: number | undefined;
  onChange: (v: number) => void;
  placeholder?: string;
}) {
  return (
    <input
      type="number"
      min={2}
      max={500}
      value={value ?? ""}
      placeholder={placeholder}
      onChange={(e) => onChange(Number(e.target.value))}
      className="w-16 rounded border border-border bg-input px-1.5 py-1 text-center text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
    />
  );
}

// ── Per-indicator period popover ─────────────────────────────────────────────

interface PeriodPopoverProps {
  ind: BuilderIndicatorBlock;
  globalMin: number;
  globalMax: number;
  globalStep: number;
  onSave: (min: number, max: number, step: number) => void;
  onReset: () => void;
}

function PeriodPopover({ ind, globalMin, globalMax, globalStep, onSave, onReset }: PeriodPopoverProps) {
  const [open, setOpen] = useState(false);
  const [draftMin, setDraftMin] = useState<number>(ind.periodMin ?? globalMin);
  const [draftMax, setDraftMax] = useState<number>(ind.periodMax ?? globalMax);
  const [draftStep, setDraftStep] = useState<number>(ind.periodStep ?? globalStep);

  const hasCustom = ind.periodMin !== undefined;

  const handleOpen = (o: boolean) => {
    if (o) {
      setDraftMin(ind.periodMin ?? globalMin);
      setDraftMax(ind.periodMax ?? globalMax);
      setDraftStep(ind.periodStep ?? globalStep);
    }
    setOpen(o);
  };

  const handleSave = () => {
    const min = Math.max(2, draftMin);
    const max = Math.max(min + 1, draftMax);
    const step = Math.max(1, draftStep);
    onSave(min, max, step);
    setOpen(false);
  };

  const handleReset = () => {
    onReset();
    setOpen(false);
  };

  return (
    <Popover open={open} onOpenChange={handleOpen}>
      <PopoverTrigger asChild>
        <button
          className={cn(
            "flex h-6 items-center gap-1 rounded border px-1.5 text-[10px] font-medium transition-colors",
            hasCustom
              ? "border-primary/40 bg-primary/10 text-primary hover:bg-primary/20"
              : "border-border/30 text-muted-foreground hover:border-primary/40 hover:text-primary"
          )}
        >
          {hasCustom ? (
            <>
              <span>{ind.periodMin}–{ind.periodMax}</span>
              <Settings2 className="h-2.5 w-2.5" />
            </>
          ) : (
            <>
              <Settings2 className="h-2.5 w-2.5" />
              <span>Personalizado</span>
            </>
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-64 p-3" align="end">
        <p className="mb-2.5 text-xs font-semibold text-foreground">
          {ind.indicatorType} — Rango de periodo
        </p>
        <div className="grid grid-cols-3 gap-2">
          <div className="flex flex-col gap-1">
            <span className="text-[10px] text-muted-foreground/70">Mín</span>
            <PeriodInput value={draftMin} onChange={setDraftMin} />
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-[10px] text-muted-foreground/70">Máx</span>
            <PeriodInput value={draftMax} onChange={setDraftMax} />
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-[10px] text-muted-foreground/70">Paso</span>
            <PeriodInput value={draftStep} onChange={setDraftStep} />
          </div>
        </div>
        <div className="mt-3 flex items-center justify-between">
          <button
            onClick={handleReset}
            className="text-[10px] text-muted-foreground/60 hover:text-destructive"
          >
            Restablecer
          </button>
          <button
            onClick={handleSave}
            className="rounded bg-primary px-3 py-1 text-[10px] font-medium text-primary-foreground hover:bg-primary/90"
          >
            Listo
          </button>
        </div>
      </PopoverContent>
    </Popover>
  );
}

// ── Calibrate Dialog ──────────────────────────────────────────────────────────

type DraftRow = { min: number; max: number; step: number; overridden: boolean };

interface CalibrateDialogProps {
  open: boolean;
  onClose: () => void;
  indicators: BuilderIndicatorBlock[];
  globalMin: number;
  globalMax: number;
  globalStep: number;
  onApply: (indicators: BuilderIndicatorBlock[]) => void;
}

function CalibrateDialog({
  open,
  onClose,
  indicators,
  globalMin,
  globalMax,
  globalStep,
  onApply,
}: CalibrateDialogProps) {
  // Draft always stores real numbers — no empty inputs
  const [draft, setDraft] = useState<DraftRow[]>([]);

  // Initialize draft whenever the dialog opens
  useEffect(() => {
    if (open) {
      setDraft(
        indicators.map((i) => ({
          min: i.periodMin ?? globalMin,
          max: i.periodMax ?? globalMax,
          step: i.periodStep ?? globalStep,
          overridden: i.periodMin !== undefined,
        }))
      );
    }
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleOpen = (o: boolean) => {
    if (!o) onClose();
  };

  const updateRow = (idx: number, partial: Partial<DraftRow>) =>
    setDraft((prev) =>
      prev.map((d, i) => (i === idx ? { ...d, ...partial, overridden: true } : d))
    );

  const resetOne = (idx: number) =>
    setDraft((prev) =>
      prev.map((d, i) =>
        i === idx ? { min: globalMin, max: globalMax, step: globalStep, overridden: false } : d
      )
    );

  const resetAll = () =>
    setDraft((prev) =>
      prev.map(() => ({ min: globalMin, max: globalMax, step: globalStep, overridden: false }))
    );

  const handleApply = () => {
    const updated = indicators.map((orig, idx) => {
      const row = draft[idx];
      if (!row || !row.overridden) return { ...orig, periodMin: undefined, periodMax: undefined, periodStep: undefined };
      const min = Math.max(2, row.min);
      const max = Math.max(min + 1, row.max);
      const step = Math.max(1, row.step);
      return { ...orig, periodMin: min, periodMax: max, periodStep: step };
    });
    onApply(updated);
    onClose();
  };

  const overriddenCount = draft.filter((d) => d.overridden).length;

  return (
    <Dialog open={open} onOpenChange={handleOpen}>
      <DialogContent className="max-w-2xl p-0">
        <DialogHeader className="border-b border-border/30 px-5 py-3">
          <DialogTitle className="text-sm font-semibold">
            Calibrar rangos de periodos
          </DialogTitle>
          <p className="text-[10px] text-muted-foreground/60 mt-0.5">
            Rango global: {globalMin}–{globalMax}, paso {globalStep}.
            Los indicadores resaltados tienen rango propio.
          </p>
        </DialogHeader>

        {/* Table */}
        <div className="max-h-[460px] overflow-y-auto px-5 py-2">
          <div className="grid grid-cols-[1fr_64px_64px_52px_28px] gap-x-3 border-b border-border/30 pb-1.5 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
            <span>Indicador</span>
            <span className="text-center">Mín</span>
            <span className="text-center">Máx</span>
            <span className="text-center">Paso</span>
            <span />
          </div>
          {draft.map((row, idx) => {
            const ind = indicators[idx];
            if (!ind) return null;
            return (
              <div
                key={ind.indicatorType}
                className={cn(
                  "grid grid-cols-[1fr_64px_64px_52px_28px] items-center gap-x-3 border-b border-border/10 py-1",
                  !ind.enabled && "opacity-40"
                )}
              >
                <span
                  className={cn(
                    "truncate text-sm",
                    row.overridden ? "font-medium text-foreground" : "text-muted-foreground"
                  )}
                >
                  {ind.indicatorType}
                </span>
                <input
                  type="number"
                  min={2}
                  max={500}
                  value={row.min}
                  onChange={(e) => updateRow(idx, { min: Number(e.target.value) })}
                  className={cn(
                    "w-full rounded border px-1.5 py-1 text-center text-xs focus:outline-none focus:ring-1 focus:ring-primary",
                    row.overridden
                      ? "border-primary/30 bg-primary/5 text-foreground"
                      : "border-border/40 bg-muted/20 text-muted-foreground"
                  )}
                />
                <input
                  type="number"
                  min={2}
                  max={500}
                  value={row.max}
                  onChange={(e) => updateRow(idx, { max: Number(e.target.value) })}
                  className={cn(
                    "w-full rounded border px-1.5 py-1 text-center text-xs focus:outline-none focus:ring-1 focus:ring-primary",
                    row.overridden
                      ? "border-primary/30 bg-primary/5 text-foreground"
                      : "border-border/40 bg-muted/20 text-muted-foreground"
                  )}
                />
                <input
                  type="number"
                  min={1}
                  max={100}
                  value={row.step}
                  onChange={(e) => updateRow(idx, { step: Number(e.target.value) })}
                  className={cn(
                    "w-full rounded border px-1.5 py-1 text-center text-xs focus:outline-none focus:ring-1 focus:ring-primary",
                    row.overridden
                      ? "border-primary/30 bg-primary/5 text-foreground"
                      : "border-border/40 bg-muted/20 text-muted-foreground"
                  )}
                />
                <button
                  onClick={() => resetOne(idx)}
                  disabled={!row.overridden}
                  title="Restablecer al global"
                  className="flex items-center justify-center rounded text-muted-foreground/30 hover:text-destructive disabled:pointer-events-none"
                >
                  <X className="h-3 w-3" />
                </button>
              </div>
            );
          })}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-border/30 px-5 py-3">
          <div className="flex items-center gap-3">
            <button
              onClick={resetAll}
              className="text-xs text-muted-foreground/60 hover:text-destructive"
            >
              Restablecer todos
            </button>
            {overriddenCount > 0 && (
              <span className="text-[10px] text-primary/70">
                {overriddenCount} personalizado{overriddenCount !== 1 ? "s" : ""}
              </span>
            )}
          </div>
          <div className="flex gap-2">
            <button
              onClick={onClose}
              className="rounded border border-border/40 px-4 py-1.5 text-xs text-muted-foreground hover:border-border hover:text-foreground"
            >
              Cancelar
            </button>
            <button
              onClick={handleApply}
              className="rounded bg-primary px-4 py-1.5 text-xs font-medium text-primary-foreground hover:bg-primary/90"
            >
              Aplicar y cerrar
            </button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ── Main tab ──────────────────────────────────────────────────────────────────

export function BuildingBlocksTab() {
  const { t } = useTranslation("builder");
  const { builderConfig, updateBuilderConfig } = useAppStore();
  const bb = builderConfig.buildingBlocks;
  const wtb = builderConfig.whatToBuild;
  const [letterFilter, setLetterFilter] = useState<string | null>(null);
  const [calibrateOpen, setCalibrateOpen] = useState(false);
  const [openSections, setOpenSections] = useState<Set<string>>(
    () => new Set(["indicators", "orderPriceIndicators"])
  );
  const toggleSection = (key: string) =>
    setOpenSections((prev) => {
      const next = new Set(prev);
      next.has(key) ? next.delete(key) : next.add(key);
      return next;
    });

  const updateIndicators = (indicators: BuilderIndicatorBlock[]) =>
    updateBuilderConfig({ buildingBlocks: { ...bb, indicators } });

  const updateOrderTypes = (orderTypes: BuilderOrderTypeBlock[]) =>
    updateBuilderConfig({ buildingBlocks: { ...bb, orderTypes } });

  const updateExitTypes = (exitTypes: BuilderExitTypeBlock[]) =>
    updateBuilderConfig({ buildingBlocks: { ...bb, exitTypes } });

  const updateOrderPriceIndicators = (orderPriceIndicators: BuilderOrderPriceBlock[]) =>
    updateBuilderConfig({ buildingBlocks: { ...bb, orderPriceIndicators } });

  const updateOrderPriceBaseStop = (orderPriceBaseStop: OrderPriceBaseField) =>
    updateBuilderConfig({ buildingBlocks: { ...bb, orderPriceBaseStop } });

  const updateOrderPriceBaseLimit = (orderPriceBaseLimit: OrderPriceBaseField) =>
    updateBuilderConfig({ buildingBlocks: { ...bb, orderPriceBaseLimit } });

  const toggleIndicator = (idx: number) => {
    const next = bb.indicators.map((ind, i) =>
      i === idx ? { ...ind, enabled: !ind.enabled } : ind
    );
    updateIndicators(next);
  };

  const setIndicatorWeight = (idx: number, weight: number) => {
    const next = bb.indicators.map((ind, i) => (i === idx ? { ...ind, weight } : ind));
    updateIndicators(next);
  };

  const setIndicatorPeriod = (idx: number, min: number, max: number, step: number) => {
    const next = bb.indicators.map((ind, i) =>
      i === idx ? { ...ind, periodMin: min, periodMax: max, periodStep: step } : ind
    );
    updateIndicators(next);
  };

  const resetIndicatorPeriod = (idx: number) => {
    const next = bb.indicators.map((ind, i) =>
      i === idx ? { ...ind, periodMin: undefined, periodMax: undefined, periodStep: undefined } : ind
    );
    updateIndicators(next);
  };

  const selectAll = (enabled: boolean) =>
    updateIndicators(bb.indicators.map((ind) => ({ ...ind, enabled })));

  const filteredIndicators = letterFilter
    ? bb.indicators.filter((ind) =>
        ind.indicatorType.toUpperCase().startsWith(letterFilter)
      )
    : bb.indicators;

  const enabledCount = bb.indicators.filter((i) => i.enabled).length;
  const customCount = bb.indicators.filter((i) => i.periodMin !== undefined).length;

  return (
    <div className="flex flex-col gap-4 p-4">
      {/* Indicators section */}
      <AccordionSection
        title={t("buildingBlocks.indicators")}
        open={openSections.has("indicators")}
        onToggle={() => toggleSection("indicators")}
        badge={`${enabledCount}/${bb.indicators.length}`}
      >
        <p className="mb-3 text-[10px] text-muted-foreground/60">
          {t("buildingBlocks.indicatorsDescription")}
        </p>

        {/* Alphabet filter + controls */}
        <div className="mb-2 flex flex-wrap items-center gap-0.5">
          <button
            onClick={() => setLetterFilter(null)}
            className={cn(
              "rounded px-1.5 py-0.5 text-[10px] font-medium transition-colors",
              letterFilter === null
                ? "bg-primary text-primary-foreground"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            All
          </button>
          {ALPHABET.map((letter) => {
            const hasMatch = bb.indicators.some((i) =>
              i.indicatorType.toUpperCase().startsWith(letter)
            );
            if (!hasMatch) return null;
            return (
              <button
                key={letter}
                onClick={() => setLetterFilter(letter === letterFilter ? null : letter)}
                className={cn(
                  "rounded px-1 py-0.5 text-[10px] font-medium transition-colors",
                  letterFilter === letter
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:text-foreground"
                )}
              >
                {letter}
              </button>
            );
          })}
          <div className="ml-auto flex items-center gap-2">
            <span className="text-[10px] text-muted-foreground/60">
              {enabledCount}/{bb.indicators.length} activos
            </span>
            <button
              onClick={() => selectAll(true)}
              className="rounded border border-border/40 px-2 py-0.5 text-[10px] text-muted-foreground hover:border-primary/50 hover:text-primary"
            >
              {t("buildingBlocks.selectAll")}
            </button>
            <button
              onClick={() => selectAll(false)}
              className="rounded border border-border/40 px-2 py-0.5 text-[10px] text-muted-foreground hover:border-primary/50 hover:text-primary"
            >
              {t("buildingBlocks.filterReset")}
            </button>
          </div>
        </div>

        {/* Table header */}
        <div className="grid grid-cols-[24px_1fr_56px_100px] gap-1 border-b border-border/30 px-1 pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
          <span />
          <span>{t("buildingBlocks.columns.name")}</span>
          <span className="text-center">{t("buildingBlocks.columns.weight")}</span>
          <span className="text-center">{t("buildingBlocks.columns.parameters")}</span>
        </div>

        {/* Rows */}
        <div className="max-h-56 overflow-y-auto">
          {filteredIndicators.map((ind) => {
            const globalIdx = bb.indicators.indexOf(ind);
            return (
              <div
                key={ind.indicatorType}
                className={cn(
                  "grid grid-cols-[24px_1fr_56px_100px] items-center gap-1 border-b border-border/10 px-1 py-1",
                  ind.enabled ? "" : "opacity-40"
                )}
              >
                <input
                  type="checkbox"
                  checked={ind.enabled}
                  onChange={() => toggleIndicator(globalIdx)}
                  className="h-3.5 w-3.5 accent-primary"
                />
                <span className="truncate text-sm text-foreground">{ind.indicatorType}</span>
                <WeightInput
                  value={ind.weight}
                  onChange={(v) => setIndicatorWeight(globalIdx, v)}
                />
                <PeriodPopover
                  ind={ind}
                  globalMin={wtb.indicatorPeriodMin}
                  globalMax={wtb.indicatorPeriodMax}
                  globalStep={wtb.indicatorPeriodStep}
                  onSave={(min, max, step) => setIndicatorPeriod(globalIdx, min, max, step)}
                  onReset={() => resetIndicatorPeriod(globalIdx)}
                />
              </div>
            );
          })}
        </div>

        {/* Calibrate button */}
        <div className="mt-3 flex items-center justify-between">
          {customCount > 0 && (
            <span className="text-[10px] text-primary/70">
              {customCount} indicador{customCount !== 1 ? "es" : ""} con rango personalizado
            </span>
          )}
          <button
            onClick={() => setCalibrateOpen(true)}
            className="ml-auto flex items-center gap-1.5 rounded border border-border/40 px-3 py-1.5 text-xs font-medium text-muted-foreground hover:border-primary/50 hover:text-primary"
          >
            <Settings2 className="h-3.5 w-3.5" />
            Calibrar indicadores
          </button>
        </div>
      </AccordionSection>

      {/* Order Price Indicators section */}
      <AccordionSection
        title="Indicadores de Precio de Orden"
        open={openSections.has("orderPriceIndicators")}
        onToggle={() => toggleSection("orderPriceIndicators")}
        badge={`${(bb.orderPriceIndicators ?? []).filter((o) => o.enabled).length}/${(bb.orderPriceIndicators ?? []).length}`}
      >
        <p className="mb-3 text-[10px] text-muted-foreground/60">
          El valor primario del indicador se usa como offset para calcular el precio de órdenes Stop/Limit (en lugar del offset fijo en pips).
        </p>
        {/* Base price selectors */}
        <div className="mb-3 grid grid-cols-2 gap-3">
          <div className="flex flex-col gap-1">
            <span className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">Base para Stop</span>
            <select
              value={bb.orderPriceBaseStop}
              onChange={(e) => updateOrderPriceBaseStop(e.target.value as OrderPriceBaseField)}
              className="rounded border border-border/40 bg-muted/30 px-2 py-1.5 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
            >
              <option value="high">High</option>
              <option value="low">Low</option>
              <option value="close">Close</option>
              <option value="open">Open</option>
            </select>
            <span className="text-[9px] text-muted-foreground/50">Precio base al colocar orden Stop</span>
          </div>
          <div className="flex flex-col gap-1">
            <span className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">Base para Limit</span>
            <select
              value={bb.orderPriceBaseLimit}
              onChange={(e) => updateOrderPriceBaseLimit(e.target.value as OrderPriceBaseField)}
              className="rounded border border-border/40 bg-muted/30 px-2 py-1.5 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
            >
              <option value="low">Low</option>
              <option value="high">High</option>
              <option value="close">Close</option>
              <option value="open">Open</option>
            </select>
            <span className="text-[9px] text-muted-foreground/50">Precio base al colocar orden Limit</span>
          </div>
        </div>
        {/* Indicator table header */}
        <div className="grid grid-cols-[24px_1fr_56px] gap-1 border-b border-border/30 px-1 pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
          <span />
          <span>{t("buildingBlocks.columns.name")}</span>
          <span className="text-center">{t("buildingBlocks.columns.weight")}</span>
        </div>
        {/* Rows */}
        <div className="max-h-40 overflow-y-auto">
          {(bb.orderPriceIndicators ?? []).map((opi, i) => (
            <div
              key={opi.indicatorType}
              className={cn(
                "grid grid-cols-[24px_1fr_56px] items-center gap-1 border-b border-border/10 px-1 py-1",
                opi.enabled ? "" : "opacity-40"
              )}
            >
              <input
                type="checkbox"
                checked={opi.enabled}
                onChange={() => {
                  const next = (bb.orderPriceIndicators ?? []).map((o, j) =>
                    j === i ? { ...o, enabled: !o.enabled } : o
                  );
                  updateOrderPriceIndicators(next);
                }}
                className="h-3.5 w-3.5 accent-primary"
              />
              <span className="truncate text-sm text-foreground">{opi.indicatorType}</span>
              <WeightInput
                value={opi.weight}
                onChange={(v) => {
                  const next = (bb.orderPriceIndicators ?? []).map((o, j) =>
                    j === i ? { ...o, weight: v } : o
                  );
                  updateOrderPriceIndicators(next);
                }}
              />
            </div>
          ))}
        </div>
      </AccordionSection>

      <div className="grid grid-cols-2 gap-4">
        {/* Order types */}
        <SectionBox title={t("buildingBlocks.orderTypes")}>
          <div className="grid grid-cols-[24px_1fr_56px] gap-1 border-b border-border/30 px-1 pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
            <span />
            <span>{t("buildingBlocks.columns.name")}</span>
            <span className="text-center">{t("buildingBlocks.columns.weight")}</span>
          </div>
          {bb.orderTypes.map((ot, i) => (
            <div
              key={ot.orderType}
              className={cn(
                "grid grid-cols-[24px_1fr_56px] items-center gap-1 border-b border-border/10 px-1 py-1.5",
                ot.enabled ? "" : "opacity-40"
              )}
            >
              <input
                type="checkbox"
                checked={ot.enabled}
                onChange={() => {
                  const next = bb.orderTypes.map((o, j) =>
                    j === i ? { ...o, enabled: !o.enabled } : o
                  );
                  updateOrderTypes(next);
                }}
                className="h-3.5 w-3.5 accent-primary"
              />
              <span className="flex items-center gap-1.5 text-sm text-foreground">
                {ORDER_TYPE_LABELS[ot.orderType]}
                {ORDER_PENDING.has(ot.orderType) && (
                  <span className="rounded bg-muted/60 px-1 py-px text-[9px] font-medium text-muted-foreground/50">próx.</span>
                )}
              </span>
              <WeightInput
                value={ot.weight}
                onChange={(v) => {
                  const next = bb.orderTypes.map((o, j) =>
                    j === i ? { ...o, weight: v } : o
                  );
                  updateOrderTypes(next);
                }}
              />
            </div>
          ))}
        </SectionBox>

        {/* Exit types */}
        <SectionBox title={t("buildingBlocks.exitTypes")}>
          <div className="grid grid-cols-[24px_1fr_48px] gap-1 border-b border-border/30 px-1 pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
            <span />
            <span>{t("buildingBlocks.columns2.use")}</span>
            <span className="text-center">{t("buildingBlocks.columns2.required")}</span>
          </div>
          {bb.exitTypes.map((et, i) => (
            <div
              key={et.exitType}
              className={cn(
                "grid grid-cols-[24px_1fr_48px] items-center gap-1 border-b border-border/10 px-1 py-1.5",
                et.enabled ? "" : "opacity-40"
              )}
            >
              <input
                type="checkbox"
                checked={et.enabled}
                onChange={() => {
                  const next = bb.exitTypes.map((e, j) =>
                    j === i ? { ...e, enabled: !e.enabled, required: !e.enabled ? e.required : false } : e
                  );
                  updateExitTypes(next);
                }}
                className="h-3.5 w-3.5 accent-primary"
              />
              <span className="flex items-center gap-1.5 text-sm text-foreground">
                {EXIT_TYPE_LABELS[et.exitType]}
                {EXIT_PENDING.has(et.exitType) && (
                  <span className="rounded bg-muted/60 px-1 py-px text-[9px] font-medium text-muted-foreground/50">próx.</span>
                )}
              </span>
              <div className="flex justify-center">
                <input
                  type="checkbox"
                  checked={et.required}
                  disabled={!et.enabled}
                  onChange={() => {
                    const next = bb.exitTypes.map((e, j) =>
                      j === i ? { ...e, required: !e.required } : e
                    );
                    updateExitTypes(next);
                  }}
                  className="h-3.5 w-3.5 accent-amber-500 disabled:opacity-30"
                />
              </div>
            </div>
          ))}
          <p className="mt-1 text-[10px] text-muted-foreground/50">
            "Required" = siempre incluido en cada estrategia generada
          </p>
        </SectionBox>
      </div>

      {/* Calibrate dialog */}
      <CalibrateDialog
        open={calibrateOpen}
        onClose={() => setCalibrateOpen(false)}
        indicators={bb.indicators}
        globalMin={wtb.indicatorPeriodMin}
        globalMax={wtb.indicatorPeriodMax}
        globalStep={wtb.indicatorPeriodStep}
        onApply={updateIndicators}
      />
    </div>
  );
}
