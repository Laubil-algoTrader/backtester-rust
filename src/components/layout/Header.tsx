import { Moon, Sun, Landmark } from "lucide-react";
import { useAppStore } from "@/stores/useAppStore";
import { Button } from "@/components/ui/Button";

const sectionTitles: Record<string, string> = {
  data: "Data Management",
  strategy: "Strategy Builder",
  backtest: "Backtest",
  optimization: "Optimization",
};

export function Header() {
  const activeSection = useAppStore((s) => s.activeSection);
  const themeMode = useAppStore((s) => s.themeMode);
  const setThemeMode = useAppStore((s) => s.setThemeMode);
  const selectedSymbolId = useAppStore((s) => s.selectedSymbolId);
  const symbols = useAppStore((s) => s.symbols);

  const selectedSymbol = symbols.find((s) => s.id === selectedSymbolId);

  return (
    <header className="flex h-10 items-center justify-between border-b border-border/30 bg-background px-4">
      <div className="flex items-center gap-3">
        <h1 className="text-sm font-bold uppercase tracking-[0.2em] text-primary">
          Backtester
        </h1>
        <span className="text-muted-foreground/30">|</span>
        <span className="text-sm uppercase tracking-wider text-muted-foreground">
          {sectionTitles[activeSection]}
        </span>
        {selectedSymbol && (
          <>
            <span className="text-muted-foreground/30">|</span>
            <span className="rounded border border-primary/20 bg-primary/5 px-2 py-0.5 text-sm font-semibold text-primary">
              {selectedSymbol.name}
            </span>
          </>
        )}
      </div>
      <Button
        variant="ghost"
        size="icon"
        onClick={() => {
          const cycle = { light: "dark", dark: "olympus", olympus: "light" } as const;
          setThemeMode(cycle[themeMode]);
        }}
        title={themeMode}
        className="h-7 w-7"
      >
        {themeMode === "light" ? (
          <Sun className="h-3.5 w-3.5" />
        ) : themeMode === "dark" ? (
          <Moon className="h-3.5 w-3.5" />
        ) : (
          <Landmark className="h-3.5 w-3.5" />
        )}
      </Button>
    </header>
  );
}
