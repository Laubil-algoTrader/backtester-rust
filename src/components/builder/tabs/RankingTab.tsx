import { useTranslation } from "react-i18next";
import { nanoid } from "nanoid";
import { useAppStore } from "@/stores/useAppStore";
import { SpinnerInput, SectionBox, Toggle, ConfigRow } from "./WhatToBuildTab";
import type {
  BuilderFilterCondition,
  BuilderFilterOperator,
  BuilderFitnessSource,
  BuilderComputeFrom,
  BuilderStopWhen,
  BuilderWeightedCriterion,
} from "@/lib/types";
import { cn } from "@/lib/utils";

const METRIC_OPTIONS = [
  "net_profit", "win_rate_pct", "profit_factor", "sharpe_ratio",
  "sortino_ratio", "calmar_ratio", "max_drawdown_pct", "return_dd_ratio",
  "avg_bars_in_trade", "total_trades", "r_expectancy", "ulcer_index_pct",
  "recovery_factor", "expectancy", "cagr",
];

const OPERATORS: BuilderFilterOperator[] = [">=", ">", "<=", "<", "=="];

type FilterTableProps = {
  filters: BuilderFilterCondition[];
  onChange: (filters: BuilderFilterCondition[]) => void;
};

function FilterTable({ filters, onChange }: FilterTableProps) {
  const { t } = useTranslation("builder");

  const add = () =>
    onChange([
      ...filters,
      { id: nanoid(6), leftValue: "win_rate_pct", operator: ">=", rightValue: 50 },
    ]);

  const remove = (id: string) => onChange(filters.filter((f) => f.id !== id));

  const update = (id: string, partial: Partial<BuilderFilterCondition>) =>
    onChange(filters.map((f) => (f.id === id ? { ...f, ...partial } : f)));

  return (
    <div className="space-y-1">
      {filters.length > 0 && (
        <div className="grid grid-cols-[1fr_52px_80px_20px] gap-1 px-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
          <span>{t("geneticOptions.filterColumns.leftValue")}</span>
          <span>{t("geneticOptions.filterColumns.operator")}</span>
          <span>{t("geneticOptions.filterColumns.rightValue")}</span>
          <span />
        </div>
      )}
      {filters.map((f) => (
        <div key={f.id} className="grid grid-cols-[1fr_52px_80px_20px] items-center gap-1">
          <select
            value={f.leftValue}
            onChange={(e) => update(f.id, { leftValue: e.target.value })}
            className="rounded border border-border bg-muted/30 px-1.5 py-1 text-[11px] text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          >
            {METRIC_OPTIONS.map((m) => (
              <option key={m} value={m}>{m}</option>
            ))}
          </select>
          <select
            value={f.operator}
            onChange={(e) => update(f.id, { operator: e.target.value as BuilderFilterOperator })}
            className="rounded border border-border bg-muted/30 px-1.5 py-1 text-[11px] text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          >
            {OPERATORS.map((op) => (
              <option key={op} value={op}>{op}</option>
            ))}
          </select>
          <input
            type="number"
            value={f.rightValue}
            onChange={(e) => update(f.id, { rightValue: Number(e.target.value) })}
            step={0.01}
            className="rounded border border-border bg-muted/30 px-2 py-1 text-[11px] text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          />
          <button
            onClick={() => remove(f.id)}
            className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground/60 hover:text-destructive"
          >
            ×
          </button>
        </div>
      ))}
      <button
        onClick={add}
        className="mt-1 rounded border border-border/40 px-3 py-1 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary"
      >
        + {t("geneticOptions.addCondition")}
      </button>
    </div>
  );
}

type WeightedTableProps = {
  criteria: BuilderWeightedCriterion[];
  onChange: (c: BuilderWeightedCriterion[]) => void;
};

