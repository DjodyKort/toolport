import { useState, type ComponentType } from "react";
import { NotBuiltPanel } from "../NotBuilt";
import { NOT_BUILT_TABS } from "../notBuiltTabs";
import { Tabs } from "../ui";
import { CompressionTab } from "./CompressionTab";

/** The tabs of the Tokens screen that are built. A tab not listed here is still the marked
 * placeholder of `NOT_BUILT_TABS`; MIG-GUI-7 adds `usage: UsageTab` from `src/plus/usage`. */
export const PANELS: Record<string, ComponentType> = {
  compression: CompressionTab,
};

const TABS = NOT_BUILT_TABS.tokens ?? [];

/** The Tokens screen: Usage and Compression as tabs. */
export function TokensScreen({
  initialTab = "usage",
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
      label="Tokens sections"
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
