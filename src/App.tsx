import { useEffect, Component } from "react";
import type { ErrorInfo, ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { Toaster } from "sonner";
import { AppLayout } from "@/components/layout/AppLayout";
import { DataPage } from "@/components/data/DataPage";
import { StrategyPage } from "@/components/strategy/StrategyPage";
import { BacktestPage } from "@/components/backtest/BacktestPage";
import { OptimizationPage } from "@/components/optimization/OptimizationPage";
import { RobustezPage } from "@/components/robustez/RobustezPage";
import { ExportPage } from "@/components/export/ExportPage";
import { BuilderPage } from "@/components/builder/BuilderPage";
import { ProjectsPage } from "@/components/projects/ProjectsPage";
import { ProjectView } from "@/components/projects/ProjectView";
import { LoginPage } from "@/components/auth/LoginPage";
import { TooltipProvider } from "@/components/ui/Tooltip";
import { useKeyboardShortcuts } from "@/hooks/useKeyboardShortcuts";
import { useAppStore } from "@/stores/useAppStore";
import { startLicenseMonitor } from "@/lib/tauri";
import type { LicenseTier } from "@/lib/types";

// ── Error Boundary ──────────────────────────────────────────────────────────
// Catches unhandled render errors and shows a recovery UI instead of a black
// screen. React only supports class-based error boundaries.
interface EBState { hasError: boolean; message: string }
class ErrorBoundary extends Component<{ children: ReactNode }, EBState> {
  state: EBState = { hasError: false, message: "" };

  static getDerivedStateFromError(err: unknown): EBState {
    const message = err instanceof Error ? err.message : String(err);
    return { hasError: true, message };
  }

  componentDidCatch(err: Error, info: ErrorInfo) {
    console.error("[ErrorBoundary]", err, info.componentStack);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex min-h-screen flex-col items-center justify-center gap-4 bg-background p-8 text-center">
          <p className="text-lg font-semibold text-destructive">Something went wrong</p>
          <p className="max-w-lg break-all font-mono text-sm text-muted-foreground">
            {this.state.message}
          </p>
          <button
            className="rounded bg-primary px-4 py-2 text-sm text-primary-foreground hover:opacity-90"
            onClick={() => this.setState({ hasError: false, message: "" })}
          >
            Try again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

function App() {
  const activeSection = useAppStore((s) => s.activeSection);
  const themeMode = useAppStore((s) => s.themeMode);
  const isLicenseChecked = useAppStore((s) => s.isLicenseChecked);
  const setLicenseInfo = useAppStore((s) => s.setLicenseInfo);
  const licenseUsername = useAppStore((s) => s.licenseUsername);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const loadProjects = useAppStore((s) => s.loadProjects);

  useKeyboardShortcuts();

  useEffect(() => {
    const el = document.documentElement;
    el.classList.remove("dark", "olympus");
    if (themeMode === "dark") el.classList.add("dark");
    else if (themeMode === "olympus") el.classList.add("olympus");
  }, [themeMode]);

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

  useEffect(() => {
    if (!isLicenseChecked) return;
    loadProjects().catch(console.error);
  }, [isLicenseChecked, loadProjects]);

  if (!isLicenseChecked) {
    return (
      <ErrorBoundary>
        <TooltipProvider delayDuration={300}>
          <LoginPage />
          <Toaster theme={themeMode === "light" ? "light" : "dark"} position="bottom-right" richColors />
        </TooltipProvider>
      </ErrorBoundary>
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
      case "robustez":
        return <RobustezPage />;
      case "export":
        return <ExportPage />;
      case "builder":
        return <BuilderPage />;
      case "projects":
        return activeProjectId ? <ProjectView /> : <ProjectsPage />;
    }
  };

  return (
    <ErrorBoundary>
      <TooltipProvider delayDuration={300}>
        <AppLayout>{renderSection()}</AppLayout>
        <Toaster theme={themeMode === "light" ? "light" : "dark"} position="bottom-right" richColors />
      </TooltipProvider>
    </ErrorBoundary>
  );
}

export default App;
