import { useState } from "react";
import { Tabs } from "../ui";
import { AgentsTab } from "./AgentsTab";
import { AGENT_TABS, type AgentTabId } from "./tabs";
import { StylesTab } from "./StylesTab";

/** The Agents and Styles panels as one screen. The Library screen mounts the same two panels
 * as tabs of its own; until it exists this screen is the way to them. */
export function AgentsScreen({ initialTab = "agents" }: { initialTab?: AgentTabId }) {
  const [current, setCurrent] = useState<AgentTabId>(initialTab);
  return (
    <Tabs
      items={[...AGENT_TABS]}
      value={current}
      onValueChange={(id) => setCurrent(id as AgentTabId)}
      label="Agents and styles sections"
    >
      {current === "agents" ? <AgentsTab /> : <StylesTab />}
    </Tabs>
  );
}
