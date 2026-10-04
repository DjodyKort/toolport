import { lazy, Suspense, useState } from "react";
import type { View } from "@/lib/types";
import type { PlusView } from "./nav";
import { ScreenSkeleton } from "./ui/States";

const AllCommandsPage = lazy(() =>
  import("./allcommands/AllCommandsPage").then((m) => ({ default: m.AllCommandsPage })),
);
const ServersScreen = lazy(() =>
  import("./servers/ServersScreen").then((m) => ({ default: m.ServersScreen })),
const LoginsScreen = lazy(() =>
  import("./logins/LoginsScreen").then((m) => ({ default: m.LoginsScreen })),
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
      ) : (
        <NotBuilt view={view} onOpenCommands={openCommands} />
      )}
    </Suspense>
  );
}
