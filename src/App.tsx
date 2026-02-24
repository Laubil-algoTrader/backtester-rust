import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { AppLayout } from "@/components/layout/AppLayout";
import { DataPage } from "@/components/data/DataPage";
import { StrategyPage } from "@/components/strategy/StrategyPage";
import { BacktestPage } from "@/components/backtest/BacktestPage";
import { OptimizationPage } from "@/components/optimization/OptimizationPage";
import { ExportPage } from "@/components/export/ExportPage";
import { LoginPage } from "@/components/auth/LoginPage";
import { TooltipProvider } from "@/components/ui/Tooltip";
import { useKeyboardShortcuts } from "@/hooks/useKeyboardShortcuts";
import { useAppStore } from "@/stores/useAppStore";
import { startLicenseMonitor } from "@/lib/tauri";
import type { LicenseTier } from "@/lib/types";

function App() {
  const activeSection = useAppStore((s) => s.activeSection);
  const darkMode = useAppStore((s) => s.darkMode);
  const isLicenseChecked = useAppStore((s) => s.isLicenseChecked);
  const setLicenseInfo = useAppStore((s) => s.setLicenseInfo);
  const licenseUsername = useAppStore((s) => s.licenseUsername);

  useKeyboardShortcuts();

  useEffect(() => {
    document.documentElement.classList.toggle("dark", darkMode);
  }, [darkMode]);

  // Start license monitor and listen for tier changes after login
  useEffect(() => {
    if (!isLicenseChecked) return;

    // Start the background monitor (re-validates every hour)
    startLicenseMonitor().catch(() => {});

    // Listen for tier changes from the backend
    const unlisten = listen<{ tier: string }>("license-tier-changed", (event) => {
      const newTier = event.payload.tier as LicenseTier;
      setLicenseInfo(newTier, licenseUsername);
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [isLicenseChecked, setLicenseInfo, licenseUsername]);

  if (!isLicenseChecked) {
    return (
      <TooltipProvider delayDuration={300}>
        <LoginPage />
      </TooltipProvider>
    );
  }

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
