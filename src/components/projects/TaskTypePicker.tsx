import { Bot, Lock } from "lucide-react";

interface Props {
  onSelect: (type: "builder") => void;
  onClose: () => void;
}

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
          <button
            onClick={() => onSelect("builder")}
            className="flex items-center gap-3 rounded-lg border border-border bg-background p-3 text-left transition-colors hover:border-primary/50 hover:bg-primary/5"
          >
            <Bot className="h-5 w-5 text-primary" />
            <div>
              <p className="text-sm font-medium text-foreground">Builder</p>
              <p className="text-xs text-muted-foreground">Discover strategies using genetic algorithms</p>
            </div>
          </button>
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
