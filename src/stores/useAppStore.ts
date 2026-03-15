import { create } from "zustand";
import type {
  AppSection,
  Symbol,
  Strategy,
  BacktestResults,
  OptimizationResult,
  Timeframe,
  BacktestPrecision,
  Rule,
  PositionSizing,
  StopLoss,
  TakeProfit,
  TrailingStop,
  TradingCosts,
  TradeDirection,
  TradingHours,
  CloseTradesAt,
  OosPeriod,
  ParameterRange,
  LicenseTier,
  MonteCarloResult,
  BuilderConfig,
  BuilderRuntimeStats,
  BuilderSavedStrategy,
  BuilderDatabank,
  BuilderIslandStats,
  IndicatorType,
  Project,
  ProjectTask,
} from "@/lib/types";
import {
  saveProject,
  loadProjectsFromDisk,
  deleteProjectFromDisk,
} from "@/lib/tauri";

// ── Default values ──

const defaultPositionSizing: PositionSizing = {
  sizing_type: "FixedLots",
  value: 1.0,
};

const defaultTradingCosts: TradingCosts = {
  spread_pips: 1.0,
  commission_type: "FixedPerLot",
  commission_value: 0,
  slippage_pips: 0,
  slippage_random: false,
};

const emptyStrategy: Omit<Strategy, "id" | "created_at" | "updated_at"> = {
  name: "New Strategy",
  long_entry_rules: [],
  short_entry_rules: [],
  long_exit_rules: [],
  short_exit_rules: [],
  position_sizing: defaultPositionSizing,
  trading_costs: defaultTradingCosts,
  trade_direction: "Both",
};

const ALL_INDICATORS: IndicatorType[] = [
  "ADX", "Aroon", "ATR", "AwesomeOscillator", "BarRange", "BearsPower", "BiggestRange",
  "BullsPower", "CCI", "DeMarker", "EMA", "Fibonacci", "Fractal",
  "BollingerBands", "GannHiLo", "HeikenAshi", "HighestInRange", "HullMA", "Ichimoku", "KeltnerChannel",
  "LaguerreRSI", "LinearRegression", "LowestInRange", "MACD", "Momentum", "ParabolicSAR",
  "Pivots", "Reflex", "ROC", "RSI", "SmallestRange", "SMA", "Stochastic", "StdDev",
  "SuperTrend", "TrueRange", "UlcerIndex", "VWAP", "Vortex", "WilliamsR",
] as IndicatorType[];

