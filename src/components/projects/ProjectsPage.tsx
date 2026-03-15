import { useState } from "react";
import { FolderOpen, Plus, Trash2, ExternalLink } from "lucide-react";
import { format } from "date-fns";
import { toast } from "sonner";
import { useAppStore } from "@/stores/useAppStore";
import { openProjectFromPath } from "@/lib/tauri";

export function ProjectsPage() {
  const projects = useAppStore((s) => s.projects);
  const createProject = useAppStore((s) => s.createProject);
  const deleteProject = useAppStore((s) => s.deleteProject);
  const importProject = useAppStore((s) => s.importProject);

  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");

  const handleCreate = async () => {
    const name = newName.trim() || `Project ${projects.length + 1}`;
    await createProject(name);
    setNewName("");
    setCreating(false);
  };

  const handleOpenProject = (id: string) => {
    useAppStore.setState({ activeProjectId: id });
  };

  const handleDelete = async (id: string, name: string) => {
    if (!confirm(`Delete project "${name}"? This cannot be undone.`)) return;
    await deleteProject(id);
    toast.success("Project deleted");
  };

  const handleOpenFromDisk = async () => {
    try {
      const project = await openProjectFromPath();
      if (!project) return;
      await importProject(project);
      useAppStore.setState({ activeProjectId: project.id });
    } catch {
      toast.error("Failed to open project file");
    }
  };

  return (
    <div className="flex h-full flex-col gap-4 p-6">
      <div className="flex items-center gap-2">
        <FolderOpen className="h-5 w-5 text-primary" />
        <h1 className="text-lg font-semibold text-foreground">Custom Projects</h1>
      </div>

      {projects.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-muted-foreground">
          <FolderOpen className="h-10 w-10 opacity-20" />
          <p className="text-sm">No projects yet. Create one to get started.</p>
        </div>
      ) : (
        <div className="flex-1 overflow-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border/30 text-xs uppercase tracking-wide text-muted-foreground">
                <th className="pb-2 text-left font-medium">Name</th>
                <th className="pb-2 text-left font-medium">Tasks</th>
                <th className="pb-2 text-left font-medium">Created</th>
                <th className="pb-2" />
              </tr>
            </thead>
            <tbody>
              {projects.map((p) => (
                <tr key={p.id} className="border-b border-border/20 hover:bg-muted/10">
                  <td className="py-2.5 font-medium">{p.name}</td>
                  <td className="py-2.5 text-muted-foreground">{p.tasks.length}</td>
                  <td className="py-2.5 text-muted-foreground">
                    {format(new Date(p.createdAt), "dd MMM yyyy")}
                  </td>
                  <td className="py-2.5">
                    <div className="flex justify-end gap-2">
                      <button
                        onClick={() => handleOpenProject(p.id)}
                        className="rounded px-2 py-0.5 text-xs bg-primary/10 text-primary hover:bg-primary/20 transition-colors"
                      >
                        Open
                      </button>
                      <button
                        onClick={() => handleDelete(p.id, p.name)}
                        className="rounded p-1 text-muted-foreground hover:text-destructive transition-colors"
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

      {creating && (
        <div className="flex items-center gap-2">
          <input
            autoFocus
            className="flex-1 rounded border border-border bg-card px-3 py-1.5 text-sm outline-none focus:ring-1 focus:ring-primary"
            placeholder="Project name…"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleCreate();
              if (e.key === "Escape") setCreating(false);
            }}
          />
          <button
            onClick={handleCreate}
            className="rounded px-3 py-1.5 text-xs bg-primary text-primary-foreground hover:opacity-90"
          >
            Create
          </button>
          <button
            onClick={() => setCreating(false)}
            className="rounded border border-border px-3 py-1.5 text-xs text-muted-foreground hover:text-foreground"
          >
            Cancel
          </button>
        </div>
      )}

      <div className="flex gap-2 border-t border-border/30 pt-2">
        <button
          onClick={() => setCreating(true)}
          className="flex items-center gap-1.5 rounded px-3 py-1.5 text-xs bg-primary text-primary-foreground hover:opacity-90"
        >
          <Plus className="h-3.5 w-3.5" />
          New project
        </button>
        <button
          onClick={handleOpenFromDisk}
          className="flex items-center gap-1.5 rounded border border-border px-3 py-1.5 text-xs text-muted-foreground hover:text-foreground"
        >
          <ExternalLink className="h-3.5 w-3.5" />
          Open existing…
        </button>
      </div>
    </div>
  );
}
