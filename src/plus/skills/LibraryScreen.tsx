import { useState, type ComponentType } from "react";
import { AgentsTab, StylesTab } from "../agents";
import { NotBuiltPanel } from "../NotBuilt";
import { NOT_BUILT_TABS } from "../notBuiltTabs";
import { Tabs } from "../ui";
import { SkillsTab } from "./SkillsTab";

/** The tabs of the Library screen that are built. A tab not listed here is still the marked
 * placeholder of `NOT_BUILT_TABS`; the item that builds a tab adds its panel to this map. */
const PANELS: Record<string, ComponentType> = {
  skills: SkillsTab,
  agents: AgentsTab,
  styles: StylesTab,
};

const TABS = NOT_BUILT_TABS.library ?? [];

/** The Library screen: Skills, Agents, Styles, Plugins and Sources as tabs. */
export function LibraryScreen({
  initialTab = "skills",
  onOpenCommands,
}: {
  initialTab?: string;
  onOpenCommands: (group?: string) => void;
}) {
  const [tab, setTab] = useState(initialTab);
  const current = TABS.find((item) => item.id === tab) ?? TABS[0];
  const Panel = current ? PANELS[current.id] : undefined;
  return (
    <Tabs
      items={TABS.map(({ id, label }) => ({ id, label }))}
      value={current?.id ?? ""}
      onValueChange={setTab}
      label="Library sections"
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
  );
}