const defaultBuilderConfig: BuilderConfig = {
  whatToBuild: {
    direction: "long_only",
    buildMode: "genetic_evolution",
    minEntryRules: 1,
    maxEntryRules: 2,
    minExitRules: 0,
    maxExitRules: 1,
    maxLookback: 1,
    indicatorPeriodMin: 5,
    indicatorPeriodMax: 200,
    indicatorPeriodStep: 1,
    slRequired: true,
    slType: "atr",
    slCoeffMin: 1.5,
    slCoeffMax: 5.0,
    slCoeffStep: 0.5,
    slAtrPeriodMin: 10,
    slAtrPeriodMax: 20,
    slAtrPeriodStep: 1,
    tpRequired: true,
    tpType: "atr",
    tpCoeffMin: 2.0,
    tpCoeffMax: 5.0,
    tpCoeffStep: 0.5,
    tpAtrPeriodMin: 10,
    tpAtrPeriodMax: 20,
    tpAtrPeriodStep: 1,
  },
  geneticOptions: {
    maxGenerations: 50,
    populationPerIsland: 40,
    crossoverProbability: 85,
    mutationProbability: 30,
    islands: 5,
    migrateEveryN: 5,
    migrationRate: 5,
    initialPopulationSize: 200,
    useFromDatabank: false,
    decimationCoefficient: 2,
    initialFilters: [],
    freshBloodDetectDuplicates: true,
    freshBloodReplacePercent: 10,
    freshBloodReplaceEvery: 10,
    showLastGeneration: false,
    startAgainWhenFinished: true,
    restartOnStagnation: false,
    stagnationSample: "in_sample",
    stagnationGenerations: 50,
    prefilterWindowPct: 30,
    prefilterMinTrades: 5,
    phaseBasedAdaptation: true,
    fitnessSharingSigma: 0.3,
    fitnessSharingAlpha: 1.0,
    nichingMode: "structural",
    metaLearningRate: 0.05,
    metaLearningTopPct: 0.25,
  },
  dataConfig: {
    symbolId: null,
    timeframe: "h1",
    startDate: "",
    endDate: "",
    precision: "SelectedTfOnly",
    spreadPips: 1,
    slippagePips: 0,
    minDistancePips: 0,
    dataRangeParts: [{ id: "p1", type: "is", percent: 100 }],
  },
  tradingOptions: {
    dontTradeWeekends: true,
    fridayCloseTime: "16:00",
    sundayOpenTime: "23:59",
    exitAtEndOfDay: false,
    endOfDayExitTime: "15:30",
    exitOnFriday: true,
    fridayExitTime: "22:00",
    limitTimeRange: true,
    timeRangeFrom: "01:00",
    timeRangeTo: "22:59",
    exitAtEndOfRange: true,
    orderTypesToClose: "all",
    maxDistanceFromMarket: false,
    maxDistancePercent: 6,
    maxTradesPerDay: 1,
    minimumSL: 0,
    maximumSL: 0,
    minimumPT: 0,
    maximumPT: 0,
  },
  buildingBlocks: {
    indicators: ALL_INDICATORS.map((t) => {
      const defaultEnabled = ["SMA", "EMA", "RSI", "MACD", "BollingerBands", "ATR", "Stochastic", "ADX", "CCI", "ROC", "WilliamsR", "ParabolicSAR", "Momentum"].includes(t);
      return { indicatorType: t, enabled: defaultEnabled, weight: 1 };
    }),
    orderTypes: [
      { orderType: "stop", enabled: true, weight: 1 },
      { orderType: "limit", enabled: false, weight: 1 },
      { orderType: "market", enabled: false, weight: 1 },
    ],
    exitTypes: [
      { exitType: "profit_target", enabled: true, required: true },
      { exitType: "stop_loss", enabled: true, required: true },
      { exitType: "trailing_stop", enabled: true, required: false },
      { exitType: "exit_after_bars", enabled: false, required: false },
      { exitType: "move_sl_be", enabled: false, required: false },
      { exitType: "exit_rule", enabled: false, required: false },
    ],
    orderPriceIndicators: ALL_INDICATORS.map((t) => ({
      indicatorType: t,
      enabled: t === "ATR",
      weight: 1,
      multiplierMin: 0.5,
      multiplierMax: 2.0,
      multiplierStep: 0.25,
    })),
    orderPriceBaseStop: "high" as const,
    orderPriceBaseLimit: "low" as const,
  },
  moneyManagement: {
    initialCapital: 100000,
    method: "fixed_amount",
    riskedMoney: 500,
    sizeDecimals: 2,
    sizeIfNoMM: 0.01,
    maximumLots: 100,
  },
  ranking: {
    maxStrategiesToStore: 3000,
    stopWhen: "never",
    stopTotallyCount: 1000,
    stopAfterDays: 0,
    stopAfterHours: 0,
    stopAfterMinutes: 0,
    fitnessSource: "main_data",
    computeFrom: "net_profit",
    weightedCriteria: [],
    customFilters: [
      { id: "r1", leftValue: "total_trades", operator: ">=", rightValue: 10 },
      { id: "r2", leftValue: "net_profit", operator: ">", rightValue: 0 },
    ],
    dismissSimilar: true,
    complexityAlpha: 0.0,
  },
  crossChecks: {
    disableAll: true,
    whatIf: false,
    monteCarlo: false,
    higherPrecision: false,
    additionalMarkets: false,
    monteCarloRetest: false,
    sequentialOpt: false,
    walkForward: false,
    walkForwardMatrix: false,
  },
};

