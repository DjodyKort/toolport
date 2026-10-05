import type { LoadItem, LoadsData, MeasureData, MeasureRun } from "../bridge/data";

export const STACK_GROUPS = [
  "Instructions",
  "Skills, commands and agents",
  "Plugins",
  "Tools (MCP)",
  "Memory",
  "Settings",
  "Loads on demand",
  "Not loaded here",
] as const;
export type StackGroup = (typeof STACK_GROUPS)[number];

const BY_KIND: Record<LoadItem["kind"], StackGroup> = {
  memory: "Instructions",
  import: "Instructions",
  rule: "Instructions",
  skill: "Skills, commands and agents",
  command: "Skills, commands and agents",
  agent: "Skills, commands and agents",
  plugin: "Plugins",
  mcp: "Tools (MCP)",
  "memory-index": "Memory",
  settings: "Settings",
};

export function groupOf(item: LoadItem): StackGroup {
  if (item.lazy) return "Loads on demand";
  if (!item.loaded) return "Not loaded here";
  return BY_KIND[item.kind];
}

export interface StackGroupView {
  group: StackGroup;
  items: LoadItem[];
  tokens: number;
}

/** The rows of `context loads` as the stack a person reads: the groups in load order, biggest
 * row first inside a group. Rows that load on demand or never are counted apart, so the
 * numbers of the first groups add up to what loads at the start. */
export function stackGroups(data: LoadsData): StackGroupView[] {
  return STACK_GROUPS.flatMap((group) => {
    const items = data.items
      .filter((item) => groupOf(item) === group)
      .sort((a, b) => b.tokens - a.tokens);
    return items.length === 0
      ? []
      : [{ group, items, tokens: items.reduce((sum, item) => sum + item.tokens, 0) }];
  });
}

export function biggestPlugin(data: LoadsData): LoadItem | null {
  const plugins = data.items.filter((item) => item.kind === "plugin" && item.loaded);
  return plugins.sort((a, b) => b.tokens - a.tokens)[0] ?? null;
}

export function basisWord(basis: string): string {
  return basis === "measured"
    ? "measured"
    : basis === "projected"
      ? "projected"
      : "estimate";
}

export function tokenLabel(tokens: number, basis: string): string {
  return `${tokens.toLocaleString("en")} tokens, ${basisWord(basis)}`;
}

/** The loaded plugin names a measurement can switch off: `--without plugin:<id>`. */
export function withoutSpec(item: LoadItem): string {
  return `plugin:${item.name}`;
}

export interface MeasuredView {
  asIs: MeasureRun | null;
  deltas: MeasureData["deltas"];
  invisible: MeasureData["invisibleSkills"];
}

export function measuredView(data: MeasureData): MeasuredView {
  return {
    asIs: data.runs.find((run) => run.label === "as is") ?? data.runs[0] ?? null,
    deltas: data.deltas,
    invisible: data.invisibleSkills,
  };
}

export function skillBudgetState(data: LoadsData): {
  used: number;
  limit: number;
  capped: string[];
  over: boolean;
} {
  const budget = data.skill_budget;
  return {
    used: budget.used_tokens,
    limit: budget.limit_tokens,
    capped: budget.capped,
    over: budget.capped.length > 0,
  };
}
