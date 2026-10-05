import { lazy, Suspense, useEffect, useState } from "react";
import type { View } from "@/lib/types";
import type { PlusView } from "./nav";
import { ScreenSkeleton } from "./ui/States";

const AllCommandsPage = lazy(() =>
  import("./allcommands/AllCommandsPage").then((m) => ({ default: m.AllCommandsPage })),
);
const ServersScreen = lazy(() =>
  import("./servers/ServersScreen").then((m) => ({ default: m.ServersScreen })),
);
const LoginsScreen = lazy(() =>
  import("./logins/LoginsScreen").then((m) => ({ default: m.LoginsScreen })),
);
const LibraryScreen = lazy(() =>
  import("./skills/LibraryScreen").then((m) => ({ default: m.LibraryScreen })),
);
const TokensScreen = lazy(() =>
  import("./compression/TokensScreen").then((m) => ({ default: m.TokensScreen })),
);
const ContextScreen = lazy(() =>
  import("./context/ContextScreen").then((m) => ({ default: m.ContextScreen })),
);
const SystemScreen = lazy(() =>
  import("./system/SystemScreen").then((m) => ({ default: m.SystemScreen })),
);
const TasksScreen = lazy(() =>
  import("./tasks/TasksScreen").then((m) => ({ default: m.TasksScreen })),
);
const NotBuilt = lazy(() => import("./NotBuilt").then((m) => ({ default: m.NotBuilt })));

/** The single entry for every Toolport+ screen. Each screen is its own chunk, so the app
 * only loads the one that is open. A screen no item has built yet is a marked placeholder
 * that names the item and leads to the All commands page. */
export function PlusViews({
  view,
  onSelectView,
}: {
  view: PlusView;
  onSelectView: (view: View) => void;
}) {
  const [group, setGroup] = useState<string | undefined>();
  const [target, setTarget] = useState<{ view: PlusView; tab: string } | null>(null);
  useEffect(() => {
    if (target && view !== target.view) setTarget(null);
  }, [view, target]);
  const openTab = (next: PlusView, tab: string) => {
    setTarget({ view: next, tab });
    onSelectView(next);
  };
  const tabOf = (own: PlusView) => (target?.view === own ? target.tab : undefined);
  const openCommands = (next?: string) => {
    setGroup(next);
    onSelectView("commands");
  };
  return (
    <Suspense fallback={<ScreenSkeleton label="Loading screen" />}>
      {view === "commands" ? (
        <AllCommandsPage key={group ?? ""} initialGroup={group} />
      ) : view === "control" ? (
        <ServersScreen
          onOpenCommands={openCommands}
          onOpenClassic={() => onSelectView("servers")}
        />
      ) : view === "logins" ? (
        <LoginsScreen onOpenCommands={openCommands} />
      ) : view === "library" ? (
        <LibraryScreen
          key={tabOf("library") ?? ""}
          initialTab={tabOf("library")}
          onOpenCommands={openCommands}
          onOpenHooks={() => openTab("context", "hooks")}
        />
      ) : view === "tokens" ? (
        <TokensScreen onOpenCommands={openCommands} />
      ) : view === "context" ? (
        <ContextScreen
          key={tabOf("context") ?? ""}
          initialTab={tabOf("context")}
          onOpenCommands={openCommands}
          onOpenPlugins={() => openTab("library", "plugins")}
        />
      ) : view === "system" ? (
        <SystemScreen onOpenCommands={openCommands} />
      ) : view === "tasks" ? (
        <TasksScreen onOpenCommands={openCommands} />
      ) : (
        <NotBuilt view={view} onOpenCommands={openCommands} />
      )}
    </Suspense>
  );
}
