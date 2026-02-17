import {
  Code2,
  Database,
  FlaskConical,
  LineChart,
  Settings2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/stores/useAppStore";
import type { AppSection } from "@/lib/types";

interface NavItem {
  id: AppSection;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
}

const navItems: NavItem[] = [
  { id: "data", label: "Data", icon: Database },
  { id: "strategy", label: "Strategy", icon: FlaskConical },
  { id: "backtest", label: "Backtest", icon: LineChart },
  { id: "optimization", label: "Optimize", icon: Settings2 },
  { id: "export", label: "Export", icon: Code2 },
];

export function Sidebar() {
  const activeSection = useAppStore((s) => s.activeSection);
  const setActiveSection = useAppStore((s) => s.setActiveSection);

  return (
    <aside className="flex h-full w-[140px] shrink-0 flex-col border-r border-border/60 bg-card pt-5 pb-4">
      <div className="mb-6 flex items-center gap-2 px-4">
        <div className="h-2 w-2 rounded-full bg-primary" />
        <div className="h-2 w-2 rounded-full bg-primary/40" />
      </div>
      <nav className="flex flex-col gap-0.5 px-1.5">
        {navItems.map((item) => {
          const Icon = item.icon;
          const isActive = activeSection === item.id;
          return (
            <button
              key={item.id}
              onClick={() => setActiveSection(item.id)}
              className={cn(
                "flex w-full items-center gap-1.5 overflow-hidden rounded px-2 py-2 text-[10px] font-medium uppercase tracking-[0.12em] transition-colors",
                isActive
                  ? "text-primary"
                  : "text-muted-foreground hover:text-foreground"
              )}
            >
              {isActive && <span className="shrink-0 text-primary/60">[</span>}
              <Icon className={cn("h-3.5 w-3.5 shrink-0", isActive ? "text-primary" : "")} />
              <span className="truncate">{item.label}</span>
              {isActive && <span className="shrink-0 text-primary/60">]</span>}
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