// ── Store interface ──

interface AppState {
  // Navigation
  activeSection: AppSection;
  setActiveSection: (section: AppSection) => void;

  // Symbols
  symbols: Symbol[];
  selectedSymbolId: string | null;
  setSymbols: (symbols: Symbol[]) => void;
  addSymbol: (symbol: Symbol) => void;
  removeSymbol: (id: string) => void;
  updateSymbol: (symbol: Symbol) => void;
  setSelectedSymbolId: (id: string | null) => void;

  // Strategy
  currentStrategy: Omit<Strategy, "id" | "created_at" | "updated_at"> & {
    id?: string;
    created_at?: string;
    updated_at?: string;
  };
  savedStrategies: Strategy[];
  setCurrentStrategy: (
    strategy: Omit<Strategy, "id" | "created_at" | "updated_at"> & {
      id?: string;
      created_at?: string;
      updated_at?: string;
    }
  ) => void;
  setSavedStrategies: (strategies: Strategy[]) => void;
  updateStrategyName: (name: string) => void;
  setLongEntryRules: (rules: Rule[]) => void;
  setShortEntryRules: (rules: Rule[]) => void;
  setLongExitRules: (rules: Rule[]) => void;
  setShortExitRules: (rules: Rule[]) => void;
  setPositionSizing: (ps: PositionSizing) => void;
  setStopLoss: (sl: StopLoss | undefined) => void;
  setTakeProfit: (tp: TakeProfit | undefined) => void;
  setTrailingStop: (ts: TrailingStop | undefined) => void;
  setTradingCosts: (costs: TradingCosts) => void;
  setTradeDirection: (dir: TradeDirection) => void;
  setTradingHours: (hours: TradingHours | undefined) => void;
  setMaxDailyTrades: (max: number | undefined) => void;
  setCloseTradesAt: (ct: CloseTradesAt | undefined) => void;
  resetStrategy: () => void;

  // Backtest
  selectedTimeframe: Timeframe;
  backtestPrecision: BacktestPrecision;
  backtestStartDate: string;
  backtestEndDate: string;
  initialCapital: number;
  leverage: number;
  backtestResults: BacktestResults | null;
  equityMarkers: { date: string; label: string }[];
  setSelectedTimeframe: (tf: Timeframe) => void;
  setBacktestPrecision: (p: BacktestPrecision) => void;
  setBacktestStartDate: (date: string) => void;
  setBacktestEndDate: (date: string) => void;
  setInitialCapital: (capital: number) => void;
  setLeverage: (leverage: number) => void;
  setBacktestResults: (results: BacktestResults | null) => void;
  setEquityMarkers: (markers: { date: string; label: string }[]) => void;

  // Optimization
  optimizationResults: OptimizationResult[];
  optimizationParamRanges: ParameterRange[];
  optimizationOosPeriods: OosPeriod[];
  setOptimizationResults: (results: OptimizationResult[]) => void;
  setOptimizationParamRanges: (ranges: ParameterRange[]) => void;
  setOptimizationOosPeriods: (periods: OosPeriod[]) => void;

  // Monte Carlo (Robustez)
  monteCarloResults: MonteCarloResult | null;
  setMonteCarloResults: (results: MonteCarloResult | null) => void;

  // Active Downloads
  activeDownloads: Record<string, { progress: number; message: string; startTime: number }>;
  addActiveDownload: (symbolName: string) => void;
  updateDownloadProgress: (symbolName: string, progress: number, message: string) => void;
  removeActiveDownload: (symbolName: string) => void;

  // Loading / Progress
  isLoading: boolean;
  loadingMessage: string;
  progressPercent: number;
  setLoading: (loading: boolean, message?: string) => void;
  setProgress: (percent: number) => void;

  // License
  licenseTier: LicenseTier;
  licenseUsername: string | null;
  isLicenseChecked: boolean;
  setLicenseInfo: (tier: LicenseTier, username: string | null) => void;
  setLicenseChecked: (checked: boolean) => void;

