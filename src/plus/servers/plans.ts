import type {
  ProfileCreateData,
  ProfileEditData,
  ProfileRmData,
  ServerUninstallData,
  ClientSyncData,
} from "../bridge/data";
import type { PlanStep, PlanV1 } from "../ui/plan";
import type {
  ClientDirectAddData,
  ClientDirectRmData,
  ClientEditData,
  ClientImportData,
} from "../types/client";
import { commandLine } from "../allcommands/model";
import type { ProfileData, ServerInfo } from "./model";

/** The older commands answer a dry run with their own shapes. These turn each one into the
 * `PlanV1` the plan preview draws, so a write shows what changes in plain sentences. The same
 * function reads the apply answer (`done`) and words it in the past tense. */

const verb = (done: boolean, present: string, past: string) => (done ? past : present);
const list = (items: unknown[]) => items.map(String).join(", ");
const cmd = (...argv: string[]) => commandLine(argv);

function plan(
  summary: string,
  steps: PlanStep[],
  warnings: string[] = [],
  undo = "",
): PlanV1 {
  return { summary, steps, effects: {}, warnings, undo };
}

type Row = Record<string, unknown>;
const rows = (value: unknown): Row[] =>
  Array.isArray(value) ? value.filter((v): v is Row => !!v && typeof v === "object") : [];

export function reAddCommand(info: ServerInfo | null | undefined): string {
  if (!info) return "";
  const argv = ["server", "new", info.name];
  if (info.url) {
    argv.push("--url", info.url);
    if (info.transport === "http" || info.transport === "sse")
      argv.push("--transport", info.transport);
  } else if (info.command) {
    argv.push("--command", info.command);
    for (const arg of info.args) argv.push("--arg", arg);
  } else return "";
  if (info.cwd) argv.push("--cwd", info.cwd);
  return commandLine(argv);
}

export interface UninstallContext {
  info?: ServerInfo | null;
  keepClients?: boolean;
  keepSecrets?: boolean;
}

export function uninstallPlan(
  data: ServerUninstallData,
  done: boolean,
  ctx: UninstallContext = {},
): PlanV1 {
  const steps: PlanStep[] = [
    {
      op: "delete",
      detail: `Server ${data.name} (${data.id}) ${verb(done, "is removed", "was removed")} from the registry`,
    },
  ];
  const warnings: string[] = [];
  for (const row of rows(data.clients)) {
    const removed = Array.isArray(row.removed) ? row.removed : [];
    if (row.error) warnings.push(`${row.client}: ${row.error}`);
    if (removed.length === 0) continue;
    steps.push({
      op: "update",
      path: typeof row.path === "string" ? row.path : undefined,
      detail: `${verb(done, "Remove", "Removed")} ${list(removed)} from ${row.client}`,
    });
    if (typeof row.backup === "string") {
      steps.push({
        op: "create",
        path: row.backup,
        detail: "Backup of the client config",
      });
    }
  }
  if (ctx.keepClients) {
    steps.push({ op: "note", detail: "Client entries are kept (--keep-clients)" });
  }
  const secretKeys = done
    ? data.secretsRemoved
    : (ctx.info?.env.filter((env) => env.secret).map((env) => env.key) ?? []);
  if (ctx.keepSecrets) {
    steps.push({ op: "note", detail: "Its secrets stay in the vault (--keep-secrets)" });
  } else if (secretKeys.length > 0) {
    steps.push({
      op: "delete",
      detail: `${verb(done, "Remove", "Removed")} secrets from the vault: ${list(secretKeys)}`,
    });
  }
  return plan(
    `${verb(done, "Remove", "Removed")} the server ${data.name}`,
    steps,
    warnings,
    reAddCommand(ctx.info),
  );
}

export function profileCreatePlan(data: ProfileCreateData, done: boolean): PlanV1 {
  if (!data.created) {
    return plan(`Profile ${data.name} already exists`, [
      { op: "note", detail: "Nothing is created, it is kept as it is" },
    ]);
  }
  return plan(
    `${verb(done, "Create", "Created")} the profile ${data.name}`,
    [
      {
        op: "create",
        detail: `Profile ${data.name} (${data.id}) with no servers`,
      },
    ],
    [],
    cmd("profile", "rm", data.id),
  );
}

export interface ProfileEditContext {
  /** Clients that use the profile, by name, for the impact line. */
  users?: string[];
}

