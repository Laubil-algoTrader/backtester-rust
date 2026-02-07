import { Moon, Sun } from "lucide-react";
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
  const darkMode = useAppStore((s) => s.darkMode);
  const toggleDarkMode = useAppStore((s) => s.toggleDarkMode);
  const selectedSymbolId = useAppStore((s) => s.selectedSymbolId);
  const symbols = useAppStore((s) => s.symbols);

  const selectedSymbol = symbols.find((s) => s.id === selectedSymbolId);

  return (
    <header className="flex h-12 items-center justify-between border-b bg-card px-4">
      <div className="flex items-center gap-3">
        <h1 className="text-sm font-bold tracking-tight">Backtester</h1>
        <span className="text-muted-foreground">|</span>
        <span className="text-sm text-muted-foreground">
          {sectionTitles[activeSection]}
        </span>
        {selectedSymbol && (
          <>
            <span className="text-muted-foreground">|</span>
            <span className="text-sm font-medium text-primary">
              {selectedSymbol.name}
            </span>
          </>
        )}
      </div>
      <Button
        variant="ghost"
        size="icon"
        onClick={toggleDarkMode}
        className="h-8 w-8"
      >
        {darkMode ? (
          <Sun className="h-4 w-4" />
        ) : (
          <Moon className="h-4 w-4" />
        )}
      </Button>
    </header>
  );
}
