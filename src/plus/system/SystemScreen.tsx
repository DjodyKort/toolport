import { useState, type ComponentType } from "react";
import { NotBuiltPanel } from "../NotBuilt";
import { NOT_BUILT_TABS } from "../notBuiltTabs";
import { useRestoreFocus } from "../agents/useRestoreFocus";
import { Tabs } from "../ui";
import { CouncilTab } from "./CouncilTab";
import { ImportTab } from "./ImportTab";
import { SelfTab } from "./SelfTab";
import { SyncTab } from "./SyncTab";
import { UpdatesTab } from "./UpdatesTab";

type Panel = ComponentType<{ onOpenCommands: (group?: string) => void }>;

/** The tabs of the System screen that are built. A tab not listed here is still the marked
 * placeholder of `NOT_BUILT_TABS`. */
const PANELS: Record<string, Panel> = {
  sync: SyncTab,
  updates: UpdatesTab,
  council: CouncilTab,
  import: ImportTab,
  self: SelfTab,
};

const TABS = NOT_BUILT_TABS.system ?? [];

/** The System screen: Sync, Updates, Council, Import and the self-management MCP as tabs. */
export function SystemScreen({
  initialTab = "sync",
  onOpenCommands,
}: {
  initialTab?: string;
  onOpenCommands: (group?: string) => void;
}) {
  useRestoreFocus();
  const [tab, setTab] = useState(initialTab);
  const current = TABS.find((item) => item.id === tab) ?? TABS[0];
  const Panel = current ? PANELS[current.id] : undefined;
  return (
    <Tabs
      items={TABS.map(({ id, label }) => ({ id, label }))}
      value={current?.id ?? ""}
      onValueChange={setTab}
      label="System sections"
    >
      {Panel ? (
        <Panel onOpenCommands={onOpenCommands} />
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
  );
}
