import { plural } from "./model";
import type {
  DeniedWhere,
  HookEntry,
  KnobRow,
  PluginEnabled,
  PluginRow,
  Tokens,
} from "./pluginsData";

/** "12 skills, 2 agents, 4 commands": what a plugin brings, empty kinds left out. */
export function bringsText(brings: PluginRow["brings"]): string {
  const parts: string[] = [];
  if (brings.skills) parts.push(plural(brings.skills, "skill"));
  if (brings.agents) parts.push(plural(brings.agents, "agent"));
  if (brings.commands) parts.push(plural(brings.commands, "command"));
  if (brings.hooks) parts.push(plural(brings.hooks, "hook"));
  if (brings.mcpServers) parts.push(plural(brings.mcpServers, "MCP server"));
  return parts.length ? parts.join(", ") : "nothing";
}

export const tokenText = (tokens: Tokens | null): string | null =>
  tokens ? `${tokens.value.toLocaleString("en-US")} tokens` : null;

/** The layers that say something about a plugin, in the order Claude Code reads them. */
export function layerRows(enabled: PluginEnabled): Array<{
  layer: string;
  state: boolean | null;
}> {
  return [
    { layer: "User settings", state: enabled.user },
    { layer: "Project settings", state: enabled.project },
    { layer: "This folder (local)", state: enabled.local },
    { layer: "Managed by your administrator", state: enabled.managed },
  ];
}

export const stateText = (state: boolean | null): string =>
  state === null ? "not set" : state ? "on" : "off";

export function deniedText(denied: DeniedWhere): string | null {
  const where = (["user", "project", "local", "managed"] as const).filter(
    (scope) => denied[scope],
  );
  return where.length ? `denied in ${where.join(", ")} settings` : null;
}

export const deniedAnywhere = (denied: DeniedWhere) =>
  denied.user || denied.project || denied.local || denied.managed;

export function ownerText(hook: HookEntry): string {
  const { kind, name } = hook.owner;
  return kind === "plugin" || kind === "skill" ? `${kind} ${name}` : kind;
}

const FROM: Record<string, string> = {
  "folder-env": "set in this folder",
  "user-env": "set in your user settings",
  "managed-env": "set by your administrator",
  option: "plugin option",
  default: "default",
};

export const fromText = (from: string): string => FROM[from] ?? from;

export function knobValue(knob: KnobRow): string {
  const value = knob.current.value;
  return value === null || value === "" ? "not set" : value;
}

/** `--set` takes a comma list for the list kinds and the words the knob's kind knows otherwise. */
export function knobChoices(knob: KnobRow): string[] | null {
  switch (knob.kind) {
    case "enum":
      return knob.choices;
    case "bool":
      return ["true", "false"];
    case "bool-off":
      return ["on", "off"];
    default:
      return null;
  }
}

export function updateText(plugin: PluginRow): string {
  switch (plugin.update.state) {
    case "update":
      return `Update to ${plugin.update.available}`;
    case "blocked":
      return `Update to ${plugin.update.available} is blocked`;
    case "current":
      return "Up to date";
    default:
      return "Update state unknown";
  }
}