export function profileEditPlan(
  data: ProfileEditData,
  done: boolean,
  ctx: ProfileEditContext = {},
): PlanV1 {
  const steps: PlanStep[] = [];
  if (data.renamed) {
    steps.push({
      op: "update",
      detail: `${verb(done, "Rename", "Renamed")} the profile from ${data.oldName} to ${data.name}`,
    });
  }
  if (data.servers.added.length > 0) {
    steps.push({
      op: "create",
      detail: `${verb(done, "Add", "Added")} ${list(data.servers.added)}`,
    });
  }
  if (data.servers.removed.length > 0) {
    steps.push({
      op: "delete",
      detail: `${verb(done, "Remove", "Removed")} ${list(data.servers.removed)}`,
    });
  }
  const users = ctx.users ?? [];
  if (data.changed && users.length > 0) {
    steps.push({
      op: "note",
      detail: `Used by ${list(users)}: their tool list changes with the next session`,
    });
  }
  const warnings = data.notInProfile.length
    ? [`Not in the profile, so left alone: ${list(data.notInProfile)}`]
    : [];
  const undoArgv = ["profile", "edit", data.id];
  if (data.renamed) undoArgv.push("--name", data.oldName);
  const { added, removed, before } = data.servers;
  if (added.length > 0 && removed.length > 0) {
    undoArgv.push("--set-servers", before.join(","));
  } else if (removed.length > 0) {
    undoArgv.push("--add-server", removed.join(","));
  } else if (added.length > 0) {
    undoArgv.push("--remove-server", added.join(","));
  }
  return plan(
    data.changed
      ? `${verb(done, "Edit", "Edited")} the profile ${data.oldName}`
      : `The profile ${data.name} already matches`,
    steps,
    warnings,
    data.changed ? commandLine(undoArgv) : "",
  );
}

export function profileRmPlan(
  data: ProfileRmData,
  done: boolean,
  ctx: { profile?: ProfileData } = {},
): PlanV1 {
  const steps: PlanStep[] = [
    {
      op: "delete",
      detail: `${verb(done, "Delete", "Deleted")} the profile ${data.name} (${data.servers} server${data.servers === 1 ? "" : "s"} stay in the registry)`,
    },
  ];
  const warnings: string[] = [];
  for (const row of rows(data.clients)) {
    if (row.error) warnings.push(`${row.name}: ${row.error}`);
    else {
      steps.push({
        op: "update",
        detail: `${verb(done, "Remove", "Removed")} the toolport entry scoped to ${data.name} from ${row.name}`,
      });
    }
    if (typeof row.backup === "string") {
      steps.push({
        op: "create",
        path: row.backup,
        detail: "Backup of the client config",
      });
    }
  }
  const left = Array.isArray(data.left) ? data.left : [];
  if (left.length > 0) {
    warnings.push(
      left.length === 1
        ? `${list(left)} still points at this profile and keeps its entry`
        : `${list(left)} still point at this profile and keep their entries`,
    );
  }
  const members = ctx.profile?.servers.map((server) => server.name) ?? [];
  const undo =
    members.length > 0
      ? `${cmd("profile", "create", data.name)} && ${commandLine(["profile", "edit", data.name, "--add-server", members.join(",")])}`
      : cmd("profile", "create", data.name);
  return plan(
    `${verb(done, "Delete", "Deleted")} the profile ${data.name}`,
    steps,
    warnings,
    undo,
  );
}

export function clientEditPlan(data: ClientEditData, done: boolean): PlanV1 {
  const steps: PlanStep[] = [];
  if (data.changed) {
    steps.push({
      op: "update",
      path: data.path,
      detail: `${verb(done, "Point", "Pointed")} ${data.name} at ${list(data.profiles.after)}`,
    });
    if (data.profiles.added.length) {
      steps.push({
        op: "create",
        detail: `Profiles added: ${list(data.profiles.added)}`,
      });
    }
    if (data.profiles.removed.length) {
      steps.push({
        op: "delete",
        detail: `Profiles removed: ${list(data.profiles.removed)}`,
      });
    }
  }
  if (typeof data.backup === "string") {
    steps.push({
      op: "create",
      path: data.backup,
      detail: "Backup of the client config",
    });
  }
  const warnings =
    Array.isArray(data.notInClient) && data.notInClient.length
      ? [`Not in the client, so left alone: ${list(data.notInClient)}`]
      : [];
  const before = data.profiles.before.map(String);
  const undo = !data.changed
    ? ""
    : before.length > 0
      ? cmd("client", "edit", data.client, "--set-profiles", before.join(","))
      : cmd(
          "client",
          "edit",
          data.client,
          "--remove-profile",
          data.profiles.after.join(","),
        );
  return plan(
    data.changed
      ? `${verb(done, "Set", "Set")} the profile of ${data.name}`
      : `${data.name} already uses these profiles`,
    steps,
    warnings,
    undo,
  );
}

