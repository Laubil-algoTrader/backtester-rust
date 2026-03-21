import { useTranslation } from "react-i18next";
import { nanoid } from "nanoid";
import { useAppStore } from "@/stores/useAppStore";
import { SpinnerInput, SectionBox, Toggle } from "./WhatToBuildTab";
import type { BuilderFilterCondition, BuilderStagnationSample } from "@/lib/types";

const METRIC_OPTIONS = [
  { value: "win_rate_pct", label: "Winning Percent (IS)" },
  { value: "avg_bars_in_trade", label: "Avg. Bars in Trade (IS)" },
  { value: "return_dd_ratio", label: "Ret/DD Ratio (IS)" },
  { value: "profit_factor", label: "Profit Factor (IS)" },
  { value: "sharpe_ratio", label: "Sharpe Ratio (IS)" },
  { value: "total_trades", label: "# of Trades (IS)" },
  { value: "net_profit", label: "Net Profit (IS)" },
  { value: "max_drawdown_pct", label: "Max Drawdown % (IS)" },
  { value: "ulcer_index_pct", label: "Ulcer Index % (IS)" },
];

const OPERATORS = [">=", ">", "<=", "<", "=="] as const;

function FilterTable({
  filters,
  onChange,
}: {
  filters: BuilderFilterCondition[];
  onChange: (f: BuilderFilterCondition[]) => void;
}) {
  const addRow = () =>
    onChange([...filters, { id: nanoid(6), leftValue: "win_rate_pct", operator: ">=", rightValue: 40 }]);

  const removeRow = (id: string) => onChange(filters.filter((f) => f.id !== id));

  const updateRow = (id: string, partial: Partial<BuilderFilterCondition>) =>
    onChange(filters.map((f) => (f.id === id ? { ...f, ...partial } : f)));

  return (
    <div className="space-y-1">
      <div className="grid grid-cols-[1fr_80px_100px_24px] gap-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60 px-1">
        <span>Left value</span>
        <span>Op</span>
        <span>Right value</span>
        <span />
      </div>
      {filters.map((f) => (
        <div key={f.id} className="grid grid-cols-[1fr_80px_100px_24px] items-center gap-1">
          <select
            value={f.leftValue}
            onChange={(e) => updateRow(f.id, { leftValue: e.target.value })}
            className="rounded border border-border bg-muted/30 px-2 py-1.5 text-sm h-8 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          >
            {METRIC_OPTIONS.map((o) => (
              <option key={o.value} value={o.value}>{o.label}</option>
            ))}
          </select>
          <select
            value={f.operator}
            onChange={(e) => updateRow(f.id, { operator: e.target.value as BuilderFilterCondition["operator"] })}
            className="rounded border border-border bg-muted/30 px-2 py-1.5 text-sm h-8 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          >
            {OPERATORS.map((op) => <option key={op} value={op}>{op}</option>)}
          </select>
          <input
            type="number"
            value={f.rightValue}
            onChange={(e) => updateRow(f.id, { rightValue: Number(e.target.value) })}
            className="rounded border border-border bg-muted/30 px-2 py-1.5 text-sm h-8 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          />
          <button
            onClick={() => removeRow(f.id)}
            className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground/60 hover:text-destructive"
          >
            ×
          </button>
        </div>
      ))}
      <button
        onClick={addRow}
        className="mt-1 rounded border border-border/40 px-3 py-1 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary"
      >
        + Add conditions
      </button>
    </div>
  );
}

function SliderRow({
  label,
  value,
  onChange,
  min,
  max,
  suffix = "%",
}: {
  label: string;
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  suffix?: string;
}) {
  return (
    <div className="flex items-center gap-3">
      <span className="w-48 shrink-0 text-sm text-muted-foreground">{label}</span>
      <span className="w-10 shrink-0 text-right text-sm font-medium text-foreground">
        {value}{suffix}
      </span>
      <input
        type="range"
        min={min}
        max={max}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="flex-1 accent-primary"
      />
    </div>
  );
}

