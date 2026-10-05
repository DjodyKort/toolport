import type { GuiEntry, GuiParityManifest } from "./guiParity";

/** The part of `toolportctl commands --json` the parity check needs. */
export interface RegistrySnapshot {
  commands: Array<{ id: string; kind: string; path: string[]; surface: string | null }>;
  tools: Array<{ name: string; command: string | null }>;
}

export interface RepoFiles {
  exists(path: string): boolean;
  text(path: string): string;
}

export interface ParityReport {
  errors: string[];
  pendingCommands: string[];
  pendingTools: string[];
}

const STATUSES = ["planned", "built"];
const SURFACES = ["screen", "terminal"];

function paste(id: string, entry: GuiEntry) {
  return `"${id}": ${JSON.stringify(entry)}`;
}

const HINT = "map it to a built route and action";

/** Every rule of R3 that a test can prove. All problems are reported, not only the first. */
export function checkParity(
  manifest: GuiParityManifest,
  registry: RegistrySnapshot,
  files: RepoFiles,
): ParityReport {
  const errors: string[] = [];
  const fail = (message: string) => errors.push(message);

  if (manifest.schemaVersion !== 1)
    fail(`schemaVersion is ${manifest.schemaVersion}, not 1`);

  const commands = registry.commands.filter((c) => c.kind === "command");
  const surfaceOf = new Map(commands.map((c) => [c.id, c.surface ?? "screen"]));
  const toolCommand = new Map(registry.tools.map((t) => [t.name, t.command]));

  for (const [id, surface] of surfaceOf) {
    if (!(id in manifest.commands)) {
      fail(
        `command \`${id}\` has no entry in commands; ${HINT}, e.g. ${paste(id, { route: "<route>", action: "<route>.<action>", surface: surface as GuiEntry["surface"] })}`,
      );
    }
  }
  for (const id of Object.keys(manifest.commands)) {
    if (!surfaceOf.has(id))
      fail(`commands entry \`${id}\` is not a command of the registry`);
  }
  for (const name of toolCommand.keys()) {
    if (!(name in manifest.tools)) {
      fail(
        `tool \`${name}\` has no entry in tools; ${HINT}, e.g. ${paste(name, { route: "<route>", action: "<route>.<action>", surface: "screen" })}`,
      );
    }
  }
  for (const name of Object.keys(manifest.tools)) {
    if (!toolCommand.has(name))
      fail(`tools entry \`${name}\` is not a tool of the catalog`);
  }

  for (const group of new Set(commands.map((c) => c.path[0]))) {
    if (!manifest.owners[group])
      fail(`command group \`${group}\` has no owner item in owners`);
  }

  const referenced = new Set<string>();
  const checkEntry = (kind: string, id: string, entry: GuiEntry) => {
    const where = `${kind} \`${id}\``;
    if (!SURFACES.includes(entry.surface)) {
      fail(`${where}: surface \`${entry.surface}\` is neither screen nor terminal`);
    }
    const route = manifest.routes[entry.route];
    if (!route) fail(`${where}: route \`${entry.route}\` is not in routes`);
    const action = manifest.actions[entry.action];
    if (!action) {
      fail(`${where}: action \`${entry.action}\` is not in actions`);
      return;
    }
    referenced.add(entry.action);
    if (action.route !== entry.route) {
      fail(
        `${where}: action \`${entry.action}\` belongs to route \`${action.route}\`, not \`${entry.route}\``,
      );
    }
  };
  for (const [id, entry] of Object.entries(manifest.commands)) {
    checkEntry("command", id, entry);
    const registered = surfaceOf.get(id);
    if (registered && registered !== entry.surface) {
      fail(
        `command \`${id}\`: surface is ${entry.surface} but the registry says ${registered}`,
      );
    }
  }
  for (const [name, entry] of Object.entries(manifest.tools)) {
    checkEntry("tool", name, entry);
    const mapped = toolCommand.get(name);
    const expected = mapped ? (surfaceOf.get(mapped) ?? "screen") : "screen";
    if (toolCommand.has(name) && entry.surface !== expected) {
      fail(
        `tool \`${name}\`: surface is ${entry.surface} but ${mapped ?? "a tool"} is ${expected}`,
      );
    }
  }

  for (const [id, route] of Object.entries(manifest.routes)) {
    if (!STATUSES.includes(route.status))
      fail(`route \`${id}\`: unknown status \`${route.status}\``);
    if (route.status === "built") {
      if (!route.component) fail(`route \`${id}\` is built but names no component`);
      else if (!files.exists(route.component)) {
        fail(`route \`${id}\`: component ${route.component} does not exist`);
      }
    }
    if (!Object.values(manifest.actions).some((a) => a.route === id)) {
      fail(`route \`${id}\` has no action`);
    }
  }
  for (const [id, action] of Object.entries(manifest.actions)) {
    if (!STATUSES.includes(action.status))
      fail(`action \`${id}\`: unknown status \`${action.status}\``);
    if (!manifest.routes[action.route])
      fail(`action \`${id}\`: route \`${action.route}\` is not in routes`);
    if (!referenced.has(id)) fail(`action \`${id}\` is not used by any command or tool`);
    if (action.status !== "built") continue;
    if (manifest.routes[action.route]?.status !== "built") {
      fail(`action \`${id}\` is built but its route \`${action.route}\` is not`);
    }
    if (!action.test) fail(`action \`${id}\` is built but has no component test`);
    else if (!files.exists(action.test))
      fail(`action \`${id}\`: test ${action.test} does not exist`);
    else if (!files.text(action.test).includes(id)) {
      fail(`action \`${id}\`: test ${action.test} never names the action`);
    }
  }

  const pending = (entry: GuiEntry | undefined) =>
    !entry || manifest.actions[entry.action]?.status !== "built";
  return {
    errors,
    pendingCommands: [...surfaceOf.keys()]
      .filter((id) => pending(manifest.commands[id]))
      .sort(),
    pendingTools: [...toolCommand.keys()]
      .filter((id) => pending(manifest.tools[id]))
      .sort(),
  };
}