  // Language
  language: "en" | "es";
  setLanguage: (lang: "en" | "es") => void;

  // Theme
  themeMode: "light" | "dark" | "olympus";
  darkMode: boolean; // derived: themeMode !== "light"
  setThemeMode: (mode: "light" | "dark" | "olympus") => void;
  toggleDarkMode: () => void;

  // Builder
  builderTopTab: "progress" | "fullSettings" | "results";
  builderSettingsTab: "whatToBuild" | "geneticOptions" | "data" | "tradingOptions" | "buildingBlocks" | "moneyManagement" | "crossChecks" | "ranking";
  setBuilderTopTab: (tab: "progress" | "fullSettings" | "results") => void;
  setBuilderSettingsTab: (tab: "whatToBuild" | "geneticOptions" | "data" | "tradingOptions" | "buildingBlocks" | "moneyManagement" | "crossChecks" | "ranking") => void;
  builderConfig: BuilderConfig;
  setBuilderConfig: (config: BuilderConfig) => void;
  updateBuilderConfig: (partial: Partial<BuilderConfig>) => void;
  // Builder runtime
  builderRunning: boolean;
  builderPaused: boolean;
  setBuilderRunning: (v: boolean) => void;
  setBuilderPaused: (v: boolean) => void;
  builderLog: string[];
  addBuilderLog: (msg: string) => void;
  clearBuilderLog: () => void;
  builderStats: BuilderRuntimeStats;
  setBuilderStats: (stats: Partial<BuilderRuntimeStats>) => void;
  builderDatabanks: BuilderDatabank[];
  activeDatabankId: string;
  targetDatabankId: string;
  createBuilderDatabank: (name?: string) => string;
  renameBuilderDatabank: (id: string, name: string) => void;
  deleteBuilderDatabank: (id: string) => void;
  clearBuilderDatabank: (id: string) => void;
  clearAllBuilderDatabanks: () => void;
  setActiveDatabankId: (id: string) => void;
  setTargetDatabankId: (id: string) => void;
  addToBuilderDatabank: (strategy: BuilderSavedStrategy) => void;
  removeFromBuilderDatabank: (databankId: string, strategyIds: string[]) => void;
  builderIslandStats: BuilderIslandStats[];
  upsertBuilderIslandStats: (stats: BuilderIslandStats) => void;
  clearBuilderIslandStats: () => void;

  // Custom Projects
  projects: Project[];
  activeProjectId: string | null;
  activeProjectTaskId: string | null;
  activeProjectTaskDirty: boolean;
  loadProjects: () => Promise<void>;
  createProject: (name: string) => Promise<void>;
  renameProject: (id: string, name: string) => Promise<void>;
  deleteProject: (id: string) => Promise<void>;
  importProject: (project: Project) => Promise<void>;
  addTaskToProject: (projectId: string, type: "builder") => Promise<void>;
  deleteTaskFromProject: (projectId: string, taskId: string) => Promise<void>;
  openProjectTask: (projectId: string, taskId: string) => Promise<void>;
  saveActiveProjectTask: () => Promise<void>;
  closeProjectTask: () => Promise<void>;
}