const GATEWAY_WORDS: Record<string, string> = {
  "would-install": "Add the Toolport entry",
  installed: "Added the Toolport entry",
  customized: "The Toolport entry was edited by hand, it is left as it is",
  "not-installed": "The client is not installed",
};

export function clientSyncPlan(data: ClientSyncData, done: boolean): PlanV1 {
  const steps: PlanStep[] = [];
  const warnings: string[] = [];
  const backups: string[] = [];
  for (const row of rows(data.clients)) {
    const client = String(row.client);
    const gateway = GATEWAY_WORDS[String(row.gateway)];
    if (gateway) {
      steps.push({
        op:
          row.gateway === "would-install" || row.gateway === "installed"
            ? "create"
            : "note",
        detail: `${client}: ${gateway}`,
      });
    }
    for (const removed of rows(row.removed)) {
      steps.push({
        op: "delete",
        detail: `${client}: ${verb(done, "remove", "removed")} the direct entry ${removed.name} (${removed.reason})`,
      });
    }
    const kept = Array.isArray(row.kept) ? row.kept : [];
    if (kept.length) {
      steps.push({
        op: "note",
        detail: `${client}: keeps the orphan entries ${list(kept)}`,
      });
    }
    const direct = Array.isArray(row.direct) ? row.direct : [];
    if (direct.length) {
      steps.push({
        op: "note",
        detail: `${client}: leaves the direct launcher entries ${list(direct)} alone`,
      });
    }
    if (row.error) warnings.push(`${client}: ${row.error}`);
    if (Array.isArray(row.backups)) backups.push(...row.backups.map(String));
  }
  for (const path of backups) {
    steps.push({ op: "create", path, detail: "Backup of a client config" });
  }
  if (steps.length === 0) steps.push({ op: "note", detail: "Every client is in sync" });
  return plan(
    done ? "Synced the managed clients" : "Sync the managed clients",
    steps,
    warnings,
    backups.length ? "Copy the backup files above over the client configs" : "",
  );
}

function skippedRow(entry: unknown): { name: string; why: string } {
  if (Array.isArray(entry))
    return { name: String(entry[0]), why: String(entry[1] ?? "") };
  if (entry && typeof entry === "object") {
    const row = entry as Row;
    return {
      name: String(row.name ?? row.server ?? ""),
      why: String(row.reason ?? row.why ?? ""),
    };
  }
  return { name: String(entry), why: "" };
}

export function clientImportPlan(data: ClientImportData, done: boolean): PlanV1 {
  const steps: PlanStep[] = data.imported.map((server) => ({
    op: "create",
    detail: `${verb(done, "Import", "Imported")} ${server.name} into the registry`,
  }));
  const warnings: string[] = [];
  for (const skipped of data.skipped) {
    const { name, why } = skippedRow(skipped);
    warnings.push(`${name} is skipped${why ? `: ${why}` : ""}`);
  }
  if (data.profile) {
    steps.push({
      op: data.profile.created ? "create" : "update",
      detail: `${data.profile.created ? verb(done, "Create", "Created") : "Use"} the profile ${data.profile.name} with ${data.profile.servers.length} server${data.profile.servers.length === 1 ? "" : "s"}`,
    });
  }
  for (const secret of rows(data.secrets)) {
    steps.push({
      op: "note",
      detail: `Registers the environment key ${secret.key} of ${secret.server}`,
    });
  }
  if (steps.length === 0) steps.push({ op: "note", detail: "Nothing to import" });
  return plan(
    `${verb(done, "Import", "Imported")} ${data.imported.length} server${data.imported.length === 1 ? "" : "s"} from ${data.name}`,
    steps,
    warnings,
    "",
  );
}

