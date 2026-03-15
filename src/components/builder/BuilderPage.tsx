import { useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/useAppStore";
import { ProgressTab } from "./ProgressTab";
import { WhatToBuildTab } from "./tabs/WhatToBuildTab";
import { GeneticOptionsTab } from "./tabs/GeneticOptionsTab";
import { DataTab } from "./tabs/DataTab";
import { TradingOptionsTab } from "./tabs/TradingOptionsTab";
import { BuildingBlocksTab } from "./tabs/BuildingBlocksTab";
import { MoneyManagementTab } from "./tabs/MoneyManagementTab";
import { CrossChecksTab } from "./tabs/CrossChecksTab";
import { RankingTab } from "./tabs/RankingTab";
import { DatabanksPanel } from "./DatabanksPanel";
import { StrategyDetailOverlay } from "./StrategyDetailOverlay";
import { ProjectBreadcrumb } from "./ProjectBreadcrumb";
import type { BuilderSavedStrategy } from "@/lib/types";

type TopTab = "progress" | "fullSettings" | "results";
type SettingsTab =
  | "whatToBuild"
  | "geneticOptions"
  | "data"
  | "tradingOptions"
  | "buildingBlocks"
  | "moneyManagement"
  | "crossChecks"
  | "ranking";

const SETTINGS_TABS: SettingsTab[] = [
  "whatToBuild",
  "geneticOptions",
  "data",
  "tradingOptions",
  "buildingBlocks",
  "moneyManagement",
  "crossChecks",
  "ranking",
];

export function BuilderPage() {
  const { t } = useTranslation("builder");
  const { builderTopTab, builderSettingsTab, setBuilderTopTab, setBuilderSettingsTab } =
    useAppStore();
  const activeProjectTaskId = useAppStore((s) => s.activeProjectTaskId);

  const [detailStrategy, setDetailStrategy] = useState<BuilderSavedStrategy | null>(null);

  const handleStrategyOpen = (strategy: BuilderSavedStrategy) => {
    setDetailStrategy(strategy);
    setBuilderTopTab("results" as TopTab);
  };

  const renderSettingsTab = () => {
    switch (builderSettingsTab) {
      case "whatToBuild":   return <WhatToBuildTab />;
      case "geneticOptions": return <GeneticOptionsTab />;
      case "data":          return <DataTab />;
      case "tradingOptions": return <TradingOptionsTab />;
      case "buildingBlocks": return <BuildingBlocksTab />;
      case "moneyManagement": return <MoneyManagementTab />;
      case "crossChecks":   return <CrossChecksTab />;
      case "ranking":       return <RankingTab />;
    }
  };

  return (
    <div className="flex h-full flex-col">
      {activeProjectTaskId && <ProjectBreadcrumb />}
      <div className="relative flex h-full flex-col overflow-hidden">
        {/* Header + top tabs */}
        <div className="flex shrink-0 items-center border-b border-border/30 bg-background px-4 py-2 gap-4">
          <span className="text-sm font-semibold text-foreground">{t("title")}</span>
          <div className="flex gap-1">
            {(["progress", "fullSettings", "results"] as TopTab[]).map((tab) => (
              <button
                key={tab}
                onClick={() => setBuilderTopTab(tab)}
                className={cn(
                  "rounded px-4 py-1.5 text-xs font-medium transition-colors",
                  builderTopTab === tab
                    ? "bg-primary/10 text-primary border border-primary/30"
                    : "text-muted-foreground hover:text-foreground border border-transparent hover:border-border/40"
                )}
              >
                {t(`tabs.${tab}`)}
              </button>
            ))}
          </div>
        </div>

        {/* Main content (flex-1, can shrink for databank panel) */}
        <div className="min-h-0 flex-1 overflow-hidden">
          {builderTopTab === "progress" && (
            <div className="h-full overflow-hidden">
              <ProgressTab />
            </div>
          )}

          {builderTopTab === "results" && (
            detailStrategy ? (
              <StrategyDetailOverlay
                saved={detailStrategy}
                onClose={() => setDetailStrategy(null)}
              />
            ) : (
              <div className="flex h-full items-center justify-center text-sm text-muted-foreground/40">
                Haz doble clic en una estrategia del databank para ver sus resultados
              </div>
            )
          )}

          {builderTopTab === "fullSettings" && (
            <div className="flex h-full flex-col">
              <div className="shrink-0 border-b border-border/20 px-4 py-1.5">
                <span className="text-[10px] uppercase tracking-widest text-muted-foreground/60">
                  {t("advancedSettings")}
                </span>
              </div>
              <div className="shrink-0 border-b border-border/30 bg-background/50 px-2">
                <div className="flex flex-wrap gap-0.5 py-1">
                  {SETTINGS_TABS.map((tab) => (
                    <button
                      key={tab}
                      onClick={() => setBuilderSettingsTab(tab)}
                      className={cn(
                        "rounded px-3 py-1.5 text-xs font-medium transition-colors whitespace-nowrap",
                        builderSettingsTab === tab
                          ? "bg-primary/10 text-primary border-b-2 border-primary"
                          : "text-muted-foreground hover:text-foreground hover:bg-muted/40"
                      )}
                    >
                      {t(`settingsTabs.${tab}`)}
                    </button>
                  ))}
                </div>
              </div>
              <div className="flex-1 overflow-auto">
                <div className="mx-auto max-w-5xl">
                  {renderSettingsTab()}
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Databanks panel — only visible on Progress tab */}
        {builderTopTab === "progress" && (
          <DatabanksPanel onStrategyOpen={handleStrategyOpen} />
        )}
      </div>
    </div>
  );
}
