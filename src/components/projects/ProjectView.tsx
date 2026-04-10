import { useState } from "react";
import { ArrowLeft, Pencil, Plus, Play, Trash2, GitCompare } from "lucide-react";
import { toast } from "sonner";
import { useAppStore } from "@/stores/useAppStore";
import { TaskTypePicker } from "./TaskTypePicker";
import { TaskCompareModal } from "./TaskCompareModal";
import { BuilderPage } from "@/components/builder/BuilderPage";

export function ProjectView() {
  const projects = useAppStore((s) => s.projects);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const activeProjectTaskId = useAppStore((s) => s.activeProjectTaskId);
  const addTaskToProject = useAppStore((s) => s.addTaskToProject);
  const deleteTaskFromProject = useAppStore((s) => s.deleteTaskFromProject);
  const openProjectTask = useAppStore((s) => s.openProjectTask);
  const renameProject = useAppStore((s) => s.renameProject);

  const [showPicker, setShowPicker] = useState(false);
  const [showCompare, setShowCompare] = useState(false);
  const [editingName, setEditingName] = useState(false);
  const [nameInput, setNameInput] = useState("");

  const project = projects.find((p) => p.id === activeProjectId);
  if (!project) return null;

  // When a task is open, render the builder inline — no navigation away from this project.
  // ProjectBreadcrumb (inside BuilderPage) provides the back button to return here.
  if (activeProjectTaskId) {
    return <BuilderPage />;
  }

  const handleBack = () => {
    useAppStore.setState({ activeProjectId: null });
  };

  const handleStartRename = () => {
    setNameInput(project.name);
    setEditingName(true);
  };

  const handleRename = async () => {
    const name = nameInput.trim();
    if (name && name !== project.name) await renameProject(project.id, name);
    setEditingName(false);
  };

  const handleOpenTask = async (taskId: string) => {
    try {
      await openProjectTask(project.id, taskId);
    } catch (e: unknown) {
      if (e instanceof Error && e.message.includes("Stop the builder")) {
        toast.warning("Stop the builder before switching tasks");
      } else {
        toast.error("Failed to save task before switching");
      }
    }
  };

  const handleDeleteTask = async (taskId: string, name: string) => {
    if (!confirm(`Delete task "${name}"?`)) return;
    await deleteTaskFromProject(project.id, taskId);
  };

  return (
    <div className="flex h-full flex-col gap-4 p-6">
      {/* Header */}
      <div className="flex items-center gap-3">
        <button
          onClick={handleBack}
          className="flex items-center gap-1 text-xs text-muted-foreground transition-colors hover:text-foreground"
        >
          <ArrowLeft className="h-3.5 w-3.5" />
          Custom Projects
        </button>
        <span className="text-muted-foreground">/</span>
        {editingName ? (
          <input
            autoFocus
            className="rounded border border-border bg-card px-2 py-0.5 text-sm font-semibold outline-none focus:ring-1 focus:ring-primary"
            value={nameInput}
            onChange={(e) => setNameInput(e.target.value)}
            onBlur={handleRename}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleRename();
              if (e.key === "Escape") setEditingName(false);
            }}
          />
        ) : (
          <button
            onClick={handleStartRename}
            className="flex items-center gap-1.5 text-sm font-semibold text-foreground transition-colors hover:text-primary"
          >
            {project.name}
            <Pencil className="h-3 w-3 text-muted-foreground" />
          </button>
        )}
      </div>

      {/* Task list */}
      {project.tasks.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-muted-foreground">
          <p className="text-sm">No tasks yet. Add one to get started.</p>
        </div>
      ) : (
        <div className="flex-1 overflow-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border/30 text-xs uppercase tracking-wide text-muted-foreground">
                <th className="pb-2 text-left font-medium">Task</th>
                <th className="pb-2 text-left font-medium">Type</th>
                <th className="pb-2 text-left font-medium">Databanks</th>
                <th className="pb-2 text-left font-medium">Strategies</th>
                <th className="pb-2 text-left font-medium">Status</th>
                <th className="pb-2" />
              </tr>
            </thead>
            <tbody>
              {project.tasks.map((task) => (
                <tr key={task.id} className="border-b border-border/20 hover:bg-muted/10">
                  <td className="py-2.5 font-medium">{task.name}</td>
                  <td className="py-2.5">
                    <span className="rounded bg-primary/10 px-1.5 py-0.5 text-xs text-primary">
                      {task.type}
                    </span>
                  </td>
                  <td className="py-2.5 text-muted-foreground">{task.databankCount}</td>
                  <td className="py-2.5 text-muted-foreground">{task.strategiesCount}</td>
                  <td className="py-2.5">
                    <span
                      className={
                        task.status === "running"
                          ? "text-xs text-green-500"
                          : "text-xs text-muted-foreground"
                      }
                    >
                      {task.status}
                    </span>
                  </td>
                  <td className="py-2.5">
                    <div className="flex justify-end gap-2">
                      <button
                        onClick={() => handleOpenTask(task.id)}
                        className="flex items-center gap-1 rounded bg-primary/10 px-2 py-0.5 text-xs text-primary transition-colors hover:bg-primary/20"
                      >
                        <Play className="h-3 w-3" />
                        Open
                      </button>
                      <button
                        onClick={() => handleDeleteTask(task.id, task.name)}
                        className="rounded p-1 text-muted-foreground transition-colors hover:text-destructive"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Footer */}
      <div className="flex items-center gap-2 border-t border-border/30 pt-2">
        <button
          onClick={() => setShowPicker(true)}
          className="flex items-center gap-1.5 rounded px-3 py-1.5 text-xs bg-primary text-primary-foreground hover:opacity-90"
        >
          <Plus className="h-3.5 w-3.5" />
          Add task
        </button>
        <button
          onClick={() => setShowCompare(true)}
          disabled={project.tasks.length < 2}
          className="flex items-center gap-1.5 rounded border border-border/40 px-3 py-1.5 text-xs text-muted-foreground hover:border-primary/50 hover:text-primary disabled:opacity-40 disabled:cursor-not-allowed"
        >
          <GitCompare className="h-3.5 w-3.5" />
          Compare
        </button>
      </div>

      {showPicker && (
        <TaskTypePicker
          onSelect={async () => {
            setShowPicker(false);
            await addTaskToProject(project.id, "builder", "blank");
          }}
          onClose={() => setShowPicker(false)}
        />
      )}

      {showCompare && (
        <TaskCompareModal project={project} onClose={() => setShowCompare(false)} />
      )}
    </div>
  );
}
