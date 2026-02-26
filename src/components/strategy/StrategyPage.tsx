import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "@/stores/useAppStore";
import { saveStrategy, loadStrategies } from "@/lib/tauri";
import type { Strategy } from "@/lib/types";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/Tabs";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/Dialog";
import { FilePlus, FolderOpen, Save } from "lucide-react";
import { RulesList } from "./RulesList";
import { ConfigPanel } from "./ConfigPanel";
import { StrategyList } from "./StrategyList";
import { SaveStrategyDialog } from "./SaveStrategyDialog";

export function StrategyPage() {
  const {
    currentStrategy,
    savedStrategies,
    updateStrategyName,
    setLongEntryRules,
    setShortEntryRules,
    setLongExitRules,
    setShortExitRules,
    setPositionSizing,
    setStopLoss,
    setTakeProfit,
    setTrailingStop,
    setTradingCosts,
    setTradeDirection,
    setTradingHours,
    setMaxDailyTrades,
    setCloseTradesAt,
    setCurrentStrategy,
    setSavedStrategies,
    resetStrategy,
  } = useAppStore();

  const { t } = useTranslation("strategy");
  const { t: tc } = useTranslation("common");

  const [showLoadDialog, setShowLoadDialog] = useState(false);
  const [showSaveDialog, setShowSaveDialog] = useState(false);

  // Load saved strategies on mount
  useEffect(() => {
    loadStrategies()
      .then(setSavedStrategies)
      .catch((err) => console.error("Failed to load strategies:", err));
  }, [setSavedStrategies]);

  // Ctrl+S shortcut listener
  useEffect(() => {
    const handler = () => setShowSaveDialog(true);
    document.addEventListener("shortcut:save-strategy", handler);
    return () => document.removeEventListener("shortcut:save-strategy", handler);
  }, []);

  const handleSave = async (name: string) => {
    const strategyToSave: Strategy = {
      id: currentStrategy.id ?? "",
      name,
      created_at: currentStrategy.created_at ?? "",
      updated_at: currentStrategy.updated_at ?? "",
      long_entry_rules: currentStrategy.long_entry_rules,
      short_entry_rules: currentStrategy.short_entry_rules,
      long_exit_rules: currentStrategy.long_exit_rules,
      short_exit_rules: currentStrategy.short_exit_rules,
      position_sizing: currentStrategy.position_sizing,
      stop_loss: currentStrategy.stop_loss,
      take_profit: currentStrategy.take_profit,
      trailing_stop: currentStrategy.trailing_stop,
      trading_costs: currentStrategy.trading_costs,
      trade_direction: currentStrategy.trade_direction,
      trading_hours: currentStrategy.trading_hours,
      max_daily_trades: currentStrategy.max_daily_trades,
      close_trades_at: currentStrategy.close_trades_at,
    };

    const id = await saveStrategy(strategyToSave);

    // Update current strategy with the returned id
    setCurrentStrategy({
      ...currentStrategy,
      id,
      name,
    });

    // Refresh saved strategies list
    const updated = await loadStrategies();
    setSavedStrategies(updated);
  };

  const handleLoad = (strategy: Strategy) => {
    setCurrentStrategy(strategy);
    setShowLoadDialog(false);
  };

  const handleDeleted = (id: string) => {
    setSavedStrategies(savedStrategies.filter((s) => s.id !== id));
    // If the deleted strategy is the current one, reset its id
    if (currentStrategy.id === id) {
      setCurrentStrategy({ ...currentStrategy, id: undefined });
    }
  };

  return (
    <div className="space-y-4">
      {/* Toolbar */}
      <div className="flex flex-wrap items-center gap-3">
        <Input
          className="h-9 max-w-xs font-medium"
          value={currentStrategy.name}
          onChange={(e) => updateStrategyName(e.target.value)}
          placeholder={t("strategyName")}
        />
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            className="text-sm"
            onClick={() => setShowLoadDialog(true)}
          >
            <FolderOpen className="mr-1.5 h-3.5 w-3.5" />
            {tc("buttons.load")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="text-sm"
            onClick={() => setShowSaveDialog(true)}
          >
            <Save className="mr-1.5 h-3.5 w-3.5" />
            {tc("buttons.save")}
          </Button>
          <Button variant="outline" size="sm" className="text-sm" onClick={resetStrategy}>
            <FilePlus className="mr-1.5 h-3.5 w-3.5" />
            {tc("buttons.new")}
          </Button>
        </div>
      </div>

      {/* Main layout: rules (left) + config (right) */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        {/* Rules panel - spans 2 cols on lg */}
        <div className="space-y-4 lg:col-span-2">
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-sm">{t("entryRules")}</CardTitle>
            </CardHeader>
            <CardContent>
              <Tabs defaultValue="long">
                <TabsList className="mb-3 w-full">
                  <TabsTrigger value="long" className="text-sm">{t("long")}</TabsTrigger>
                  <TabsTrigger value="short" className="text-sm">{t("short")}</TabsTrigger>
                </TabsList>
                <TabsContent value="long">
                  <RulesList
                    title=""
                    rules={currentStrategy.long_entry_rules}
                    onChange={setLongEntryRules}
                  />
                </TabsContent>
                <TabsContent value="short">
                  <RulesList
                    title=""
                    rules={currentStrategy.short_entry_rules}
                    onChange={setShortEntryRules}
                  />
                </TabsContent>
              </Tabs>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-sm">{t("exitRules")}</CardTitle>
            </CardHeader>
            <CardContent>
              <Tabs defaultValue="long">
                <TabsList className="mb-3 w-full">
                  <TabsTrigger value="long" className="text-sm">{t("long")}</TabsTrigger>
                  <TabsTrigger value="short" className="text-sm">{t("short")}</TabsTrigger>
                </TabsList>
                <TabsContent value="long">
                  <RulesList
                    title=""
                    rules={currentStrategy.long_exit_rules}
                    onChange={setLongExitRules}
                  />
                </TabsContent>
                <TabsContent value="short">
                  <RulesList
                    title=""
                    rules={currentStrategy.short_exit_rules}
                    onChange={setShortExitRules}
                  />
                </TabsContent>
              </Tabs>
            </CardContent>
          </Card>
        </div>

        {/* Configuration panel */}
        <div>
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-sm">{t("configuration")}</CardTitle>
            </CardHeader>
            <CardContent>
              <ConfigPanel
                positionSizing={currentStrategy.position_sizing}
                stopLoss={currentStrategy.stop_loss}
                takeProfit={currentStrategy.take_profit}
                trailingStop={currentStrategy.trailing_stop}
                tradingCosts={currentStrategy.trading_costs}
                tradeDirection={currentStrategy.trade_direction}
                tradingHours={currentStrategy.trading_hours}
                maxDailyTrades={currentStrategy.max_daily_trades}
                onPositionSizingChange={setPositionSizing}
                onStopLossChange={setStopLoss}
                onTakeProfitChange={setTakeProfit}
                onTrailingStopChange={setTrailingStop}
                onTradingCostsChange={setTradingCosts}
                onTradeDirectionChange={setTradeDirection}
                onTradingHoursChange={setTradingHours}
                onMaxDailyTradesChange={setMaxDailyTrades}
                closeTradesAt={currentStrategy.close_trades_at}
                onCloseTradesAtChange={setCloseTradesAt}
              />
            </CardContent>
          </Card>
        </div>
      </div>

      {/* Load strategies dialog */}
      <Dialog open={showLoadDialog} onOpenChange={setShowLoadDialog}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>{t("loadStrategy")}</DialogTitle>
            <DialogDescription>
              {t("selectSaved")}
            </DialogDescription>
          </DialogHeader>
          <StrategyList
            strategies={savedStrategies}
            onLoad={handleLoad}
            onDeleted={handleDeleted}
          />
        </DialogContent>
      </Dialog>

      {/* Save dialog */}
      <SaveStrategyDialog
        open={showSaveDialog}
        onOpenChange={setShowSaveDialog}
        currentName={currentStrategy.name}
        onSave={handleSave}
      />
    </div>
  );
}