export function directAddPlan(data: ClientDirectAddData, done: boolean): PlanV1 {
  const launcher = [data.launcher.command, ...data.launcher.args].join(" ");
  const steps: PlanStep[] = [
    {
      op:
        data.action === "unchanged"
          ? "note"
          : data.action === "added"
            ? "create"
            : "update",
      path: data.path,
      detail: `${verb(done, "Give", "Gave")} ${data.clientName} its own entry ${data.entry} for ${data.serverName}, started by ${launcher}`,
    },
    ...data.notes.map((note): PlanStep => ({ op: "note", detail: note })),
  ];
  if (data.backup)
    steps.push({
      op: "create",
      path: data.backup,
      detail: "Backup of the client config",
    });
  return plan(
    `${verb(done, "Add", "Added")} a direct entry for ${data.serverName} in ${data.clientName}`,
    steps,
    [data.tradeoff],
    cmd("client", "direct", "rm", data.server, "--client", data.client),
  );
}

export function directRmPlan(data: ClientDirectRmData, done: boolean): PlanV1 {
  const steps: PlanStep[] = [
    {
      op: "delete",
      path: data.path,
      detail: `${verb(done, "Remove", "Removed")} the direct entry ${data.entry} of ${data.serverName} from ${data.clientName}`,
    },
    ...data.notes.map((note): PlanStep => ({ op: "note", detail: String(note) })),
  ];
  if (data.backup)
    steps.push({
      op: "create",
      path: data.backup,
      detail: "Backup of the client config",
    });
  return plan(
    `${verb(done, "Remove", "Removed")} a direct entry of ${data.serverName} in ${data.clientName}`,
    steps,
    [],
    cmd("client", "direct", "add", data.server, "--client", data.client),
  );
}

export interface ServerFields {
  name: string;
  kind: "command" | "url";
  command: string;
  args: string[];
  url: string;
  transport: string;
  cwd: string;
}

export function launchOf(fields: ServerFields): string {
  return fields.kind === "url"
    ? fields.url
    : [fields.command, ...fields.args].filter(Boolean).join(" ");
}

/** `server new`, `server install` and `server edit` have no dry run in the policy table, so
 * their plan is worked out here from what the form says. */
export function newServerPlan(fields: ServerFields): PlanV1 {
  const steps: PlanStep[] = [
    {
      op: "create",
      detail: `Add the server ${fields.name}: ${launchOf(fields)}`,
      keys: [`servers.${fields.name}`],
    },
  ];
  if (fields.cwd) steps.push({ op: "note", detail: `Working directory: ${fields.cwd}` });
  steps.push({
    op: "note",
    detail: "It is not in any profile yet; secrets and logins are set after it exists",
  });
  return plan(
    `Add the server ${fields.name}`,
    steps,
    [],
    cmd("server", "uninstall", fields.name),
  );
}

export function newServerResult(
  data: { id: string; name: string },
  undoName: string,
): PlanV1 {
  return plan(
    `Added the server ${data.name}`,
    [{ op: "create", detail: `Server ${data.name} (${data.id}) is in the registry` }],
    [],
    cmd("server", "uninstall", undoName),
  );
}

export interface CatalogRow {
  name: string;
  source: string;
  transport: string;
  command: string | null;
  args: string[];
  url: string | null;
  envKeys: string[];
}

export function installPlan(entry: CatalogRow): PlanV1 {
  const launch = entry.url ?? [entry.command ?? "", ...entry.args].join(" ").trim();
  const steps: PlanStep[] = [
    {
      op: "create",
      detail: `Add ${entry.name} from the ${entry.source === "catalog" ? "" : `${entry.source} `}catalog: ${launch}`,
      keys: [`servers.${entry.name}`],
    },
  ];
  if (entry.envKeys.length > 0) {
    steps.push({
      op: "note",
      detail: `It needs ${list(entry.envKeys)}; set them after installing`,
    });
  }
  return plan(
    `Install ${entry.name}`,
    steps,
    entry.transport === "stdio"
      ? ["It starts a program on this computer: " + launch]
      : [],
    cmd("server", "uninstall", entry.name),
  );
}

export interface EditChange {
  field: string;
  before: string;
  after: string;
}

export function editServerPlan(
  name: string,
  id: string,
  changes: EditChange[],
  undoArgv: string[] | null,
): PlanV1 {
  return plan(
    `Edit the server ${name}`,
    changes.map((change): PlanStep => ({
      op: "update",
      detail: `Change ${change.field}`,
      keys: [change.field],
      diff: { before: change.before, after: change.after },
    })),
    [`Clients keep using the new definition from their next session (${id})`],
    undoArgv ? commandLine(undoArgv) : "",
  );
}
