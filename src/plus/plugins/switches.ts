import type { PlanV1 } from "../ui";
import type { Gate, WriteSpec } from "../skills/hooks";
import type { PluginsFolderSwitchData, PluginsUserSwitchData } from "../types/plugins";

type SwitchData = PluginsFolderSwitchData | PluginsUserSwitchData;

const RESTART = /^Claude Code reads plugin settings/;

/** The plan of a switch as the dialogs show it: a conflict that no warning names becomes its own
 * line, and an applied switch adds what it changed and the command that puts it back. */
export function switchPlan(raw: unknown, done: boolean, summary: string): PlanV1 | null {
  const data = raw as SwitchData | null;
  if (!data?.plan) return null;
  const named = data.plan.warnings.join("\n");
  const conflicts = data.conflicts
    .filter((key) => !named.includes(key))
    .map((key): PlanV1["steps"][number] => ({
      op: "note",
      detail: "Conflict, left as it is because it changed since Toolport wrote it",
      keys: [key],
    }));
  const changed = done ? (data.result?.changed ?? []) : [];
  return {
    ...data.plan,
    summary: done ? summary : data.plan.summary,
    steps: [
      ...data.plan.steps,
      ...conflicts,
      ...changed.map((what): PlanV1["steps"][number] =>
        data.scope === "user"
          ? { op: "exec", detail: `Ran ${what}` }
          : { op: "update", path: what, detail: "Written" },
      ),
    ],
    undo: data.result?.undo ?? data.plan.undo,
  };
}

function switchView(done: string) {
  return (raw: unknown, isDone: boolean): unknown => {
    const plan = switchPlan(raw, isDone, done);
    return plan ? { plan } : null;
  };
}

/** A folder switch that would write nothing is not offered for a confirm: the reason is the
 * CLI's own warning ("already turned off by `plugins off`", "Toolport did not write it"). */
function switchGate(raw: unknown): Gate | null {
  const data = raw as SwitchData | null;
  if (!data || data.scope !== "folder") return null;
  if (data.ledger !== null || data.changes.some((change) => change.action !== "none"))
    return null;
  const why = data.plan.warnings.find((warning) => !RESTART.test(warning));
  return {
    reason: why ? `Nothing to do: ${why}` : "Nothing to change in this folder.",
    info: true,
  };
}

/** `plugins off`: `enabledPlugins.<id> = false` in the folder's settings.local.json, through the
 * controls ledger, so `plugins on` puts back exactly what was there. */
export function offSpec(id: string, name: string, cwd: string): WriteSpec {
  return {
    command: "plugins off",
    title: `Turn ${name} off in this folder`,
    argv: ["plugins", "off", id, "--cwd", cwd],
    confirmLabel: "Turn off",
    phrase: "off",
    gate: switchGate,
    view: switchView(`Turned ${name} off in ${cwd}`),
  };
}

export function onSpec(id: string, name: string, cwd: string): WriteSpec {
  return {
    command: "plugins on",
    title: `Turn ${name} back on in this folder`,
    argv: ["plugins", "on", id, "--cwd", cwd],
    confirmLabel: "Turn on",
    phrase: "on",
    gate: switchGate,
    view: switchView(`Turned ${name} back on in ${cwd}`),
  };
}

/** `plugins disable` runs `claude plugin disable <id> --scope user` for every project: the
 * destructive tier, so the confirmation asks for the plugin id (D-081). */
export function disableSpec(id: string, name: string): WriteSpec {
  return {
    command: "plugins disable",
    title: `Disable ${name} everywhere`,
    argv: ["plugins", "disable", id],
    confirmLabel: "Disable",
    phrase: id,
    view: switchView(`Disabled ${name} everywhere`),
  };
}

export function enableSpec(id: string, name: string): WriteSpec {
  return {
    command: "plugins enable",
    title: `Enable ${name} everywhere`,
    argv: ["plugins", "enable", id],
    confirmLabel: "Enable",
    phrase: "enable",
    view: switchView(`Enabled ${name} everywhere`),
  };
}
