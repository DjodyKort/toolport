import { lazy, Suspense, useState, type ComponentType } from "react";
import { NotBuiltPanel } from "../NotBuilt";
import { NOT_BUILT_TABS } from "../notBuiltTabs";
import { ScreenSkeleton, Tabs, useCtlQuery } from "../ui";
import type { CommandsData } from "../bridge/data";
import { useFolderChoice, type FolderChoice } from "./folder";
import { HereTab } from "./HereTab";
import { LaunchTab } from "./LaunchTab";
import { LayersTab } from "./LayersTab";
import { ProfilesTab } from "./ProfilesTab";
import { RowsContext, RowsReloadContext } from "./parts";

/** The tabs of the Context screen that are built. A tab not listed here is still the marked
 * placeholder of `NOT_BUILT_TABS`; the item that builds a tab adds its panel to this map. */
interface PanelProps {
  openTab: (id: string) => void;
  here: FolderChoice;
}

const HooksTabLazy = lazy(() =>
  import("../hooks/HooksTab").then((m) => ({ default: m.HooksTab })),
);

function HooksPanel({ here }: PanelProps) {
  return (
    <Suspense fallback={<ScreenSkeleton label="Loading tab" />}>
      <HooksTabLazy here={here} />
    </Suspense>
  );
}

const PANELS: Record<string, ComponentType<PanelProps>> = {
  here: HereTab,
  profiles: ProfilesTab,
  layers: LayersTab,
  hooks: HooksPanel,
  launch: LaunchTab,
};

const TABS = NOT_BUILT_TABS.context ?? [];

/** The Context screen: This folder, Profiles, Layers, Hooks and Launch & shell as tabs. */
export function ContextScreen({
  initialTab = "here",
  onOpenCommands,
}: {
  initialTab?: string;
  onOpenCommands: (group?: string) => void;
}) {
  const [tab, setTab] = useState(initialTab);
  const here = useFolderChoice();
  const registry = useCtlQuery<CommandsData>(["commands"]);
  const current = TABS.find((item) => item.id === tab) ?? TABS[0];
  const Panel = current ? PANELS[current.id] : undefined;
  return (
    <RowsContext.Provider value={registry.data?.commands ?? null}>
      <RowsReloadContext.Provider value={registry.reload}>
        <Tabs
          items={TABS.map(({ id, label }) => ({ id, label }))}
          value={current?.id ?? ""}
          onValueChange={setTab}
          label="Context sections"
        >
          {Panel ? (
            <Panel openTab={setTab} here={here} />
          ) : (
            current && (
              <NotBuiltPanel
                name={current.label}
                builtBy={current.builtBy}
                group={current.group}
                onOpenCommands={onOpenCommands}
              />
            )
          )}
        </Tabs>
      </RowsReloadContext.Provider>
    </RowsContext.Provider>
  );
}