export const useAppStore = create<AppState>((set, get) => ({
  // Navigation
  activeSection: "data",
  setActiveSection: (section) => set({ activeSection: section }),

  // Symbols
  symbols: [],
  selectedSymbolId: null,
  setSymbols: (symbols) => set({ symbols }),
  addSymbol: (symbol) =>
    set((state) => ({ symbols: [...state.symbols, symbol] })),
  removeSymbol: (id) =>
    set((state) => ({
      symbols: state.symbols.filter((s) => s.id !== id),
      selectedSymbolId:
        state.selectedSymbolId === id ? null : state.selectedSymbolId,
    })),
  updateSymbol: (symbol) =>
    set((state) => ({
      symbols: state.symbols.map((s) => (s.id === symbol.id ? symbol : s)),
    })),
  setSelectedSymbolId: (id) => set({ selectedSymbolId: id }),

  // Strategy
  currentStrategy: { ...emptyStrategy },
  savedStrategies: [],
  setCurrentStrategy: (strategy) => set({ currentStrategy: strategy }),
  setSavedStrategies: (strategies) => set({ savedStrategies: strategies }),
  updateStrategyName: (name) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, name },
    })),
  setLongEntryRules: (rules) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, long_entry_rules: rules },
    })),
  setShortEntryRules: (rules) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, short_entry_rules: rules },
    })),
  setLongExitRules: (rules) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, long_exit_rules: rules },
    })),
  setShortExitRules: (rules) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, short_exit_rules: rules },
    })),
  setPositionSizing: (ps) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, position_sizing: ps },
    })),
  setStopLoss: (sl) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, stop_loss: sl },
    })),
  setTakeProfit: (tp) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, take_profit: tp },
    })),
  setTrailingStop: (ts) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, trailing_stop: ts },
    })),
  setTradingCosts: (costs) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, trading_costs: costs },
    })),
  setTradeDirection: (dir) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, trade_direction: dir },
    })),
  setTradingHours: (hours) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, trading_hours: hours },
    })),
  setMaxDailyTrades: (max) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, max_daily_trades: max },
    })),
  setCloseTradesAt: (ct) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, close_trades_at: ct },
    })),
  resetStrategy: () => set({ currentStrategy: { ...emptyStrategy } }),

  // Backtest
  selectedTimeframe: "h1",
  backtestPrecision: "SelectedTfOnly",
  backtestStartDate: "",
  backtestEndDate: "",
  initialCapital: 10000,
  leverage: 1,
  backtestResults: null,
  equityMarkers: [],
  setSelectedTimeframe: (tf) => set({ selectedTimeframe: tf }),
  setBacktestPrecision: (p) => set({ backtestPrecision: p }),
  setBacktestStartDate: (date) => set({ backtestStartDate: date }),
  setBacktestEndDate: (date) => set({ backtestEndDate: date }),
  setInitialCapital: (capital) => set({ initialCapital: capital }),
  setLeverage: (leverage) => set({ leverage }),
  setBacktestResults: (results) => set({ backtestResults: results }),
  setEquityMarkers: (markers) => set({ equityMarkers: markers }),

  // Optimization
  optimizationResults: [],
  optimizationParamRanges: [],
  optimizationOosPeriods: [],
  setOptimizationResults: (results) => set({ optimizationResults: results }),
  setOptimizationParamRanges: (ranges) => set({ optimizationParamRanges: ranges }),
  setOptimizationOosPeriods: (periods) => set({ optimizationOosPeriods: periods }),

  // Monte Carlo (Robustez)
  monteCarloResults: null,
  setMonteCarloResults: (results) => set({ monteCarloResults: results }),

  // Active Downloads
  activeDownloads: {},
  addActiveDownload: (symbolName) =>
    set((state) => ({
      activeDownloads: {
        ...state.activeDownloads,
        [symbolName]: { progress: 0, message: "Starting download...", startTime: Date.now() },
      },
    })),
  updateDownloadProgress: (symbolName, progress, message) =>
    set((state) => {
      const existing = state.activeDownloads[symbolName];
      return {
        activeDownloads: {
          ...state.activeDownloads,
          [symbolName]: { progress, message, startTime: existing?.startTime ?? Date.now() },
        },
      };
    }),
  removeActiveDownload: (symbolName) =>
    set((state) => {
      const { [symbolName]: _, ...rest } = state.activeDownloads;
      return { activeDownloads: rest };
    }),

  // Loading / Progress
  isLoading: false,
  loadingMessage: "",
  progressPercent: 0,
  setLoading: (loading, message = "") =>
    set({ isLoading: loading, loadingMessage: message, progressPercent: 0 }),
  setProgress: (percent) => set({ progressPercent: percent }),

  // License
  licenseTier: "free",
  licenseUsername: null,
  isLicenseChecked: false,
  setLicenseInfo: (tier, username) => set({ licenseTier: tier, licenseUsername: username }),
  setLicenseChecked: (checked) => set({ isLicenseChecked: checked }),

  // Language
  language: (localStorage.getItem("lbquant-lang") as "en" | "es") || "en",
  setLanguage: (lang) => {
    import("i18next").then((i18n) => i18n.default.changeLanguage(lang));
    set({ language: lang });
  },

  // Theme
  themeMode: (localStorage.getItem("lbquant-theme") as "light" | "dark" | "olympus") || "dark",
  darkMode: (localStorage.getItem("lbquant-theme") || "dark") !== "light",
  setThemeMode: (mode) => {
    localStorage.setItem("lbquant-theme", mode);
    set({ themeMode: mode, darkMode: mode !== "light" });
  },
  toggleDarkMode: () =>
    set((state) => {
      const next = state.themeMode === "light" ? "dark" : "light";
      localStorage.setItem("lbquant-theme", next);
      return { themeMode: next, darkMode: next !== "light" };
    }),

  // Builder
  builderTopTab: "fullSettings",
  builderSettingsTab: "whatToBuild",
  setBuilderTopTab: (tab) => set({ builderTopTab: tab }),
  setBuilderSettingsTab: (tab) => set({ builderSettingsTab: tab }),
  builderConfig: defaultBuilderConfig,
  setBuilderConfig: (config) => set((s) => ({
    builderConfig: config,
    activeProjectTaskDirty: s.activeProjectTaskId ? true : s.activeProjectTaskDirty,
  })),
  updateBuilderConfig: (partial) =>
    set((s) => ({
      builderConfig: { ...s.builderConfig, ...partial },
      activeProjectTaskDirty: s.activeProjectTaskId ? true : s.activeProjectTaskDirty,
    })),

  // Builder runtime
  builderRunning: false,
  builderPaused: false,
  setBuilderRunning: (v) => set({ builderRunning: v }),
  setBuilderPaused: (v) => set({ builderPaused: v }),
  builderLog: [],
  addBuilderLog: (msg) =>
    set((state) => ({ builderLog: [...state.builderLog, msg] })),
  clearBuilderLog: () => set({ builderLog: [] }),
  builderStats: {
    generated: 0,
    accepted: 0,
    rejected: 0,
    inDatabank: 0,
    startTime: null,
    strategiesPerHour: 0,
    acceptedPerHour: 0,
    timePerStrategyMs: 0,
    generation: 0,
    island: 0,
    bestFitness: 0,
  },
  setBuilderStats: (partial) =>
    set((state) => ({ builderStats: { ...state.builderStats, ...partial } })),
  builderDatabanks: [{ id: "results", name: "Results", strategies: [] }],
  activeDatabankId: "results",
  targetDatabankId: "results",
  createBuilderDatabank: (name) => {
    const id = `db_${Date.now()}`;
    const bankName = name ?? `Databank ${id.slice(-4)}`;
    set((s) => ({
      builderDatabanks: [...s.builderDatabanks, { id, name: bankName, strategies: [] }],
      activeProjectTaskDirty: s.activeProjectTaskId ? true : s.activeProjectTaskDirty,
    }));
    return id;
  },
  renameBuilderDatabank: (id, name) =>
    set((s) => ({
      builderDatabanks: s.builderDatabanks.map((db) =>
        db.id === id ? { ...db, name } : db
      ),
      activeProjectTaskDirty: s.activeProjectTaskId ? true : s.activeProjectTaskDirty,
    })),
  deleteBuilderDatabank: (id) =>
    set((s) => {
      const remaining = s.builderDatabanks.filter((db) => db.id !== id);
      const fallback = remaining[0]?.id ?? "results";
      return {
        builderDatabanks: remaining,
        activeDatabankId: s.activeDatabankId === id ? fallback : s.activeDatabankId,
        targetDatabankId: s.targetDatabankId === id ? fallback : s.targetDatabankId,
        activeProjectTaskDirty: s.activeProjectTaskId ? true : s.activeProjectTaskDirty,
      };
    }),
  clearBuilderDatabank: (id) =>
    set((s) => ({
      builderDatabanks: s.builderDatabanks.map((db) =>
        db.id === id ? { ...db, strategies: [] } : db
      ),
      activeProjectTaskDirty: s.activeProjectTaskId ? true : s.activeProjectTaskDirty,
    })),
  clearAllBuilderDatabanks: () =>
    set((s) => ({
      builderDatabanks: s.builderDatabanks.map((db) => ({ ...db, strategies: [] })),
      activeProjectTaskDirty: s.activeProjectTaskId ? true : s.activeProjectTaskDirty,
    })),
  setActiveDatabankId: (id) => set({ activeDatabankId: id }),
  setTargetDatabankId: (id) => set({ targetDatabankId: id }),
  addToBuilderDatabank: (strategy) =>
    set((s) => ({
      builderDatabanks: s.builderDatabanks.map((db) =>
        db.id === s.targetDatabankId
          ? { ...db, strategies: [...db.strategies, strategy] }
          : db
      ),
      activeProjectTaskDirty: s.activeProjectTaskId ? true : s.activeProjectTaskDirty,
    })),
  removeFromBuilderDatabank: (databankId, strategyIds) =>
    set((s) => ({
      builderDatabanks: s.builderDatabanks.map((db) =>
        db.id === databankId
          ? { ...db, strategies: db.strategies.filter((strat) => !strategyIds.includes(strat.id)) }
          : db
      ),
      activeProjectTaskDirty: s.activeProjectTaskId ? true : s.activeProjectTaskDirty,
    })),
  builderIslandStats: [],
  upsertBuilderIslandStats: (stats) =>
    set((state) => {
      const existing = state.builderIslandStats.findIndex(
        (s) => s.islandId === stats.islandId
      );
      if (existing >= 0) {
        const updated = [...state.builderIslandStats];
        updated[existing] = stats;
        return { builderIslandStats: updated };
      }
      return { builderIslandStats: [...state.builderIslandStats, stats] };
    }),
  clearBuilderIslandStats: () => set({ builderIslandStats: [] }),

  // Custom Projects
  projects: [],
  activeProjectId: null,
  activeProjectTaskId: null,
  activeProjectTaskDirty: false,
  loadProjects: async () => {
    const list = await loadProjectsFromDisk();
    set({ projects: list });
  },
  createProject: async (name) => {
    const id = crypto.randomUUID();
    const now = new Date().toISOString();
    const project: Project = { id, name, tasks: [], createdAt: now, updatedAt: now };
    await saveProject(project);
    set((s) => ({ projects: [...s.projects, project] }));
  },
  renameProject: async (id, name) => {
    const now = new Date().toISOString();
    let target: Project | undefined;
    const updated = get().projects.map((p) => {
      if (p.id === id) { target = { ...p, name, updatedAt: now }; return target; }
      return p;
    });
    set({ projects: updated });
    if (target) await saveProject(target);
  },
  deleteProject: async (id) => {
    await deleteProjectFromDisk(id);
    set((s) => {
      const taskBelongsToProject =
        s.projects.find((p) => p.id === id)?.tasks.some((t) => t.id === s.activeProjectTaskId) ?? false;
      return {
        projects: s.projects.filter((p) => p.id !== id),
        activeProjectId: s.activeProjectId === id ? null : s.activeProjectId,
        ...(taskBelongsToProject && {
          activeProjectTaskId: null,
          activeProjectTaskDirty: false,
          builderConfig: defaultBuilderConfig,
          builderDatabanks: [{ id: "results", name: "Results", strategies: [] }],
        }),
      };
    });
  },
  importProject: async (project) => {
    await saveProject(project);
    set((s) => {
      const exists = s.projects.some((p) => p.id === project.id);
      return {
        projects: exists
          ? s.projects.map((p) => (p.id === project.id ? project : p))
          : [...s.projects, project],
      };
    });
  },
  addTaskToProject: async (projectId, type) => {
    const s = get();
    const parent = s.projects.find((p) => p.id === projectId);
    if (!parent) return;
    const builderCount = parent.tasks.filter((t) => t.type === "builder").length;
    const name = builderCount === 0 ? "Builder" : `Builder(${builderCount + 1})`;
    const id = crypto.randomUUID();
    const now = new Date().toISOString();
    const task: ProjectTask = {
      id,
      name,
      type,
      config: s.builderConfig,
      databanks: [{ id: "results", name: "Results", strategies: [] }],
      status: "idle",
      strategiesCount: 0,
      databankCount: 1,
      createdAt: now,
    };
    let updated: Project | undefined;
    const projects = s.projects.map((p) => {
      if (p.id === projectId) {
        updated = { ...p, tasks: [...p.tasks, task], updatedAt: now };
        return updated;
      }
      return p;
    });
    set({ projects });
    if (updated) await saveProject(updated);
  },
  deleteTaskFromProject: async (projectId, taskId) => {
    const s = get();
    const reset: Record<string, unknown> = {};
    if (taskId === s.activeProjectTaskId) {
      reset.activeProjectTaskId = null;
      reset.builderConfig = defaultBuilderConfig;
      reset.builderDatabanks = [{ id: "results", name: "Results", strategies: [] }];
      reset.activeProjectTaskDirty = false;
      reset.activeSection = "projects";
    }
    const now = new Date().toISOString();
    let updated: Project | undefined;
    const projects = s.projects.map((p) => {
      if (p.id === projectId) {
        updated = { ...p, tasks: p.tasks.filter((t) => t.id !== taskId), updatedAt: now };
        return updated;
      }
      return p;
    });
    set({ projects, ...reset } as Partial<AppState>);
    if (updated) await saveProject(updated);
  },
  openProjectTask: async (projectId, taskId) => {
    const s = get();
    if (s.builderRunning || s.builderPaused) {
      throw new Error("Stop the builder before switching tasks");
    }
    if (s.activeProjectTaskDirty) {
      await get().saveActiveProjectTask();
    }
    const project = get().projects.find((p) => p.id === projectId);
    const task = project?.tasks.find((t) => t.id === taskId);
    if (!task) return;
    set({
      builderConfig: task.config as BuilderConfig,
      builderDatabanks: task.databanks as BuilderDatabank[],
      activeProjectId: projectId,
      activeProjectTaskId: taskId,
      activeProjectTaskDirty: false,
      activeSection: "builder",
    });
  },
  saveActiveProjectTask: async () => {
    const s = get();
    if (!s.activeProjectTaskId || !s.activeProjectId) return;
    const now = new Date().toISOString();
    const totalStrategies = s.builderDatabanks.reduce(
      (sum, db) => sum + db.strategies.length, 0
    );
    let target: Project | undefined;
    const projects = s.projects.map((p) => {
      if (p.id !== s.activeProjectId) return p;
      target = {
        ...p,
        updatedAt: now,
        tasks: p.tasks.map((t) =>
          t.id !== s.activeProjectTaskId
            ? t
            : {
                ...t,
                config: s.builderConfig,
                databanks: s.builderDatabanks,
                strategiesCount: totalStrategies,
                databankCount: s.builderDatabanks.length,
              }
        ),
      };
      return target;
    });
    set({ projects, activeProjectTaskDirty: false });
    if (target) await saveProject(target);
  },
  closeProjectTask: async () => {
    const s = get();
    if (s.activeProjectTaskDirty) {
      await get().saveActiveProjectTask();
    }
    set({
      activeProjectTaskId: null,
      builderConfig: defaultBuilderConfig,
      builderDatabanks: [{ id: "results", name: "Results", strategies: [] }],
      activeProjectTaskDirty: false,
      activeSection: "projects",
    });
  },
}));
