import type { CcUpdateData } from "../types/cc";
import type { AdapterKnob, PluginRow } from "../types/plugins";
import type { PlanV1 } from "../ui";
import type { Gate, WriteSpec } from "../skills/hooks";
import { plural } from "../skills/model";

const HOME = /^(\/fixture\/home|\/home\/[^/]+|\/Users\/[^/]+)(?=\/|$)/;

/** A path with the home folder written as `~`. */
export const homeShort = (path: string): string => path.replace(HOME, "~");

export const tokenText = (tokens: { value: number } | null): string =>
  tokens === null ? "not available" : `${tokens.value.toLocaleString("en-US")} tokens`;

export const UPDATE_LABEL: Record<string, string> = {
  current: "up to date",
  update: "update available",
  unknown: "update state unknown",
  blocked: "update blocked",
};

export const updateText = (row: Pick<PluginRow, "update">): string => {
  const label = UPDATE_LABEL[row.update.state] ?? row.update.state;
  return row.update.state === "update" && row.update.available
    ? `${label}: ${row.update.available}`
    : label;
};

export type Scope = "user" | "project" | "local";

export const SCOPE_NAME: Record<Scope, string> = {
  user: "all your folders",
  project: "this project (shared settings)",
  local: "this folder only",
};

export function whereText(
  enabled: PluginRow["enabled"],
  scope: Scope,
): "on" | "off" | "not set" {
  const value = enabled[scope];
  return value === null ? "not set" : value ? "on" : "off";
}

export const KNOB_FROM: Record<AdapterKnob["current"]["from"], string> = {
  "folder-env": "set in this folder",
  "user-env": "set in your settings.json env",
  option: "plugin option",
  default: "default",
};

export const KNOB_WRITTEN: Record<AdapterKnob["kind"], string> = {
  enum: "one of the plugin's own profiles",
  bool: "on or off",
  "bool-off": "on (the key is removed) or off",
  csv: "a comma separated list",
  globs: "folder or file patterns, comma separated",
  "hook-ids": "hook ids the plugin defines",
};

/** `null` takes the key out of the folder (`--unset`), a string writes it (`--set`). A knob
 * that is not in the draft is left as it is. */
export type Draft = Record<string, string | null>;

export function draftArgs(draft: Draft, knobs: AdapterKnob[]): string[] {
  const keys = knobs.map((knob) => knob.key).filter((key) => key in draft);
  return [
    ...keys
      .filter((key) => draft[key] !== null)
      .flatMap((key) => ["--set", `${key}=${draft[key]}`]),
    ...keys.filter((key) => draft[key] === null).flatMap((key) => ["--unset", key]),
  ];
}

/** The value a draft entry stands for in the form: bool-off turns on by removing the key. */
export function pick(knob: AdapterKnob, value: string): string | null {
  return knob.kind === "bool-off" && value === "on" ? null : value;
}

export const folderKnobs = (knobs: AdapterKnob[]): AdapterKnob[] =>
  knobs.filter((knob) => knob.current.from === "folder-env");

export const undoArgs = (knobs: AdapterKnob[]): string[] =>
  folderKnobs(knobs).flatMap((knob) => ["--unset", knob.key]);

const controlResult = (raw: unknown, summary: string): PlanV1 | null => {
  const data = raw as {
    plan?: PlanV1;
    result?: { changed: string[]; undo: string } | null;
  } | null;
  if (!data?.plan) return null;
  return {
    ...data.plan,
    summary,
    steps: [
      ...data.plan.steps,
      ...(data.result?.changed ?? []).map((path): PlanV1["steps"][number] => ({
        op: "update",
        path,
        detail: "Written",
      })),
    ],
    undo: data.result?.undo ?? data.plan.undo,
  };
};

function controlView(done: string) {
  return (raw: unknown, isDone: boolean): unknown => {
    if (!isDone) return null;
    const plan = controlResult(raw, done);
    return plan ? { plan } : null;
  };
}

export function configSpec(
  id: string,
  cwd: string,
  args: string[],
  title: string,
): WriteSpec {
  return {
    command: "plugins config",
    title,
    argv: ["plugins", "config", id, "--cwd", cwd, ...args],
    confirmLabel: "Apply",
    phrase: "apply",
    view: controlView("Plugin settings written"),
  };
}

export function mcpSpec(
  action: "deny" | "allow",
  id: string,
  server: string,
  cwd: string,
): WriteSpec {
  return {
    command: "plugins mcp",
    title: action === "deny" ? `Deny ${server} in this folder` : `Allow ${server} again`,
    argv: ["plugins", "mcp", action, id, server, "--cwd", cwd],
    confirmLabel: action === "deny" ? "Deny" : "Allow again",
    phrase: action,
    view: controlView(action === "deny" ? "Server denied" : "Server allowed again"),
  };
}

const UNDO_NOTE =
  "Toolport does not roll a plugin back. Claude Code keeps what it installed; reinstall an older version with claude plugin if you need it.";

/** `cc update` answers a list of plugins, not a plan: this words it as the plan the dialogs show. */
export function ccPlan(raw: unknown, done: boolean): PlanV1 {
  const data = raw as CcUpdateData;
  const rows = data.plugins;
  const warnings = [
    ...(data.refreshError
      ? [`The marketplaces could not be refreshed: ${data.refreshError}`]
      : []),
    ...(done && data.restartRequired
      ? ["Restart Claude Code for the new versions to load."]
      : []),
  ];
  const step = (row: CcUpdateData["plugins"][number]): PlanV1["steps"][number] => ({
    op: "exec",
    detail: done
      ? `${row.id}: ${row.outcome ?? row.status}${row.error ? ` (${row.error})` : ""}`
      : `${row.id}: ${row.installed}${row.available ? ` to ${row.available}` : ", Claude Code picks the newest"}`,
  });
  return {
    summary: done
      ? `Updated ${plural(rows.length, "plugin")}`
      : `Update ${plural(rows.length, "plugin")}`,
    steps: rows.map(step),
    effects: {},
    warnings,
    undo: UNDO_NOTE,
  };
}

const ccView = (raw: unknown, done: boolean): unknown => ({ plan: ccPlan(raw, done) });

function ccGate(raw: unknown): Gate | null {
  const rows = (raw as CcUpdateData).plugins;
  return rows.length === 0 || rows.every((row) => row.status === "current")
    ? { reason: "Every plugin is up to date: there is nothing to update.", info: true }
    : null;
}

export function updateSpec(name?: string): WriteSpec {
  return {
    command: "cc update",
    title: name ? `Update ${name}` : "Update all plugins",
    argv: ["cc", "update", ...(name ? [name] : [])],
    confirmLabel: "Update",
    phrase: "update",
    gate: ccGate,
    view: ccView,
  };
}

/** The command that turns a plugin off in a folder (local) or for everyone (user). Claude Code
 * owns it: no Toolport command writes `enabledPlugins` for one plugin yet. */
export function disableLine(id: string, scope: "local" | "user", cwd?: string): string {
  const base = `claude plugin disable ${id} --scope ${scope}`;
  return scope === "local" && cwd ? `cd ${cwd} && ${base}` : base;
}

export const cwdArgs = (cwd: string): string[] => (cwd ? ["--cwd", cwd] : []);
