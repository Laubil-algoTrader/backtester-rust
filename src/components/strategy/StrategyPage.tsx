import { useEffect, useState } from "react";
import { useAppStore } from "@/stores/useAppStore";
import { saveStrategy, loadStrategies } from "@/lib/tauri";
import type { Strategy } from "@/lib/types";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
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
    setEntryRules,
    setExitRules,
    setPositionSizing,
    setStopLoss,
    setTakeProfit,
    setTrailingStop,
    setTradingCosts,
    setTradeDirection,
    setCurrentStrategy,
    setSavedStrategies,
    resetStrategy,
  } = useAppStore();

  const [showLoadDialog, setShowLoadDialog] = useState(false);
  const [showSaveDialog, setShowSaveDialog] = useState(false);

  // Load saved strategies on mount
  useEffect(() => {
    loadStrategies()
      .then(setSavedStrategies)
      .catch((err) => console.error("Failed to load strategies:", err));
  }, [setSavedStrategies]);

  const handleSave = async (name: string) => {
    const strategyToSave: Strategy = {
      id: currentStrategy.id ?? "",
      name,
      created_at: currentStrategy.created_at ?? "",
      updated_at: currentStrategy.updated_at ?? "",
      entry_rules: currentStrategy.entry_rules,
      exit_rules: currentStrategy.exit_rules,
      position_sizing: currentStrategy.position_sizing,
      stop_loss: currentStrategy.stop_loss,
      take_profit: currentStrategy.take_profit,
      trailing_stop: currentStrategy.trailing_stop,
      trading_costs: currentStrategy.trading_costs,
      trade_direction: currentStrategy.trade_direction,
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
          placeholder="Strategy name"
        />
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => setShowLoadDialog(true)}
          >
            <FolderOpen className="mr-1.5 h-4 w-4" />
            Load
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setShowSaveDialog(true)}
          >
            <Save className="mr-1.5 h-4 w-4" />
            Save
          </Button>
          <Button variant="outline" size="sm" onClick={resetStrategy}>
            <FilePlus className="mr-1.5 h-4 w-4" />
            New
          </Button>
        </div>
      </div>

      {/* Main layout: rules (left) + config (right) */}
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        {/* Rules panel - spans 2 cols on lg */}
        <div className="space-y-4 lg:col-span-2">
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-base">Entry Rules</CardTitle>
            </CardHeader>
            <CardContent>
              <RulesList
                title=""
                rules={currentStrategy.entry_rules}
                onChange={setEntryRules}
              />
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-base">Exit Rules</CardTitle>
            </CardHeader>
            <CardContent>
              <RulesList
                title=""
                rules={currentStrategy.exit_rules}
                onChange={setExitRules}
              />
            </CardContent>
          </Card>
        </div>

        {/* Configuration panel */}
        <div>
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-base">Configuration</CardTitle>
            </CardHeader>
            <CardContent>
              <ConfigPanel
                positionSizing={currentStrategy.position_sizing}
                stopLoss={currentStrategy.stop_loss}
                takeProfit={currentStrategy.take_profit}
                trailingStop={currentStrategy.trailing_stop}
                tradingCosts={currentStrategy.trading_costs}
                tradeDirection={currentStrategy.trade_direction}
                onPositionSizingChange={setPositionSizing}
                onStopLossChange={setStopLoss}
                onTakeProfitChange={setTakeProfit}
                onTrailingStopChange={setTrailingStop}
                onTradingCostsChange={setTradingCosts}
                onTradeDirectionChange={setTradeDirection}
              />
            </CardContent>
          </Card>
        </div>
      </div>

      {/* Load strategies dialog */}
      <Dialog open={showLoadDialog} onOpenChange={setShowLoadDialog}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Load Strategy</DialogTitle>
            <DialogDescription>
              Select a saved strategy to load.
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
