import { Moon, Sun, LogOut, Lock, Globe } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/useAppStore";
import { clearLicense } from "@/lib/tauri";
import type { AppSection } from "@/lib/types";

interface NavItem {
  id: AppSection;
  labelKey: string;
  proOnly?: boolean;
}

const navItems: NavItem[] = [
  { id: "data", labelKey: "nav.data" },
  { id: "strategy", labelKey: "nav.strategy" },
  { id: "backtest", labelKey: "nav.backtest" },
  { id: "optimization", labelKey: "nav.optimize", proOnly: true },
  { id: "export", labelKey: "nav.export", proOnly: true },
];

export function TopNav() {
  const { t } = useTranslation("common");
  const activeSection = useAppStore((s) => s.activeSection);
  const setActiveSection = useAppStore((s) => s.setActiveSection);
  const darkMode = useAppStore((s) => s.darkMode);
  const toggleDarkMode = useAppStore((s) => s.toggleDarkMode);
  const language = useAppStore((s) => s.language);
  const setLanguage = useAppStore((s) => s.setLanguage);
  const selectedSymbolId = useAppStore((s) => s.selectedSymbolId);
  const symbols = useAppStore((s) => s.symbols);
  const licenseTier = useAppStore((s) => s.licenseTier);
  const licenseUsername = useAppStore((s) => s.licenseUsername);
  const setLicenseChecked = useAppStore((s) => s.setLicenseChecked);
  const setLicenseInfo = useAppStore((s) => s.setLicenseInfo);

  const selectedSymbol = symbols.find((s) => s.id === selectedSymbolId);
  const isFree = licenseTier === "free";

  const handleLogout = async () => {
    try {
      await clearLicense();
    } catch {
      // ignore
    }
    setLicenseInfo("free", null);
    setLicenseChecked(false);
  };

  return (
    <nav className="flex h-11 shrink-0 items-center justify-between border-b border-border/30 bg-background px-6">
      {/* Left — branding + symbol */}
      <div className="flex items-center gap-4">
        <span className="text-sm font-bold tracking-tight text-foreground">
          Backtester
        </span>
        {licenseTier === "pro" && (
          <span className="rounded bg-primary/15 px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wider text-primary">
            PRO
          </span>
        )}
        {selectedSymbol && (
          <>
            <span className="text-border">|</span>
            <span className="text-sm font-semibold text-primary">
              {selectedSymbol.name}
            </span>
          </>
        )}
      </div>

      {/* Center — navigation links */}
      <div className="flex items-center gap-1">
        {navItems.map((item) => {
          const isActive = activeSection === item.id;
          const isLocked = item.proOnly && isFree;
          return (
            <button
              key={item.id}
              onClick={() => setActiveSection(item.id)}
              className={cn(
                "relative flex items-center gap-1 px-3 py-1.5 text-sm font-medium transition-colors rounded-md",
                isActive
                  ? "text-foreground bg-foreground/[0.06]"
                  : isLocked
                    ? "text-muted-foreground/50 hover:text-muted-foreground hover:bg-foreground/[0.02]"
                    : "text-muted-foreground hover:text-foreground hover:bg-foreground/[0.03]"
              )}
            >
              {t(item.labelKey)}
              {isLocked && <Lock className="h-3 w-3" />}
            </button>
          );
        })}
      </div>

      {/* Right — user info + dark mode + logout */}
      <div className="flex items-center gap-2">
        {licenseUsername && (
          <span className="text-xs text-muted-foreground">
            {licenseUsername}
          </span>
        )}
        <button
          onClick={() => setLanguage(language === "en" ? "es" : "en")}
          title={t("language")}
          className="flex h-7 items-center justify-center gap-1 rounded-md px-1.5 text-muted-foreground transition-colors hover:bg-foreground/[0.06] hover:text-foreground"
        >
          <Globe className="h-3.5 w-3.5" />
          <span className="text-[10px] font-bold uppercase">{language}</span>
        </button>
        <button
          onClick={toggleDarkMode}
          className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/[0.06] hover:text-foreground"
        >
          {darkMode ? (
            <Sun className="h-3.5 w-3.5" />
          ) : (
            <Moon className="h-3.5 w-3.5" />
          )}
        </button>
        <button
          onClick={handleLogout}
          title="Logout"
          className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/[0.06] hover:text-foreground"
        >
          <LogOut className="h-3.5 w-3.5" />
        </button>
      </div>
    </nav>
  );
}
