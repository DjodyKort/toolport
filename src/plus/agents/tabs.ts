export const AGENT_TABS = [
  { id: "agents", label: "Agents" },
  { id: "styles", label: "Styles" },
] as const;

export type AgentTabId = (typeof AGENT_TABS)[number]["id"];
