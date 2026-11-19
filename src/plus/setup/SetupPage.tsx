import { lazy, Suspense, useState } from "react";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";

const ExportView = lazy(() =>
  import("./ExportView").then((m) => ({ default: m.ExportView })),
);
const RestoreView = lazy(() =>
  import("./RestoreView").then((m) => ({ default: m.RestoreView })),
);
const ResetView = lazy(() =>
  import("./ResetView").then((m) => ({ default: m.ResetView })),
);
const SyncView = lazy(() => import("./SyncView"));

type View = "export" | "restore" | "reset" | "sync";

const VIEWS: { id: View; label: string }[] = [
  { id: "export", label: "Export" },
  { id: "restore", label: "Restore" },
  { id: "reset", label: "Before a reset" },
  { id: "sync", label: "Sync" },
];

/** Settings > Toolport+ > Setup & backup: export a setup file, restore one from a plan, and the
 * checklist before a reset. Each view is its own chunk. */
export function SetupPage({ onBack }: { onBack: () => void }) {
  const [view, setView] = useState<View>("export");
  // The reset checklist sends a person to Export with parts already ticked.
  const [seed, setSeed] = useState<string[] | null>(null);
  const [exportKey, setExportKey] = useState(0);

  const openExport = (parts: string[] | null) => {
    setSeed(parts);
    setExportKey((k) => k + 1);
    setView("export");
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-2">
        <Button variant="ghost" size="sm" onClick={onBack}>
          <ArrowLeft /> Back
        </Button>
        <h2 className="text-base font-semibold">Setup &amp; backup</h2>
      </div>
      <div role="tablist" className="flex flex-wrap gap-2">
        {VIEWS.map((entry) => (
          <Button
            key={entry.id}
            role="tab"
            aria-selected={view === entry.id}
            variant={view === entry.id ? "default" : "outline"}
            size="sm"
            onClick={() => setView(entry.id)}
          >
            {entry.label}
          </Button>
        ))}
      </div>
      <Suspense fallback={<p className="text-meta text-muted-foreground">Loading…</p>}>
        {view === "export" && (
          <ExportView
            key={exportKey}
            seedParts={seed}
            onOpenReset={() => setView("reset")}
          />
        )}
        {view === "restore" && <RestoreView />}
        {view === "reset" && (
          <ResetView
            onExport={() => openExport(null)}
            onExportSessions={() =>
              openExport([
                "repos",
                "servers",
                "access",
                "library",
                "tasks",
                "settings",
                "claudeUser",
                "memory",
                "history",
              ])
            }
          />
        )}
        {view === "sync" && <SyncView />}
      </Suspense>
    </div>
  );
}
