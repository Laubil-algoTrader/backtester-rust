import { create } from "zustand";
import type {
  AppSection,
  Symbol,
  Strategy,
  BacktestResults,
  OptimizationResult,
  Timeframe,
  Rule,
  PositionSizing,
  StopLoss,
  TakeProfit,
  TrailingStop,
  TradingCosts,
  TradeDirection,
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
  entry_rules: [],
  exit_rules: [],
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
  setEntryRules: (rules: Rule[]) => void;
  setExitRules: (rules: Rule[]) => void;
  setPositionSizing: (ps: PositionSizing) => void;
  setStopLoss: (sl: StopLoss | undefined) => void;
  setTakeProfit: (tp: TakeProfit | undefined) => void;
  setTrailingStop: (ts: TrailingStop | undefined) => void;
  setTradingCosts: (costs: TradingCosts) => void;
  setTradeDirection: (dir: TradeDirection) => void;
  resetStrategy: () => void;

  // Backtest
  selectedTimeframe: Timeframe;
  backtestStartDate: string;
  backtestEndDate: string;
  initialCapital: number;
  leverage: number;
  backtestResults: BacktestResults | null;
  setSelectedTimeframe: (tf: Timeframe) => void;
  setBacktestStartDate: (date: string) => void;
  setBacktestEndDate: (date: string) => void;
  setInitialCapital: (capital: number) => void;
  setLeverage: (leverage: number) => void;
  setBacktestResults: (results: BacktestResults | null) => void;

  // Optimization
  optimizationResults: OptimizationResult[];
  setOptimizationResults: (results: OptimizationResult[]) => void;

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
  setEntryRules: (rules) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, entry_rules: rules },
    })),
  setExitRules: (rules) =>
    set((state) => ({
      currentStrategy: { ...state.currentStrategy, exit_rules: rules },
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
  resetStrategy: () => set({ currentStrategy: { ...emptyStrategy } }),

  // Backtest
  selectedTimeframe: "h1",
  backtestStartDate: "",
  backtestEndDate: "",
  initialCapital: 10000,
  leverage: 1,
  backtestResults: null,
  setSelectedTimeframe: (tf) => set({ selectedTimeframe: tf }),
  setBacktestStartDate: (date) => set({ backtestStartDate: date }),
  setBacktestEndDate: (date) => set({ backtestEndDate: date }),
  setInitialCapital: (capital) => set({ initialCapital: capital }),
  setLeverage: (leverage) => set({ leverage }),
  setBacktestResults: (results) => set({ backtestResults: results }),

  // Optimization
  optimizationResults: [],
  setOptimizationResults: (results) => set({ optimizationResults: results }),

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
