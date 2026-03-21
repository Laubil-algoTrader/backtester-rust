import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/useAppStore";
import { SpinnerInput, SectionBox, ConfigRow } from "./WhatToBuildTab";
import type { BuilderMMMethod } from "@/lib/types";
import { cn } from "@/lib/utils";

const MM_METHODS: { value: BuilderMMMethod; labelKey: string; description: string }[] = [
  { value: "fixed_size", labelKey: "fixedSize", description: "Always trade the same fixed lot size." },
  { value: "risk_fixed_balance", labelKey: "riskFixedBalance", description: "Risk a fixed % of starting balance per trade." },
  { value: "risk_fixed_account", labelKey: "riskFixedAccount", description: "Risk a fixed % of current account equity per trade." },
  { value: "fixed_amount", labelKey: "fixedAmount", description: "Risk a fixed monetary amount per trade." },
  { value: "crypto_by_price", labelKey: "cryptoByPrice", description: "Position size = risked money ÷ current price (crypto)." },
  { value: "stocks_by_price", labelKey: "stocksByPrice", description: "Position size = risked money ÷ current price (stocks)." },
  { value: "simple_martingale", labelKey: "simpleMartingale", description: "Double position size after each loss (use with caution)." },
];

export function MoneyManagementTab() {
  const { t } = useTranslation("builder");
  const { builderConfig, updateBuilderConfig } = useAppStore();
  const mm = builderConfig.moneyManagement;
  const update = (partial: Partial<typeof mm>) =>
    updateBuilderConfig({ moneyManagement: { ...mm, ...partial } });

  const showRiskedMoney = [
    "risk_fixed_balance",
    "risk_fixed_account",
    "fixed_amount",
    "crypto_by_price",
    "stocks_by_price",
    "simple_martingale",
  ].includes(mm.method);

  const isPercentage = mm.method === "risk_fixed_balance" || mm.method === "risk_fixed_account";

  return (
    <div className="grid grid-cols-2 gap-4 p-4">
      {/* Left: capital + method selector */}
      <div className="space-y-4">
        <SectionBox title={t("moneyManagement.initialCapital")}>
          <div className="flex flex-col gap-2">
            <span className="text-[10px] text-muted-foreground/60">
              {t("moneyManagement.initialCapitalLabel")}
            </span>
            <div className="flex items-center gap-2">
              <span className="text-lg font-semibold text-foreground">$</span>
              <input
                type="number"
                min={100}
                max={100_000_000}
                step={1000}
                value={mm.initialCapital}
                onChange={(e) => update({ initialCapital: Number(e.target.value) })}
                className="w-full rounded border border-border bg-muted/30 px-3 py-1.5 text-lg font-semibold text-foreground focus:outline-none focus:ring-1 focus:ring-primary"
              />
            </div>
            <p className="text-[10px] text-muted-foreground/50">
              ${mm.initialCapital.toLocaleString()}
            </p>
          </div>
        </SectionBox>

        <SectionBox title={t("moneyManagement.chooseMethod")}>
          <div className="space-y-1">
            {MM_METHODS.map((m) => (
              <label
                key={m.value}
                className={cn(
                  "flex cursor-pointer items-start gap-2.5 rounded p-2 transition-colors",
                  mm.method === m.value
                    ? "bg-primary/10 ring-1 ring-primary/30"
                    : "hover:bg-muted/50"
                )}
              >
                <input
                  type="radio"
                  name="mm-method"
                  value={m.value}
                  checked={mm.method === m.value}
                  onChange={() => update({ method: m.value })}
                  className="mt-0.5 accent-primary"
                />
                <div>
                  <p className="text-sm font-medium text-foreground">
                    {t(`moneyManagement.methods.${m.labelKey}`)}
                  </p>
                  <p className="text-[10px] text-muted-foreground/60">{m.description}</p>
                </div>
              </label>
            ))}
          </div>
        </SectionBox>
      </div>

      {/* Right: method-specific config */}
      <div className="space-y-4">
        <SectionBox title="Method configuration">
          <div className="space-y-2.5">
            {showRiskedMoney && (
              <ConfigRow label={t("moneyManagement.riskedMoney")}>
                <div className="flex items-center gap-1">
                  {!isPercentage && <span className="text-sm text-muted-foreground">$</span>}
                  <SpinnerInput
                    value={mm.riskedMoney}
                    onChange={(v) => update({ riskedMoney: v })}
                    min={isPercentage ? 0.01 : 1}
                    max={isPercentage ? 100 : 1_000_000}
                    step={isPercentage ? 0.1 : 100}
                  />
                  {isPercentage && <span className="text-[10px] text-muted-foreground/60">%</span>}
                </div>
              </ConfigRow>
            )}

            <ConfigRow label={t("moneyManagement.sizeDecimals")}>
              <SpinnerInput
                value={mm.sizeDecimals}
                onChange={(v) => update({ sizeDecimals: v })}
                min={0}
                max={6}
                step={1}
              />
            </ConfigRow>

            <ConfigRow label={t("moneyManagement.sizeIfNoMM")}>
              <SpinnerInput
                value={mm.sizeIfNoMM}
                onChange={(v) => update({ sizeIfNoMM: v })}
                min={0.01}
                max={1000}
                step={0.01}
              />
            </ConfigRow>

            <ConfigRow label={t("moneyManagement.maximumLots")}>
              <SpinnerInput
                value={mm.maximumLots}
                onChange={(v) => update({ maximumLots: v })}
                min={0.01}
                max={10000}
                step={1}
              />
            </ConfigRow>
          </div>

          {mm.method === "simple_martingale" && (
            <div className="mt-3 rounded border border-amber-500/30 bg-amber-500/10 p-2">
              <p className="text-[10px] text-amber-400">
                Warning: Martingale strategies can lead to catastrophic losses. Always set a maximum lot size.
              </p>
            </div>
          )}
        </SectionBox>

        <SectionBox title="Summary">
          <div className="space-y-1 text-sm text-muted-foreground">
            <div className="flex justify-between">
              <span>Initial capital</span>
              <span className="font-medium text-foreground">${mm.initialCapital.toLocaleString()}</span>
            </div>
            <div className="flex justify-between">
              <span>Method</span>
              <span className="font-medium text-foreground">
                {t(`moneyManagement.methods.${MM_METHODS.find((m) => m.value === mm.method)?.labelKey ?? ""}`)}
              </span>
            </div>
            <div className="flex justify-between">
              <span>Size decimals</span>
              <span className="font-medium text-foreground">{mm.sizeDecimals}</span>
            </div>
            <div className="flex justify-between">
              <span>Max lots</span>
              <span className="font-medium text-foreground">{mm.maximumLots}</span>
            </div>
          </div>
        </SectionBox>
      </div>
    </div>
  );
}
