import { Bot, Zap, TrendingUp, BarChart2, Lock } from "lucide-react";
import type { TemplateKey } from "@/stores/useAppStore";

interface Props {
  onSelect: (type: "builder", templateKey: TemplateKey) => void;
  onClose: () => void;
}

const TEMPLATES: { key: TemplateKey; label: string; desc: string; icon: React.ReactNode }[] = [
  { key: "blank", label: "Blank", desc: "Start from scratch with all defaults", icon: <Bot className="h-5 w-5 text-primary" /> },
  { key: "scalper", label: "Scalper", desc: "Short rules, tight stop-loss, quick exits", icon: <Zap className="h-5 w-5 text-yellow-400" /> },
  { key: "swing", label: "Swing Trader", desc: "Longer lookback, 2–4 entry conditions", icon: <TrendingUp className="h-5 w-5 text-sky-400" /> },
  { key: "trend", label: "Trend Follower", desc: "Long-only, 2–3 rules, trend-based", icon: <BarChart2 className="h-5 w-5 text-emerald-400" /> },
];

export function TaskTypePicker({ onSelect, onClose }: Props) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      onClick={onClose}
    >
      <div
        className="w-80 rounded-lg border border-border bg-card p-5 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="mb-4 text-sm font-semibold text-foreground">Add task</h2>
        <div className="flex flex-col gap-2">
          {TEMPLATES.map(({ key, label, desc, icon }) => (
            <button
              key={key}
              onClick={() => onSelect("builder", key)}
              className="flex items-center gap-3 rounded-lg border border-border bg-background p-3 text-left transition-colors hover:border-primary/50 hover:bg-primary/5"
            >
              {icon}
              <div>
                <p className="text-sm font-medium text-foreground">{label}</p>
                <p className="text-xs text-muted-foreground">{desc}</p>
              </div>
            </button>
          ))}
          <button
            disabled
            className="flex cursor-not-allowed items-center gap-3 rounded-lg border border-border/40 bg-background/50 p-3 text-left opacity-50"
          >
            <Lock className="h-4 w-4 text-muted-foreground" />
            <div>
              <p className="text-sm font-medium text-muted-foreground">Retester</p>
              <p className="text-xs text-muted-foreground">Coming soon</p>
            </div>
          </button>
        </div>
      </div>
    </div>
  );
}