export function GeneticOptionsTab() {
  const { t } = useTranslation("builder");
  const { builderConfig, updateBuilderConfig } = useAppStore();
  const go = builderConfig.geneticOptions;
  const update = (partial: Partial<typeof go>) =>
    updateBuilderConfig({ geneticOptions: { ...go, ...partial } });

  const stagnationLabels: Record<string, string> = {
    in_sample: t("geneticOptions.stagnationSample.inSample"),
    out_of_sample: t("geneticOptions.stagnationSample.outOfSample"),
    full: t("geneticOptions.stagnationSample.full"),
  };

  return (
    <div className="grid grid-cols-2 gap-4 p-4">
      {/* Left column */}
      <div className="space-y-4">
        <SectionBox title={t("geneticOptions.geneticOptions")}>
          <div className="space-y-2.5">
            <div className="flex items-center justify-between">
              <span className="text-sm text-muted-foreground">{t("geneticOptions.maxGenerations")}</span>
              <SpinnerInput value={go.maxGenerations} onChange={(v) => update({ maxGenerations: v })} min={1} max={1000} />
            </div>
            <div className="flex items-center justify-between">
              <span className="text-sm text-muted-foreground">{t("geneticOptions.populationSize")}</span>
              <SpinnerInput value={go.populationPerIsland} onChange={(v) => update({ populationPerIsland: v })} min={5} max={500} />
            </div>
            <SliderRow label={t("geneticOptions.crossoverProbability")} value={go.crossoverProbability} onChange={(v) => update({ crossoverProbability: v })} min={0} max={100} />
            <SliderRow label={t("geneticOptions.mutationProbability")} value={go.mutationProbability} onChange={(v) => update({ mutationProbability: v })} min={0} max={100} />
          </div>
        </SectionBox>

        <SectionBox title={t("geneticOptions.initialPopulation")}>
          <div className="space-y-2.5">
            <div className="flex items-center justify-between">
              <span className="text-sm text-muted-foreground">{t("geneticOptions.initialPopulationSize")}</span>
              <SpinnerInput value={go.initialPopulationSize} onChange={(v) => update({ initialPopulationSize: v })} min={10} max={10000} />
            </div>
            <label className="flex items-start gap-2 cursor-pointer">
              <Toggle checked={go.useFromDatabank} onChange={(v) => update({ useFromDatabank: v })} />
              <div>
                <p className="text-sm text-foreground">{t("geneticOptions.useFromDatabank")}</p>
                <p className="text-[10px] text-muted-foreground/60 mt-0.5">{t("geneticOptions.useDatabankNote")}</p>
              </div>
            </label>
            <div>
              <SliderRow label={t("geneticOptions.decimationCoefficient")} value={go.decimationCoefficient} onChange={(v) => update({ decimationCoefficient: v })} min={1} max={10} suffix="" />
              <p className="mt-1 text-[10px] text-muted-foreground/50 leading-relaxed">
                {t("geneticOptions.decimationNote")}
              </p>
            </div>
          </div>
        </SectionBox>

        <SectionBox title={t("geneticOptions.freshBlood")}>
          <div className="space-y-2.5">
            <label className="flex items-center gap-2 cursor-pointer">
              <Toggle checked={go.freshBloodDetectDuplicates} onChange={(v) => update({ freshBloodDetectDuplicates: v })} />
              <span className="text-sm text-foreground">{t("geneticOptions.detectDuplicates")}</span>
            </label>
            <div className="flex items-center gap-2 text-sm flex-wrap">
              <span className="text-muted-foreground">{t("geneticOptions.replaceWeakest")}</span>
              <select
                value={go.freshBloodReplacePercent}
                onChange={(e) => update({ freshBloodReplacePercent: Number(e.target.value) })}
                className="rounded border border-border bg-muted/30 px-2 py-1.5 text-sm h-8 text-foreground"
              >
                {[5, 10, 15, 20, 25, 30].map((v) => <option key={v} value={v}>{v}</option>)}
              </select>
              <span className="text-muted-foreground">{t("geneticOptions.replaceWeakestOf")}</span>
              <span className="text-muted-foreground">{t("geneticOptions.replaceEvery")}</span>
              <SpinnerInput value={go.freshBloodReplaceEvery} onChange={(v) => update({ freshBloodReplaceEvery: v })} min={1} max={50} />
              <span className="text-muted-foreground">{t("geneticOptions.replaceGenerations")}</span>
            </div>
            <label className="flex items-center gap-2 cursor-pointer">
              <Toggle checked={go.showLastGeneration} onChange={(v) => update({ showLastGeneration: v })} />
              <span className="text-sm text-foreground">{t("geneticOptions.showLastGeneration")}</span>
            </label>
          </div>
        </SectionBox>
      </div>

      {/* Right column */}
      <div className="space-y-4">
        <SectionBox title={t("geneticOptions.islandsOptions")}>
          <div className="space-y-2.5">
            <div className="flex items-center justify-between">
              <span className="text-sm text-muted-foreground">{t("geneticOptions.islands")}</span>
              <SpinnerInput value={go.islands} onChange={(v) => update({ islands: v })} min={1} max={20} />
            </div>
            <SliderRow label={t("geneticOptions.migrateEvery")} value={go.migrateEveryN} onChange={(v) => update({ migrateEveryN: v })} min={1} max={50} suffix="" />
            <SliderRow label={t("geneticOptions.migrationRate")} value={go.migrationRate} onChange={(v) => update({ migrationRate: v })} min={1} max={50} />
          </div>
        </SectionBox>

        <SectionBox title={t("geneticOptions.filterInitialPopulation")}>
          <p className="mb-2 text-[10px] text-muted-foreground/60">{t("geneticOptions.filterDescription")}</p>
          <FilterTable
            filters={go.initialFilters}
            onChange={(f) => update({ initialFilters: f })}
          />
        </SectionBox>

        <SectionBox title={t("geneticOptions.evolutionManagement")}>
          <div className="space-y-2.5">
            <label className="flex items-center gap-2 cursor-pointer">
              <Toggle checked={go.startAgainWhenFinished} onChange={(v) => update({ startAgainWhenFinished: v })} />
              <span className="text-sm text-foreground">{t("geneticOptions.startAgain")}</span>
            </label>
            <div className="space-y-1.5">
              <label className="flex items-center gap-2 cursor-pointer">
                <Toggle checked={go.restartOnStagnation} onChange={(v) => update({ restartOnStagnation: v })} />
                <span className="text-sm text-foreground">{t("geneticOptions.restartIfStagnates")}</span>
              </label>
              {go.restartOnStagnation && (
                <div className="ml-11 flex items-center gap-2 flex-wrap">
                  <select
                    value={go.stagnationSample}
                    onChange={(e) => update({ stagnationSample: e.target.value as BuilderStagnationSample })}
                    className="rounded border border-border bg-muted/30 px-2 py-1.5 text-sm h-8 text-foreground"
                  >
                    {(["in_sample", "out_of_sample", "full"] as BuilderStagnationSample[]).map((s) => (
                      <option key={s} value={s}>{stagnationLabels[s]}</option>
                    ))}
                  </select>
                  <span className="text-sm text-muted-foreground">{t("geneticOptions.stagnatesFor")}</span>
                  <SpinnerInput value={go.stagnationGenerations} onChange={(v) => update({ stagnationGenerations: v })} min={1} max={1000} />
                  <span className="text-sm text-muted-foreground">{t("geneticOptions.stagnatesGenerations")}</span>
                </div>
              )}
            </div>
          </div>
        </SectionBox>
      </div>
    </div>
  );
}
