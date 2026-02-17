import { useEffect } from "react";
import { AppLayout } from "@/components/layout/AppLayout";
import { DataPage } from "@/components/data/DataPage";
import { StrategyPage } from "@/components/strategy/StrategyPage";
import { BacktestPage } from "@/components/backtest/BacktestPage";
import { OptimizationPage } from "@/components/optimization/OptimizationPage";
import { ExportPage } from "@/components/export/ExportPage";
import { TooltipProvider } from "@/components/ui/Tooltip";
import { useKeyboardShortcuts } from "@/hooks/useKeyboardShortcuts";
import { useAppStore } from "@/stores/useAppStore";

function App() {
  const activeSection = useAppStore((s) => s.activeSection);
  const darkMode = useAppStore((s) => s.darkMode);

  useKeyboardShortcuts();

  useEffect(() => {
    document.documentElement.classList.toggle("dark", darkMode);
  }, [darkMode]);

  const renderSection = () => {
    switch (activeSection) {
      case "data":
        return <DataPage />;
      case "strategy":
        return <StrategyPage />;
      case "backtest":
        return <BacktestPage />;
      case "optimization":
        return <OptimizationPage />;
      case "export":
        return <ExportPage />;
    }
  };

  return (
    <TooltipProvider delayDuration={300}>
      <AppLayout>{renderSection()}</AppLayout>
    </TooltipProvider>
  );
}

export default App;
