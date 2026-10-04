import type { PlusView } from "./nav";

export interface PlusTab {
  id: string;
  label: string;
  /** The migration item that builds this tab. */
  builtBy: string;
  /** The command group the All commands page offers until the tab exists. */
  group?: string;
}

/** The tabs of the mockup for a screen that is not built yet. Only the placeholder reads
 * them, so they stay out of the startup bundle. */
export const NOT_BUILT_TABS: Partial<Record<PlusView, PlusTab[]>> = {
  library: [
    { id: "skills", label: "Skills", builtBy: "MIG-GUI-3", group: "skills" },
    { id: "agents", label: "Agents", builtBy: "MIG-GUI-4", group: "agents" },
    { id: "styles", label: "Styles", builtBy: "MIG-GUI-4", group: "styles" },
    { id: "plugins", label: "Plugins", builtBy: "MIG-GUI-12" },
    { id: "sources", label: "Sources", builtBy: "MIG-GUI-10" },
  ],
  context: [
    { id: "here", label: "This folder", builtBy: "MIG-GUI-10", group: "context" },
    { id: "profiles", label: "Profiles", builtBy: "MIG-GUI-10", group: "context" },
    { id: "layers", label: "Layers", builtBy: "MIG-GUI-10", group: "context" },
    { id: "hooks", label: "Hooks", builtBy: "MIG-GUI-12" },
    { id: "launch", label: "Launch & shell", builtBy: "MIG-GUI-6", group: "context" },
  ],
  tokens: [
    { id: "usage", label: "Usage", builtBy: "MIG-GUI-7", group: "usage" },
    {
      id: "compression",
      label: "Compression",
      builtBy: "MIG-GUI-5",
      group: "compression",
    },
  ],
  tasks: [
    { id: "tasks", label: "Tasks", builtBy: "MIG-AUTO-2" },
    { id: "history", label: "History", builtBy: "MIG-AUTO-2" },
  ],
  system: [
    { id: "sync", label: "Sync", builtBy: "MIG-GUI-8", group: "sync" },
    { id: "updates", label: "Updates", builtBy: "MIG-GUI-8", group: "update" },
    { id: "council", label: "Council", builtBy: "MIG-GUI-8", group: "council" },
    { id: "import", label: "Import", builtBy: "MIG-GUI-8", group: "import" },
    { id: "self", label: "Self-management", builtBy: "MIG-GUI-8", group: "mcp" },
  ],
};
