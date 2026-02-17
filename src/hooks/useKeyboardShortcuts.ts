import { useEffect } from "react";
import { useAppStore } from "@/stores/useAppStore";
import { cancelBacktest, cancelOptimization } from "@/lib/tauri";

/**
 * Global keyboard shortcuts.
 * - Ctrl+S: Save current strategy (on strategy page)
 * - Ctrl+Enter: Run backtest / optimization (on respective page)
 * - Escape: Cancel running operation
 */
export function useKeyboardShortcuts() {
  const activeSection = useAppStore((s) => s.activeSection);
  const isLoading = useAppStore((s) => s.isLoading);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const ctrl = e.ctrlKey || e.metaKey;

      // Ctrl+S → save strategy
      if (ctrl && e.key === "s") {
        e.preventDefault();
        if (activeSection === "strategy") {
          document.dispatchEvent(new CustomEvent("shortcut:save-strategy"));
        }
        return;
      }

      // Ctrl+Enter → run backtest or optimization
      if (ctrl && e.key === "Enter") {
        e.preventDefault();
        if (activeSection === "backtest") {
          document.dispatchEvent(new CustomEvent("shortcut:run-backtest"));
        } else if (activeSection === "optimization") {
          document.dispatchEvent(new CustomEvent("shortcut:run-optimization"));
        }
        return;
      }

      // Escape → cancel running operation
      if (e.key === "Escape" && isLoading) {
        e.preventDefault();
        if (activeSection === "backtest") {
          cancelBacktest().catch(() => {});
        } else if (activeSection === "optimization") {
          cancelOptimization().catch(() => {});
        }
      }
    };

    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [activeSection, isLoading]);
}
