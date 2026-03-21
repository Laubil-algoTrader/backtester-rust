import { invoke } from "@tauri-apps/api/core";
import type {
  Symbol,
  Strategy,
  InstrumentConfig,
  BacktestConfig,
  BacktestResults,
  OptimizationResult,
  OptimizationConfig,
  MonteCarloConfig,
  MonteCarloResult,
  Timeframe,
  TickStorageFormat,
  TickPipeline,
  CodeGenerationResult,
  LicenseResponse,
  SavedCredentials,
  BuilderConfig,
  Project,
  SrConfig,
  SrStrategy,
} from "./types";

/// Upload a CSV file and create a new symbol.
export async function uploadCsv(
  filePath: string,
  symbolName: string,
  instrumentConfig: InstrumentConfig,
  tickStorageFormat?: TickStorageFormat
): Promise<Symbol> {
  return invoke<Symbol>("upload_csv", {
    filePath,
    symbolName,
    instrumentConfig,
    tickStorageFormat,
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

/// Run Monte Carlo simulation on a list of historical trades.
export async function runMonteCarlo(
  trades: BacktestResults["trades"],
  initialCapital: number,
  config: MonteCarloConfig
): Promise<MonteCarloResult> {
  return invoke<MonteCarloResult>("run_monte_carlo", {
    trades,
    initialCapital,
    config,
  });
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

/// Export raw tick data for a symbol to a CSV file in MetaTrader 5 import format.
/// Returns the number of rows written.
/// Only available for symbols with base_timeframe === "Tick".
export async function exportTickDataMt5(
  symbolId: string,
  filePath: string
): Promise<number> {
  return invoke<number>("export_tick_data_mt5", { symbolId, filePath });
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
  instrumentConfig: InstrumentConfig,
  tickStorageFormat?: TickStorageFormat,
  tickPipeline?: TickPipeline,
  keepCsv?: boolean,
  ignoreFlats?: boolean,
  retryOnEmpty?: boolean,
  useCache?: boolean
): Promise<Symbol> {
  return invoke<Symbol>("download_dukascopy", {
    symbolName,
    dukaSymbol,
    pointValue,
    startDate,
    endDate,
    baseTimeframe,
    instrumentConfig,
    tickStorageFormat,
    tickPipeline,
    keepCsv,
    ignoreFlats,
    retryOnEmpty,
    useCache,
  });
}

/// Cancel an ongoing download by symbol name.
export async function cancelDownload(symbolName: string): Promise<void> {
  return invoke<void>("cancel_download", { symbolName });
}

/// Transform all stored timestamps of a symbol to a new timezone offset.
/// Returns the updated Symbol with adjusted start_date, end_date and tz_offset_hours.
export async function transformSymbolTimezone(
  symbolId: string,
  newTzOffsetHours: number
): Promise<Symbol> {
  return invoke<Symbol>("transform_symbol_timezone", {
    symbolId,
    newTzOffsetHours,
  });
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

// ── Builder ──

/// Start the builder (GP strategy evolution).
export async function startBuilder(
  builderConfig: BuilderConfig,
  symbolId: string,
  timeframe: Timeframe,
  startDate: string,
  endDate: string,
  initialCapital: number,
): Promise<void> {
  return invoke<void>("start_builder", {
    builderConfig,
    symbolId,
    timeframe,
    startDate,
    endDate,
    initialCapital,
  });
}

/// Stop the builder.
export async function stopBuilder(): Promise<void> {
  return invoke<void>("stop_builder");
}

/// Pause or resume the builder.
export async function pauseBuilder(paused: boolean): Promise<void> {
  return invoke<void>("pause_builder", { paused });
}

// ── Projects ──

/// Save (create or update) a project to disk.
export async function saveProject(project: Project): Promise<void> {
  return invoke<void>("save_project", { project });
}

/// Load all projects from disk.
export async function loadProjectsFromDisk(): Promise<Project[]> {
  return invoke<Project[]>("load_projects");
}

/// Delete a project by id.
export async function deleteProjectFromDisk(id: string): Promise<void> {
  return invoke<void>("delete_project", { id });
}

/// Open a project from an arbitrary path via native file picker.
/// Returns null if the user cancelled the dialog.
export async function openProjectFromPath(): Promise<Project | null> {
  return invoke<Project | null>("open_project_from_path");
}

// ── SR Builder ──

/// Start the Symbolic Regression builder. Emits "sr-progress" events.
export async function runSrBuilder(config: SrConfig): Promise<void> {
  return invoke<void>("run_sr_builder", { config });
}

/// Cancel a running SR builder.
export async function cancelSrBuilder(): Promise<void> {
  return invoke<void>("cancel_sr_builder");
}

/// Run a full backtest for a single SR strategy. Returns BacktestResults.
export async function runSrBacktest(
  strategy: SrStrategy,
  symbolId: string,
  timeframe: import("./types").Timeframe,
  startDate: string,
  endDate: string,
  initialCapital: number
): Promise<import("./types").BacktestResults> {
  return invoke("run_sr_backtest", {
    strategy,
    symbolId,
    timeframe,
    startDate,
    endDate,
    initialCapital,
  });
}

/// Generate MQL5 code for an SR strategy.
export async function generateSrCode(
  strategy: SrStrategy,
  name: string
): Promise<import("./types").CodeGenerationResult> {
  return invoke("generate_sr_code", { strategy, name });
}
