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
    <aside className="flex h-full w-16 shrink-0 flex-col border-r border-border/30 bg-background pt-4 pb-3">
      <div className="mb-5 flex justify-center">
        <div className="h-1.5 w-1.5 rounded-full bg-primary shadow-sm shadow-primary/40" />
      </div>
      <nav className="flex flex-col gap-0.5 px-1">
        {navItems.map((item) => {
          const Icon = item.icon;
          const isActive = activeSection === item.id;
          return (
            <button
              key={item.id}
              onClick={() => setActiveSection(item.id)}
              title={item.label}
              className={cn(
                "flex w-full flex-col items-center justify-center gap-1 rounded px-1 py-2.5 transition-colors",
                isActive
                  ? "text-primary bg-primary/5"
                  : "text-muted-foreground hover:text-foreground hover:bg-primary/5"
              )}
            >
              <Icon className={cn("h-4 w-4 shrink-0", isActive ? "text-primary" : "")} />
              <span className="text-[10px] font-medium uppercase tracking-[0.1em]">{item.label}</span>
            </button>
          );
        })}
      </nav>
    </aside>
  );
}
