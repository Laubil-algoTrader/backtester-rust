import { ArrowLeft, Save } from "lucide-react";
import { toast } from "sonner";
import { useAppStore } from "@/stores/useAppStore";

export function ProjectBreadcrumb() {
  const projects = useAppStore((s) => s.projects);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const activeProjectTaskId = useAppStore((s) => s.activeProjectTaskId);
  const activeProjectTaskDirty = useAppStore((s) => s.activeProjectTaskDirty);
  const closeProjectTask = useAppStore((s) => s.closeProjectTask);
  const saveActiveProjectTask = useAppStore((s) => s.saveActiveProjectTask);

  const project = projects.find((p) => p.id === activeProjectId);
  const task = project?.tasks.find((t) => t.id === activeProjectTaskId);

  if (!project || !task) return null;

  const handleBack = async () => {
    await closeProjectTask();
  };

  const handleSave = async () => {
    await saveActiveProjectTask();
    toast.success("Task saved");
  };

  return (
    <div className="flex items-center justify-between border-b border-border/30 bg-card/50 px-4 py-2 text-xs">
      <div className="flex items-center gap-2 text-muted-foreground">
        <button
          onClick={handleBack}
          className="flex items-center gap-1 transition-colors hover:text-foreground"
        >
          <ArrowLeft className="h-3.5 w-3.5" />
          {project.name}
        </button>
        <span>/</span>
        <span className="font-medium text-foreground">{task.name}</span>
        {activeProjectTaskDirty && (
          <span className="text-[10px] text-warning opacity-70">● unsaved</span>
        )}
      </div>
      <button
        onClick={handleSave}
        className="flex items-center gap-1.5 rounded border border-border px-2 py-1 text-xs text-muted-foreground transition-colors hover:border-primary/50 hover:text-foreground"
      >
        <Save className="h-3 w-3" />
        Save task
      </button>
    </div>
  );
}
