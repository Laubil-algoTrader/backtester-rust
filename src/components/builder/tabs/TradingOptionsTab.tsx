import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/useAppStore";
import { SpinnerInput, SectionBox, Toggle, ConfigRow } from "./WhatToBuildTab";
import type { BuilderOrderTypesToClose } from "@/lib/types";

const TIME_INPUT = "rounded border border-border bg-muted/30 px-2 py-1.5 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-primary w-24 h-8";

function TimeInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <input
      type="time"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className={TIME_INPUT}
    />
  );
}

export function TradingOptionsTab() {
  const { t } = useTranslation("builder");
  const { builderConfig, updateBuilderConfig } = useAppStore();
  const to = builderConfig.tradingOptions;
  const update = (partial: Partial<typeof to>) =>
    updateBuilderConfig({ tradingOptions: { ...to, ...partial } });

  return (
    <div className="grid grid-cols-2 gap-4 p-4">
      {/* Left column */}
      <div className="space-y-4">
        <SectionBox title={t("tradingOptions.tradingOptions")}>
          <div className="space-y-2">
            {/* Weekends */}
            <ConfigRow label={t("tradingOptions.dontTradeWeekends")}>
              <Toggle
                checked={to.dontTradeWeekends}
                onChange={(v) => update({ dontTradeWeekends: v })}
              />
            </ConfigRow>

            {to.dontTradeWeekends && (
              <>
                <ConfigRow label={t("tradingOptions.fridayCloseTime")} indent>
                  <TimeInput value={to.fridayCloseTime} onChange={(v) => update({ fridayCloseTime: v })} />
                </ConfigRow>
                <ConfigRow label={t("tradingOptions.sundayOpenTime")} indent>
                  <TimeInput value={to.sundayOpenTime} onChange={(v) => update({ sundayOpenTime: v })} />
                </ConfigRow>
              </>
            )}

            {/* Exit at end of day */}
            <ConfigRow label={t("tradingOptions.exitAtEndOfDay")}>
              <Toggle
                checked={to.exitAtEndOfDay}
                onChange={(v) => update({ exitAtEndOfDay: v })}
              />
            </ConfigRow>
            {to.exitAtEndOfDay && (
              <ConfigRow label={t("tradingOptions.endOfDayExitTime")} indent>
                <TimeInput value={to.endOfDayExitTime} onChange={(v) => update({ endOfDayExitTime: v })} />
              </ConfigRow>
            )}

            {/* Exit on friday */}
            <ConfigRow label={t("tradingOptions.exitOnFriday")}>
              <Toggle
                checked={to.exitOnFriday}
                onChange={(v) => update({ exitOnFriday: v })}
              />
            </ConfigRow>
            {to.exitOnFriday && (
              <ConfigRow label={t("tradingOptions.fridayExitTime")} indent>
                <TimeInput value={to.fridayExitTime} onChange={(v) => update({ fridayExitTime: v })} />
              </ConfigRow>
            )}

            {/* Limit time range */}
            <ConfigRow label={t("tradingOptions.limitTimeRange")}>
              <Toggle
                checked={to.limitTimeRange}
                onChange={(v) => update({ limitTimeRange: v })}
              />
            </ConfigRow>
            {to.limitTimeRange && (
              <>
                <ConfigRow label={t("tradingOptions.timeRangeFrom")} indent>
                  <TimeInput value={to.timeRangeFrom} onChange={(v) => update({ timeRangeFrom: v })} />
                </ConfigRow>
                <ConfigRow label={t("tradingOptions.timeRangeTo")} indent>
                  <TimeInput value={to.timeRangeTo} onChange={(v) => update({ timeRangeTo: v })} />
                </ConfigRow>
                <ConfigRow label={t("tradingOptions.exitAtEndOfRange")} indent>
                  <Toggle
                    checked={to.exitAtEndOfRange}
                    onChange={(v) => update({ exitAtEndOfRange: v })}
                  />
                </ConfigRow>
                <ConfigRow label={t("tradingOptions.orderTypesToClose")} indent>
                  <select
                    value={to.orderTypesToClose}
                    onChange={(e) => update({ orderTypesToClose: e.target.value as BuilderOrderTypesToClose })}
                    className="rounded border border-border bg-muted/30 px-2 py-1.5 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-primary h-8"
                  >
                    {(["all", "market", "stop", "limit"] as BuilderOrderTypesToClose[]).map((opt) => (
                      <option key={opt} value={opt}>
                        {t(`tradingOptions.orderTypesOptions.${opt}`)}
                      </option>
                    ))}
                  </select>
                </ConfigRow>
              </>
            )}
          </div>
        </SectionBox>
      </div>

      {/* Right column */}
      <div className="space-y-4">
        <SectionBox title={t("tradingOptions.buildOptions")}>
          <div className="space-y-2">
            {/* Max distance from market */}
            <ConfigRow label={t("tradingOptions.maxDistanceFromMarket")}>
              <Toggle
                checked={to.maxDistanceFromMarket}
                onChange={(v) => update({ maxDistanceFromMarket: v })}
              />
            </ConfigRow>
            {to.maxDistanceFromMarket && (
              <ConfigRow label={t("tradingOptions.maxDistancePercent")} indent>
                <div className="flex items-center gap-1">
                  <SpinnerInput
                    value={to.maxDistancePercent}
                    onChange={(v) => update({ maxDistancePercent: v })}
                    min={0}
                    max={100}
                    step={0.5}
                  />
                  <span className="text-[10px] text-muted-foreground/60">%</span>
                </div>
              </ConfigRow>
            )}

            {/* Max trades per day */}
            <ConfigRow label={t("tradingOptions.maxTradesPerDay")}>
              <SpinnerInput
                value={to.maxTradesPerDay}
                onChange={(v) => update({ maxTradesPerDay: v })}
                min={0}
                max={100}
                step={1}
              />
            </ConfigRow>
          </div>
        </SectionBox>

        <SectionBox title="Stop Loss / Profit Target limits">
          <p className="mb-2 text-[10px] text-muted-foreground/60">
            0 = no limit. Values in pips.
          </p>
          <div className="space-y-2">
            <ConfigRow label={t("tradingOptions.minimumSL")}>
              <div className="flex items-center gap-1">
                <SpinnerInput
                  value={to.minimumSL}
                  onChange={(v) => update({ minimumSL: v })}
                  min={0}
                  max={9999}
                  step={1}
                />
                <span className="text-[10px] text-muted-foreground/60">pips</span>
              </div>
            </ConfigRow>
            <ConfigRow label={t("tradingOptions.maximumSL")}>
              <div className="flex items-center gap-1">
                <SpinnerInput
                  value={to.maximumSL}
                  onChange={(v) => update({ maximumSL: v })}
                  min={0}
                  max={9999}
                  step={1}
                />
                <span className="text-[10px] text-muted-foreground/60">pips</span>
              </div>
            </ConfigRow>
            <ConfigRow label={t("tradingOptions.minimumPT")}>
              <div className="flex items-center gap-1">
                <SpinnerInput
                  value={to.minimumPT}
                  onChange={(v) => update({ minimumPT: v })}
                  min={0}
                  max={9999}
                  step={1}
                />
                <span className="text-[10px] text-muted-foreground/60">pips</span>
              </div>
            </ConfigRow>
            <ConfigRow label={t("tradingOptions.maximumPT")}>
              <div className="flex items-center gap-1">
                <SpinnerInput
                  value={to.maximumPT}
                  onChange={(v) => update({ maximumPT: v })}
                  min={0}
                  max={9999}
                  step={1}
                />
                <span className="text-[10px] text-muted-foreground/60">pips</span>
              </div>
            </ConfigRow>
          </div>
        </SectionBox>
      </div>
    </div>
  );
}
