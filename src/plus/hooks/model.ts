import type { HookEntry, HooksLsData } from "../types/plugins";
import { plural } from "../skills/model";

export const TOOLS = ["Bash", "Edit", "Write", "Read"] as const;
export type Tool = (typeof TOOLS)[number];

export const SWITCH_LABEL: Record<string, string> = {
  "plugin-setting": "plugin setting",
  "disable-plugin": "turn the plugin off",
  "edit-settings": "edit settings.json",
  "skills-sync": "skills sync",
  "toolport-command": "Toolport command",
  none: "no switch",
};

export const ownerLabel = (hook: HookEntry): string =>
  `${hook.owner.kind}: ${hook.owner.name}`;

/** Claude Code starts every hook whose matcher fits, in parallel; these are the two events
 * that fire around a tool call. */
export function around(data: HooksLsData, tool: Tool) {
  const fits = data.hooks.filter((hook) => hook.tools.includes(tool));
  return {
    before: fits.filter((hook) => hook.event === "PreToolUse"),
    after: fits.filter((hook) => hook.event === "PostToolUse"),
  };
}

/** Per owner, how many of these hooks it has. */
export function byOwner(hooks: HookEntry[]): string {
  const counts = new Map<string, number>();
  for (const hook of hooks) {
    const key = ownerLabel(hook);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return (
    [...counts].map(([owner, n]) => `${owner} ${n}`).join(" · ") || "no hook matches"
  );
}

export function otherTotal(data: HooksLsData): number {
  return Object.values(data.counts.otherEvents).reduce((sum, n) => sum + n, 0);
}

export function otherByOwner(data: HooksLsData): string {
  const rows = Object.entries(data.counts.otherEventsByOwner).map(
    ([owner, events]) =>
      `${owner.replace(":", ": ")} ${Object.values(events).reduce((s, n) => s + n, 0)}`,
  );
  return rows.length > 0 ? rows.join(" · ") : "none";
}

/** What the person can do about the hooks that fire for this tool, by who owns them. */
export function switchText(hooks: HookEntry[]): string {
  const methods = [...new Set(hooks.map((hook) => hook.switch.method))];
  if (methods.length === 0) return "nothing to switch";
  return methods.map((method) => SWITCH_LABEL[method] ?? method).join(", ");
}

export const processesText = (n: number): string => plural(n, "process", "processes");

export const hooksArgs = (cwd: string): string[] => [
  "hooks",
  "ls",
  ...(cwd ? ["--cwd", cwd] : []),
];
