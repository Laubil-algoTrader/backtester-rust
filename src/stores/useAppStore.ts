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
} from "@/lib/types";

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
  optimizationOosPeriods: OosPeriod[];
  setOptimizationResults: (results: OptimizationResult[]) => void;
  setOptimizationOosPeriods: (periods: OosPeriod[]) => void;

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

  // Theme
  darkMode: boolean;
  toggleDarkMode: () => void;
}

export const useAppStore = create<AppState>((set) => ({
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
  optimizationOosPeriods: [],
  setOptimizationResults: (results) => set({ optimizationResults: results }),
  setOptimizationOosPeriods: (periods) => set({ optimizationOosPeriods: periods }),

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

  // Theme
  darkMode: true,
  toggleDarkMode: () =>
    set((state) => ({ darkMode: !state.darkMode })),
}));
