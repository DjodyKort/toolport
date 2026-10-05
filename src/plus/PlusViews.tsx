import { lazy, Suspense, useState } from "react";
import type { View } from "@/lib/types";
import type { PlusView } from "./nav";
import type { TabId } from "./servers/useServers";
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
const AttentionScreen = lazy(() =>
  import("./attention/AttentionScreen").then((m) => ({ default: m.AttentionScreen })),
);

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
  const [target, setTarget] = useState<{
    view: PlusView;
    params: Record<string, string>;
  } | null>(null);
  if (target && target.view !== view) setTarget(null);
  const navigate = (next: PlusView, params: Record<string, string>) => {
    setTarget({ view: next, params });
    onSelectView(next);
  };
  const openTab = (next: PlusView, tab: string) => navigate(next, { tab });
  const params = target?.view === view ? target.params : undefined;
  const tabOf = (own: PlusView) => (target?.view === own ? target.params.tab : undefined);
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
          initialTab={params?.tab as TabId | undefined}
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
        <TokensScreen initialTab={params?.tab} onOpenCommands={openCommands} />
      ) : view === "context" ? (
        <ContextScreen
          key={tabOf("context") ?? ""}
          initialTab={tabOf("context")}
          onOpenCommands={openCommands}
          onOpenPlugins={() => openTab("library", "plugins")}
        />
      ) : view === "system" ? (
        <SystemScreen initialTab={params?.tab} onOpenCommands={openCommands} />
      ) : view === "tasks" ? (
        <TasksScreen
          initialTab={params?.tab}
          initialTask={params?.task}
          onOpenCommands={openCommands}
        />
      ) : view === "attention" ? (
        <AttentionScreen onNavigate={navigate} onOpenCommands={openCommands} />
      ) : (
        <NotBuilt view={view} onOpenCommands={openCommands} />
      )}
    </Suspense>
  );
}
