import { useMemo } from "react";
import type { BacktestPrecision } from "@/lib/types";
import type { Symbol } from "@/lib/types";

/**
 * Returns the list of BacktestPrecision modes available for a given symbol,
 * based on which timeframe Parquet files were generated at import time.
 *
 * Shared between BacktestPanel and OptimizerPanel to avoid duplication.
 */
export function useAvailablePrecisions(
  selectedSymbol: Symbol | null | undefined
): BacktestPrecision[] {
  return useMemo<BacktestPrecision[]>(() => {
    if (!selectedSymbol) return ["SelectedTfOnly"];

    const base = selectedSymbol.base_timeframe;
    const hasTick = !!selectedSymbol.timeframe_paths["tick"];
    const hasM1 = !!selectedSymbol.timeframe_paths["m1"];
    const hasTickRaw = "tick_raw" in selectedSymbol.timeframe_paths;

    if (base === "tick") {
      const modes: BacktestPrecision[] = ["SelectedTfOnly"];
      if (hasM1) modes.push("M1TickSimulation");
      if (hasTick) modes.push("RealTickCustomSpread");
      if (hasTickRaw) modes.push("RealTickRealSpread");
      return modes;
    }
    if (base === "m1") {
      const modes: BacktestPrecision[] = ["SelectedTfOnly"];
      if (hasM1) modes.push("M1TickSimulation");
      return modes;
    }
    return ["SelectedTfOnly"];
  }, [selectedSymbol]);
}
