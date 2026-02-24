import { invoke } from "@tauri-apps/api/core";
import type {
  Symbol,
  Strategy,
  InstrumentConfig,
  BacktestConfig,
  BacktestResults,
  OptimizationResult,
  OptimizationConfig,
  Timeframe,
  CodeGenerationResult,
  LicenseResponse,
  SavedCredentials,
} from "./types";

/// Upload a CSV file and create a new symbol.
export async function uploadCsv(
  filePath: string,
  symbolName: string,
  instrumentConfig: InstrumentConfig
): Promise<Symbol> {
  return invoke<Symbol>("upload_csv", {
    filePath,
    symbolName,
    instrumentConfig,
  });
}

/// Get all imported symbols.
export async function getSymbols(): Promise<Symbol[]> {
  return invoke<Symbol[]>("get_symbols");
}

/// Delete a symbol and its data.
export async function deleteSymbol(symbolId: string): Promise<void> {
  return invoke<void>("delete_symbol", { symbolId });
}

/// Preview first N rows of data for a symbol.
export async function previewData(
  symbolId: string,
  timeframe: Timeframe,
  limit: number
): Promise<Record<string, unknown>[]> {
  return invoke<Record<string, unknown>[]>("preview_data", {
    symbolId,
    timeframe,
    limit,
  });
}

/// Run a backtest with the given strategy and configuration.
export async function runBacktest(
  strategy: Strategy,
  config: BacktestConfig
): Promise<BacktestResults> {
  return invoke<BacktestResults>("run_backtest", { strategy, config });
}

/// Cancel a running backtest.
export async function cancelBacktest(): Promise<void> {
  return invoke<void>("cancel_backtest");
}

/// Run optimization.
export async function runOptimization(
  strategy: Strategy,
  optimizationConfig: OptimizationConfig
): Promise<OptimizationResult[]> {
  return invoke<OptimizationResult[]>("run_optimization", {
    strategy,
    optimizationConfig,
  });
}

/// Cancel a running optimization.
export async function cancelOptimization(): Promise<void> {
  return invoke<void>("cancel_optimization");
}

/// Save a strategy.
export async function saveStrategy(strategy: Strategy): Promise<string> {
  return invoke<string>("save_strategy", { strategy });
}

/// Load all saved strategies.
export async function loadStrategies(): Promise<Strategy[]> {
  return invoke<Strategy[]>("load_strategies");
}

/// Delete a strategy.
export async function deleteStrategy(strategyId: string): Promise<void> {
  return invoke<void>("delete_strategy", { strategyId });
}

/// Export trades to CSV.
export async function exportTradesCsv(
  trades: unknown[],
  filePath: string
): Promise<void> {
  return invoke<void>("export_trades_csv", { trades, filePath });
}

/// Export metrics report to CSV.
export async function exportMetricsCsv(
  metrics: unknown,
  filePath: string
): Promise<void> {
  return invoke<void>("export_metrics_csv", { metrics, filePath });
}

/// Export full backtest report as HTML.
export async function exportReportHtml(
  results: BacktestResults,
  filePath: string
): Promise<void> {
  return invoke<void>("export_report_html", { results, filePath });
}

/// Generate strategy code for MQL5 or PineScript.
export async function generateStrategyCode(
  language: "mql5" | "pinescript",
  strategy: Strategy
): Promise<CodeGenerationResult> {
  return invoke<CodeGenerationResult>("generate_strategy_code", { language, strategy });
}

/// Download historical tick data from Dukascopy servers.
export async function downloadDukascopy(
  symbolName: string,
  dukaSymbol: string,
  pointValue: number,
  startDate: string,
  endDate: string,
  baseTimeframe: "tick" | "m1",
  instrumentConfig: InstrumentConfig
): Promise<Symbol> {
  return invoke<Symbol>("download_dukascopy", {
    symbolName,
    dukaSymbol,
    pointValue,
    startDate,
    endDate,
    baseTimeframe,
    instrumentConfig,
  });
}

/// Cancel an ongoing download by symbol name.
export async function cancelDownload(symbolName: string): Promise<void> {
  return invoke<void>("cancel_download", { symbolName });
}

/// Placeholder greet command (for testing communication).
export async function greet(name: string): Promise<string> {
  return invoke<string>("greet", { name });
}

// ── License ──

/// Validate a license key.
export async function validateLicense(
  username: string,
  licenseKey: string,
  remember: boolean
): Promise<LicenseResponse> {
  return invoke<LicenseResponse>("validate_license", {
    username,
    licenseKey,
    remember,
  });
}

/// Load saved credentials from disk.
export async function loadSavedLicense(): Promise<SavedCredentials | null> {
  return invoke<SavedCredentials | null>("load_saved_license");
}

/// Clear saved license and reset to Free tier.
export async function clearLicense(): Promise<void> {
  return invoke<void>("clear_license");
}

/// Start background license monitor (re-validates every hour).
export async function startLicenseMonitor(): Promise<void> {
  return invoke<void>("start_license_monitor");
}
