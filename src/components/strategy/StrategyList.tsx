import type { Strategy } from "@/lib/types";
import { Button } from "@/components/ui/Button";
import { Card, CardContent } from "@/components/ui/Card";
import { Upload, Trash2, FileText } from "lucide-react";
import { deleteStrategy } from "@/lib/tauri";

interface StrategyListProps {
  strategies: Strategy[];
  onLoad: (strategy: Strategy) => void;
  onDeleted: (id: string) => void;
}

export function StrategyList({
  strategies,
  onLoad,
  onDeleted,
}: StrategyListProps) {
  const handleDelete = async (id: string) => {
    try {
      await deleteStrategy(id);
      onDeleted(id);
    } catch (err) {
      console.error("Failed to delete strategy:", err);
    }
  };

  if (strategies.length === 0) {
    return (
      <div className="py-8 text-center text-sm text-muted-foreground">
        <FileText className="mx-auto mb-2 h-8 w-8 opacity-40" />
        No saved strategies yet.
      </div>
    );
  }

  return (
    <div className="space-y-2">
      {strategies.map((s) => (
        <Card key={s.id} className="overflow-hidden">
          <CardContent className="flex items-center gap-3 p-3">
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium">{s.name}</p>
              <p className="text-xs text-muted-foreground">
                {s.long_entry_rules.length + s.short_entry_rules.length} entry / {s.long_exit_rules.length + s.short_exit_rules.length} exit rules
                {s.updated_at ? ` \u00b7 ${s.updated_at.slice(0, 10)}` : ""}
              </p>
            </div>
            <Button
              variant="outline"
              size="sm"
              className="shrink-0"
              onClick={() => onLoad(s)}
            >
              <Upload className="mr-1 h-3.5 w-3.5" />
              Load
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 shrink-0 text-muted-foreground hover:text-destructive"
              onClick={() => handleDelete(s.id)}
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
