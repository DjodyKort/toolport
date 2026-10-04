import { useState, type ComponentType } from "react";
import { NotBuiltPanel } from "../NotBuilt";
import { NOT_BUILT_TABS } from "../notBuiltTabs";
import { Tabs, useCtlQuery } from "../ui";
import type { CommandsData } from "../bridge/data";
import { LaunchTab } from "./LaunchTab";
import { RowsContext, RowsReloadContext } from "./parts";

/** The tabs of the Context screen that are built. A tab not listed here is still the marked
 * placeholder of `NOT_BUILT_TABS`; the item that builds a tab adds its panel to this map. */
const PANELS: Record<string, ComponentType> = {
  launch: LaunchTab,
};

const TABS = NOT_BUILT_TABS.context ?? [];

/** The Context screen: This folder, Profiles, Layers, Hooks and Launch & shell as tabs. */
export function ContextScreen({
  initialTab = "launch",
  onOpenCommands,
}: {
  initialTab?: string;
  onOpenCommands: (group?: string) => void;
}) {
  const [tab, setTab] = useState(initialTab);
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
            <Panel />
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