function WeightedCriteriaTable({ criteria, onChange }: WeightedTableProps) {
  const { t } = useTranslation("builder");

  const add = () =>
    onChange([
      ...criteria,
      { id: nanoid(6), criterium: "net_profit", type: "maximize", weight: 1, target: 0 },
    ]);

  const remove = (id: string) => onChange(criteria.filter((c) => c.id !== id));

  const update = (id: string, partial: Partial<BuilderWeightedCriterion>) =>
    onChange(criteria.map((c) => (c.id === id ? { ...c, ...partial } : c)));

  return (
    <div className="space-y-1">
      {criteria.length > 0 && (
        <div className="grid grid-cols-[1fr_76px_52px_64px_20px] gap-1 px-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
          <span>Criterium</span>
          <span>{t("ranking.type")}</span>
          <span>{t("ranking.weight")}</span>
          <span>{t("ranking.target")}</span>
          <span />
        </div>
      )}
      {criteria.map((c) => (
        <div key={c.id} className="grid grid-cols-[1fr_76px_52px_64px_20px] items-center gap-1">
          <select
            value={c.criterium}
            onChange={(e) => update(c.id, { criterium: e.target.value })}
            className="rounded border border-border bg-muted/30 px-1.5 py-1 text-[11px] text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          >
            {METRIC_OPTIONS.map((m) => (
              <option key={m} value={m}>{m}</option>
            ))}
          </select>
          <select
            value={c.type}
            onChange={(e) => update(c.id, { type: e.target.value as "minimize" | "maximize" })}
            className="rounded border border-border bg-muted/30 px-1 py-1 text-[11px] text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          >
            <option value="maximize">{t("ranking.maximize")}</option>
            <option value="minimize">{t("ranking.minimize")}</option>
          </select>
          <input
            type="number"
            min={1}
            max={100}
            value={c.weight}
            onChange={(e) => update(c.id, { weight: Number(e.target.value) })}
            className="rounded border border-border bg-muted/30 px-2 py-1 text-center text-[11px] text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          />
          <input
            type="number"
            value={c.target}
            onChange={(e) => update(c.id, { target: Number(e.target.value) })}
            step={0.01}
            className="rounded border border-border bg-muted/30 px-2 py-1 text-[11px] text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
          />
          <button
            onClick={() => remove(c.id)}
            className="flex h-5 w-5 items-center justify-center rounded text-muted-foreground/60 hover:text-destructive"
          >
            ×
          </button>
        </div>
      ))}
      <button
        onClick={add}
        className="mt-1 rounded border border-border/40 px-3 py-1 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary"
      >
        + Add criterium
      </button>
    </div>
  );
}

