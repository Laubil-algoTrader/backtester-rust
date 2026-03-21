import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/useAppStore";
import { SectionBox, Toggle } from "./WhatToBuildTab";
import type { BuilderCrossChecks } from "@/lib/types";
import { cn } from "@/lib/utils";

type CheckKey = keyof Omit<BuilderCrossChecks, "disableAll">;

interface CheckItem {
  key: CheckKey;
  labelKey: string;
  badge: "basic" | "standard" | "extensive";
}

const BASIC_CHECKS: CheckItem[] = [
  { key: "whatIf", labelKey: "whatIf", badge: "basic" },
  { key: "monteCarlo", labelKey: "monteCarlo", badge: "basic" },
  { key: "higherPrecision", labelKey: "higherPrecision", badge: "basic" },
  { key: "additionalMarkets", labelKey: "additionalMarkets", badge: "basic" },
];

const STANDARD_CHECKS: CheckItem[] = [
  { key: "monteCarloRetest", labelKey: "monteCarloRetest", badge: "standard" },
  { key: "sequentialOpt", labelKey: "sequentialOpt", badge: "standard" },
];

const EXTENSIVE_CHECKS: CheckItem[] = [
  { key: "walkForward", labelKey: "walkForward", badge: "extensive" },
  { key: "walkForwardMatrix", labelKey: "walkForwardMatrix", badge: "extensive" },
];

const BADGE_CLASSES = {
  basic: "bg-emerald-600/20 text-emerald-400 border-emerald-600/30",
  standard: "bg-amber-600/20 text-amber-400 border-amber-600/30",
  extensive: "bg-red-600/20 text-red-400 border-red-600/30",
};

function CheckSection({
  title,
  description,
  checks,
  values,
  onChange,
  disabled,
}: {
  title: string;
  description: string;
  checks: CheckItem[];
  values: BuilderCrossChecks;
  onChange: (partial: Partial<BuilderCrossChecks>) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation("builder");
  const enabledCount = checks.filter((c) => values[c.key]).length;

  return (
    <SectionBox
      title={
        <div className="flex items-center gap-2">
          <span>{title}</span>
          <span
            className={cn(
              "rounded border px-1.5 py-0.5 text-[10px] font-medium",
              BADGE_CLASSES[checks[0]?.badge ?? "basic"]
            )}
          >
            {enabledCount}/{checks.length}
          </span>
        </div>
      }
    >
      <p className="mb-2 text-[10px] text-muted-foreground/60">{description}</p>
      <div className="space-y-1.5">
        {checks.map((check) => (
          <div
            key={check.key}
            className={cn(
              "flex items-center justify-between rounded px-2 py-1.5",
              disabled || !values[check.key] ? "opacity-50" : "bg-muted/20"
            )}
          >
            <span className="text-sm text-foreground">
              {t(`crossChecks.checks.${check.labelKey}`)}
            </span>
            <Toggle
              checked={values[check.key]}
              onChange={(v) => onChange({ [check.key]: v } as Partial<BuilderCrossChecks>)}
              disabled={disabled}
            />
          </div>
        ))}
      </div>
    </SectionBox>
  );
}

export function CrossChecksTab() {
  const { t } = useTranslation("builder");
  const { builderConfig, updateBuilderConfig } = useAppStore();
  const cc = builderConfig.crossChecks;
  const update = (partial: Partial<BuilderCrossChecks>) =>
    updateBuilderConfig({ crossChecks: { ...cc, ...partial } });

  const activeCount = Object.entries(cc)
    .filter(([k, v]) => k !== "disableAll" && v === true)
    .length;

  return (
    <div className="flex flex-col gap-4 p-4">
      {/* Disable all toggle */}
      <div
        className={cn(
          "flex items-center justify-between rounded-md border px-4 py-3",
          cc.disableAll
            ? "border-destructive/40 bg-destructive/10"
            : "border-border/40 bg-muted/20"
        )}
      >
        <div>
          <p className="text-sm font-medium text-foreground">{t("crossChecks.disableAll")}</p>
          <p className="text-[10px] text-muted-foreground/60">
            {cc.disableAll
              ? "All cross checks disabled — strategies will only be evaluated on the main data."
              : `${activeCount} cross check(s) active`}
          </p>
        </div>
        <Toggle
          checked={cc.disableAll}
          onChange={(v) => update({ disableAll: v })}
        />
      </div>

      <div className="grid grid-cols-3 gap-4">
        <CheckSection
          title={t("crossChecks.basic")}
          description={t("crossChecks.basicDescription")}
          checks={BASIC_CHECKS}
          values={cc}
          onChange={update}
          disabled={cc.disableAll}
        />
        <CheckSection
          title={t("crossChecks.standard")}
          description={t("crossChecks.standardDescription")}
          checks={STANDARD_CHECKS}
          values={cc}
          onChange={update}
          disabled={cc.disableAll}
        />
        <CheckSection
          title={t("crossChecks.extensive")}
          description={t("crossChecks.extensiveDescription")}
          checks={EXTENSIVE_CHECKS}
          values={cc}
          onChange={update}
          disabled={cc.disableAll}
        />
      </div>

      {!cc.disableAll && (
        <div className="rounded-md border border-border/30 bg-muted/10 px-4 py-3 text-[10px] text-muted-foreground/70">
          <span className="font-medium text-muted-foreground">{t("crossChecks.crossCheckSettings")}: </span>
          Cross check filter thresholds are configured per cross-check method when available.
          Strategies failing cross checks can be filtered in the Ranking tab.
        </div>
      )}
    </div>
  );
}