/** The pending rows grouped by the item that owns the group, for the printed list. */
export function pendingSummary(
  manifest: GuiParityManifest,
  report: ParityReport,
): string[] {
  const byOwner = new Map<string, number>();
  for (const id of report.pendingCommands) {
    const owner = manifest.owners[id.split(" ")[0]] ?? "unowned";
    byOwner.set(owner, (byOwner.get(owner) ?? 0) + 1);
  }
  return [...byOwner].sort().map(([owner, count]) => `${owner}: ${count}`);
}

export interface ParitySummary {
  commands: number;
  tools: number;
  commandRows: number;
  toolRows: number;
  actionsBuilt: number;
  pending: number;
  waivers: number;
}

/** The counts of the parity gate: the registry on one side, the manifest on the other. A waiver is
 * a row that is neither on a screen nor a terminal-only command. */
export function paritySummary(
  manifest: GuiParityManifest,
  registry: RegistrySnapshot,
  report: ParityReport,
): ParitySummary {
  const rows = [...Object.values(manifest.commands), ...Object.values(manifest.tools)];
  return {
    commands: registry.commands.filter((c) => c.kind === "command").length,
    tools: registry.tools.length,
    commandRows: Object.keys(manifest.commands).length,
    toolRows: Object.keys(manifest.tools).length,
    actionsBuilt: Object.values(manifest.actions).filter((a) => a.status === "built")
      .length,
    pending: report.pendingCommands.length + report.pendingTools.length,
    waivers: rows.filter((entry) => !SURFACES.includes(entry.surface)).length,
  };
}

export function summaryLine(s: ParitySummary): string {
  return (
    `gui parity: ${s.commands} commands, ${s.tools} tools, ${s.commandRows + s.toolRows} manifest rows, ` +
    `${s.actionsBuilt} screen actions built, ${s.pending} pending, ${s.waivers} waivers`
  );
}