export function RankingTab() {
  const { t } = useTranslation("builder");
  const { builderConfig, updateBuilderConfig } = useAppStore();
  const rk = builderConfig.ranking;
  const update = (partial: Partial<typeof rk>) =>
    updateBuilderConfig({ ranking: { ...rk, ...partial } });

  const STOP_OPTIONS: BuilderStopWhen[] = ["never", "totally", "databank_full", "after_time"];

  return (
    <div className="grid grid-cols-2 gap-4 p-4">
      {/* Left column */}
      <div className="space-y-4">
        <SectionBox title={t("ranking.maxStrategies")}>
          <div className="flex flex-col gap-2">
            <span className="text-[10px] text-muted-foreground/60">
              {t("ranking.maxStrategiesLabel")}
            </span>
            <SpinnerInput
              value={rk.maxStrategiesToStore}
              onChange={(v) => update({ maxStrategiesToStore: v })}
              min={1}
              max={100_000}
              step={100}
              className="w-full"
            />
          </div>
        </SectionBox>

        <SectionBox title={t("ranking.stopGeneration")}>
          <div className="space-y-2">
            {STOP_OPTIONS.map((opt) => (
              <label key={opt} className="flex cursor-pointer items-center gap-2">
                <input
                  type="radio"
                  name="stop-when"
                  value={opt}
                  checked={rk.stopWhen === opt}
                  onChange={() => update({ stopWhen: opt })}
                  className="accent-primary"
                />
                <span className="text-sm text-foreground">
                  {opt === "never" && t("ranking.stopOptions.never")}
                  {opt === "totally" && (
                    <span className="flex items-center gap-2">
                      {t("ranking.stopOptions.totally")}
                      {rk.stopWhen === "totally" && (
                        <SpinnerInput
                          value={rk.stopTotallyCount}
                          onChange={(v) => update({ stopTotallyCount: v })}
                          min={1}
                          max={100_000}
                          step={100}
                        />
                      )}
                      {t("ranking.stopOptions.strategiesGenerated")}
                    </span>
                  )}
                  {opt === "databank_full" && t("ranking.stopOptions.databankFull")}
                  {opt === "after_time" && (
                    <span className="flex items-center gap-1.5">
                      {t("ranking.stopOptions.after")}
                      {rk.stopWhen === "after_time" && (
                        <>
                          <SpinnerInput value={rk.stopAfterDays} onChange={(v) => update({ stopAfterDays: v })} min={0} max={365} step={1} />
                          <span className="text-[10px] text-muted-foreground/60">{t("ranking.days")}</span>
                          <SpinnerInput value={rk.stopAfterHours} onChange={(v) => update({ stopAfterHours: v })} min={0} max={23} step={1} />
                          <span className="text-[10px] text-muted-foreground/60">{t("ranking.hours")}</span>
                          <SpinnerInput value={rk.stopAfterMinutes} onChange={(v) => update({ stopAfterMinutes: v })} min={0} max={59} step={1} />
                          <span className="text-[10px] text-muted-foreground/60">{t("ranking.minutes")}</span>
                        </>
                      )}
                    </span>
                  )}
                </span>
              </label>
            ))}
          </div>
        </SectionBox>

        <SectionBox title={t("ranking.filteringConditions")}>
          <p className="mb-3 text-[10px] text-muted-foreground/60">
            {t("ranking.filterDescription")}
          </p>
          <FilterTable
            filters={rk.customFilters}
            onChange={(f) => update({ customFilters: f })}
          />
          <div className="mt-3 border-t border-border/20 pt-3">
            <ConfigRow label={t("ranking.dismissSimilar")}>
              <Toggle
                checked={rk.dismissSimilar}
                onChange={(v) => update({ dismissSimilar: v })}
              />
            </ConfigRow>
          </div>
        </SectionBox>
      </div>

      {/* Right column */}
      <div className="space-y-4">
        <SectionBox title={t("ranking.strategyQuality")}>
          <div className="space-y-3">
            <ConfigRow label={t("ranking.use")}>
              <select
                value={rk.fitnessSource}
                onChange={(e) => update({ fitnessSource: e.target.value as BuilderFitnessSource })}
                className="rounded border border-border bg-muted/30 px-2 py-1.5 text-sm h-8 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
              >
                <option value="main_data">{t("ranking.fitnessOptions.mainData")}</option>
                <option value="in_sample">{t("ranking.fitnessOptions.inSample")}</option>
                <option value="out_of_sample">{t("ranking.fitnessOptions.outOfSample")}</option>
                <option value="full">{t("ranking.fitnessOptions.full")}</option>
              </select>
            </ConfigRow>

            <ConfigRow label={t("ranking.computeFrom")}>
              <select
                value={rk.computeFrom}
                onChange={(e) => update({ computeFrom: e.target.value as BuilderComputeFrom })}
                className="rounded border border-border bg-muted/30 px-2 py-1.5 text-sm h-8 text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
              >
                <option value="net_profit">{t("ranking.computeOptions.netProfit")}</option>
                <option value="return_dd">{t("ranking.computeOptions.returnDD")}</option>
                <option value="r_expectancy">{t("ranking.computeOptions.rExpectancy")}</option>
                <option value="annual_max_dd">{t("ranking.computeOptions.annualMaxDD")}</option>
                <option value="weighted_fitness">{t("ranking.computeOptions.weightedFitness")}</option>
              </select>
            </ConfigRow>
          </div>

          {rk.computeFrom === "weighted_fitness" && (
            <div className="mt-3 border-t border-border/20 pt-3">
              <p className="mb-2 text-[10px] font-medium uppercase tracking-wider text-muted-foreground/60">
                {t("ranking.rankingCriteria")}
              </p>
              <WeightedCriteriaTable
                criteria={rk.weightedCriteria}
                onChange={(c) => update({ weightedCriteria: c })}
              />
            </div>
          )}
        </SectionBox>

        <div
          className={cn(
            "rounded-md border border-border/30 bg-muted/10 p-3 text-[10px] text-muted-foreground/70"
          )}
        >
          <p className="mb-1 font-medium text-muted-foreground">{t("ranking.crossCheckFilters")}</p>
          <p>{t("ranking.crossCheckFiltersNote")}</p>
        </div>
      </div>
    </div>
  );
}
