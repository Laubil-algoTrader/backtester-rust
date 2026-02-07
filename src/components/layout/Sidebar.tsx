import {
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
];

export function Sidebar() {
  const activeSection = useAppStore((s) => s.activeSection);
  const setActiveSection = useAppStore((s) => s.setActiveSection);

  return (
    <aside className="flex h-full w-16 flex-col items-center border-r bg-card py-4 gap-1">
      {navItems.map((item) => {
        const Icon = item.icon;
        const isActive = activeSection === item.id;
        return (
          <button
            key={item.id}
            onClick={() => setActiveSection(item.id)}
            className={cn(
              "flex flex-col items-center justify-center w-12 h-12 rounded-lg text-xs gap-0.5 transition-colors",
              isActive
                ? "bg-primary/10 text-primary"
                : "text-muted-foreground hover:bg-accent hover:text-accent-foreground"
            )}
          >
            <Icon className="h-5 w-5" />
            <span className="text-[10px] leading-none">{item.label}</span>
          </button>
        );
      })}
    </aside>
  );
}
