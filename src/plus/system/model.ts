import type { CommandRow } from "../bridge/data";
import type { PlanOp, PlanStep, PlanV1 } from "../ui";
import type { Tier } from "../allcommands/model";

type Data = Record<string, unknown>;

export const plural = (n: number, one: string, many = `${one}s`) =>
  `${n} ${n === 1 ? one : many}`;

const record = (value: unknown): Data | null =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Data)
    : null;
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const str = (value: unknown): string => String(value ?? "");

/** One entry of a list the commands print as `any`: a path, or an object that names one. */
export function itemText(value: unknown): string {
  if (typeof value === "string") return value;
  const row = record(value);
  if (!row) return String(value);
  for (const key of ["path", "file", "name", "id", "reference"]) {
    if (typeof row[key] === "string") return row[key];
  }
  return JSON.stringify(value);
}

export interface Policy {
  tier: Tier;
  previewFlag: string | null;
  terminal: boolean;
}

/** What the registry says about a command; one it does not have, or marks as planned, has no
 * policy and the screen does not run it. */
export function policyOf(rows: CommandRow[] | null, id: string): Policy | null {
  const row = rows?.find((c) => c.kind === "command" && c.id === id);
  if (!row || row.planned || !row.tier) return null;
  return {
    tier: row.tier,
    previewFlag: row.preview?.mode === "flag" ? row.preview.flag : null,
    terminal: row.surface === "terminal" || row.needs.includes("terminal-only"),
  };
}

/** `a b  c` to `["a", "b", "c"]`; one entry per line or per blank. */
export function words(text: string): string[] {
  return text.split(/\s+/).filter(Boolean);
}

export function lines(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
}

export function optionFlag(flag: string, value: string): string[] {
  const text = value.trim();
  return text ? [flag, text] : [];
}

export function repoProblem(value: string): string | null {
  const text = value.trim();
  if (text === "") return "Give the git repository that holds the bundle";
  return null;
}

export function ownerRepoProblem(value: string): string | null {
  const text = value.trim();
  if (text === "") return null;
  return /^[\w.-]+\/[\w.-]+$/.test(text) ? null : "Use the form owner/repo";
}

// ---- sync ------------------------------------------------------------------------------

export interface SyncChanges {
  new: unknown[];
  modified: unknown[];
  removed: unknown[];
  conflicts: unknown[];
  unchanged: unknown[];
}

export function changesOf(value: unknown): SyncChanges | null {
  const row = record(value);
  if (!row) return null;
  return {
    new: list(row.new),
    modified: list(row.modified),
    removed: list(row.removed),
    conflicts: list(row.conflicts),
    unchanged: list(row.unchanged),
  };
}

export function changeCount(changes: SyncChanges | null): number {
  return changes
    ? changes.new.length +
        changes.modified.length +
        changes.removed.length +
        changes.conflicts.length
    : 0;
}

const NEVER_IN_BUNDLE = "Keychain keys, tokens and secret values are never in the bundle";

function changeSteps(changes: SyncChanges | null): PlanStep[] {
  if (!changes) return [];
  return [
    ...changes.new.map((x): PlanStep => ({
      op: "create",
      path: itemText(x),
      detail: "New",
    })),
    ...changes.modified.map((x): PlanStep => ({
      op: "update",
      path: itemText(x),
      detail: "Changed",
    })),
    ...changes.removed.map((x): PlanStep => ({
      op: "delete",
      path: itemText(x),
      detail: "Removed",
    })),
  ];
}

export function planOfSyncPush(data: Data, done: boolean): PlanV1 {
  const entries = list(data.entries).map(itemText);
  const warnings: string[] = [];
  if (done && data.committed === true && data.pushed !== true) {
    warnings.push("Committed in the sync repository but not pushed to the remote");
  }
  return {
    summary:
      entries.length === 0
        ? "Nothing to push"
        : `${done ? "Pushed" : "Push"} ${plural(entries.length, "file")} from ${str(data.machineId)}`,
    steps: [
      ...entries.map((path): PlanStep => ({
        op: "update",
        path,
        detail: "Encrypted into the bundle",
      })),
      { op: "note", detail: NEVER_IN_BUNDLE },
    ],
    effects: {},
    warnings,
    undo: "",
  };
}

export function planOfSyncPull(data: Data, done: boolean): PlanV1 {
  const changes = changesOf(data.changes);
  const conflicts = changes?.conflicts ?? list(data.conflicts);
  const applied = list(data.applied).map(itemText);
  const warnings = conflicts.map((x) => `Conflict: ${itemText(x)}`);
  if (data.noRemote === true) warnings.push("The remote has no bundle yet; push first");
  const kept = list(data.keptLocal).map(itemText);
  const skipped = list(data.skipped).map(itemText);
  const steps: PlanStep[] = done
    ? [
        ...applied.map((path): PlanStep => ({
          op: "update",
          path,
          detail: "Applied from the bundle",
        })),
        ...kept.map((path): PlanStep => ({
          op: "note",
          path,
          detail: "Kept the local copy",
        })),
        ...skipped.map((path): PlanStep => ({ op: "note", path, detail: "Skipped" })),
        ...list(data.resolved).map((x): PlanStep => ({
          op: "note",
          path: itemText(x),
          detail: "Conflict resolved",
        })),
      ]
    : changeSteps(changes);
  const count = done ? applied.length : changeCount(changes) - conflicts.length;
  const skills = Number(data.skillsRepoChanged ?? 0);
  if (skills > 0) {
    steps.push({
      op: "note",
      detail: `${plural(skills, "change")} in the skills repository`,
    });
  }
  return {
    summary:
      count === 0 && conflicts.length === 0
        ? done
          ? "Nothing needed to change"
          : "Nothing to pull: this machine matches the bundle"
        : `${done ? "Pulled" : "Pull"} ${plural(count, "file")} from ${str(data.machineId)}`,
    steps,
    effects: {},
    warnings,
    undo: "",
  };
}

export function planOfSyncInit(args: {
  repo: string;
  branch: string;
  machineId: string;
  reconfigure: boolean;
}): PlanV1 {
  return {
    summary: `${args.reconfigure ? "Reconfigure" : "Set up"} encrypted sync with ${args.repo}`,
    steps: [
      {
        op: args.reconfigure ? "update" : "create",
        path: "sync/sync.json",
        detail: `Sync settings${args.branch ? ` for branch ${args.branch}` : ""}${args.machineId ? `, this machine is ${args.machineId}` : ""}`,
      },
      {
        op: "create",
        path: "sync/sync_keyfile",
        detail: "Key derived from the passphrase, kept on this machine",
      },
      {
        op: "create",
        path: "sync/sync_repo",
        detail: "Local clone of the sync repository",
      },
      { op: "note", detail: "The passphrase is sent on stdin and never stored or shown" },
    ],
    effects: {},
    warnings: args.reconfigure ? ["The previous sync setup is replaced"] : [],
    undo: "toolportctl sync reset",
  };
}

export function planOfSyncReset(repoUrl: string | null): PlanV1 {
  return {
    summary: "Remove the remote sync data and the local sync state",
    steps: [
      ...(repoUrl
        ? [{ op: "delete", path: repoUrl, detail: "Remote sync data" } as PlanStep]
        : []),
      {
        op: "delete",
        path: "sync/",
        detail: "Local sync settings, key file and repository clone",
      },
      {
        op: "note",
        detail: "Servers, profiles and skills on this machine are not changed",
      },
    ],
    effects: {},
    warnings: ["Syncing stops until you set it up again with init"],
    undo: "toolportctl sync init --repo <repository> --passphrase-stdin",
  };
}

export function planOfRotate(): PlanV1 {
  return {
    summary: "Re-encrypt the remote bundle under a new passphrase",
    steps: [
      { op: "update", path: "remote bundle", detail: "Every blob is encrypted again" },
      {
        op: "update",
        path: "sync/sync_keyfile",
        detail: "Key derived from the new passphrase",
      },
    ],
    effects: {},
    warnings: ["Other machines must run init again with the new passphrase"],
    undo: "",
  };
}

export function planOfAddProject(args: {
  path: string;
  name: string;
  files: string[];
}): PlanV1 {
  return {
    summary: `Add the project ${args.name} to the sync set`,
    steps: [
      {
        op: "update",
        path: "sync/sync.json",
        detail: `Track ${args.path}${args.files.length ? ` (${args.files.join(", ")})` : ""}`,
      },
    ],
    effects: {},
    warnings: [],
    undo: `toolportctl sync remove-project ${args.name}`,
  };
}

export function planOfRemoveProject(name: string): PlanV1 {
  return {
    summary: `Remove the project ${name} from the sync set`,
    steps: [
      { op: "update", path: "sync/sync.json", detail: `Stop tracking ${name}` },
      { op: "note", detail: "The files on disk and in the bundle are not deleted" },
    ],
    effects: {},
    warnings: [],
    undo: "",
  };
}

export function planOfGitSync(args: {
  repo: string;
  branch: string;
  auto: boolean;
  clear: boolean;
}): PlanV1 {
  if (args.clear) {
    return {
      summary: "Remove the git sync setup",
      steps: [{ op: "delete", path: "skills_repo", detail: "The git sync settings" }],
      effects: {},
      warnings: [],
      undo: "",
    };
  }
  return {
    summary: `Sync the data directory with ${args.repo}${args.branch ? ` (${args.branch})` : ""}`,
    steps: [
      { op: "create", path: "skills_repo", detail: "Clone the repository and pull it" },
      ...(args.auto
        ? [{ op: "note", detail: "Syncs automatically from now on" } as PlanStep]
        : []),
    ],
    effects: {},
    warnings: [],
    undo: "toolportctl sync git-sync --clear",
  };
}

export function planOfMigrate(args: { bundleDir: string; projects: boolean }): PlanV1 {
  return {
    summary: `Import the mcpm sync bundle in ${args.bundleDir}`,
    steps: [
      {
        op: "create",
        path: args.bundleDir,
        detail: `Decrypt and re-encrypt it as a Toolport bundle${args.projects ? ", project files included" : ""}`,
      },
    ],
    effects: {},
    warnings: [],
    undo: "",
  };
}

// ---- updates ---------------------------------------------------------------------------

export interface UpdateServer {
  id: string;
  kind: string;
  status: string;
  message: string;
  detected: boolean;
  current?: string;
  latest?: string;
  behind?: number;
  ahead?: number;
  plan: string[];
  steps: Array<{ name: string; ok: boolean; detail: string }>;
}

export interface UpdateReport {
  mode: string;
  counts: Record<string, number>;
  servers: UpdateServer[];
}

export function updateReport(data: unknown): UpdateReport {
  const row = record(data) ?? {};
  return {
    mode: str(row.mode),
    counts: (record(row.counts) ?? {}) as Record<string, number>,
    servers: list(row.servers).map((entry) => {
      const server = record(entry) ?? {};
      return {
        id: str(server.id),
        kind: str(server.kind),
        status: str(server.status),
        message: str(server.message),
        detected: server.detected === true,
        current: typeof server.current === "string" ? server.current : undefined,
        latest: typeof server.latest === "string" ? server.latest : undefined,
        behind: typeof server.behind === "number" ? server.behind : undefined,
        ahead: typeof server.ahead === "number" ? server.ahead : undefined,
        plan: list(server.plan).map(str),
        steps: list(server.steps).map((step) => {
          const s = record(step) ?? {};
          return { name: str(s.name), ok: s.ok === true, detail: str(s.detail) };
        }),
      };
    }),
  };
}

export const KIND_LABEL: Record<string, string> = {
  git: "git",
  "github-release": "GitHub release",
  release: "GitHub release",
  npx: "npx",
  uvx: "uvx",
  remote: "Remote server",
  unknown: "Unknown source",
};

export const kindLabel = (kind: string) => KIND_LABEL[kind] ?? (kind || "Unknown source");

export type Tone = "success" | "warning" | "destructive" | "secondary" | "info";

export const STATUS: Record<string, { label: string; tone: Tone }> = {
  "up-to-date": { label: "Up to date", tone: "success" },
  "update-available": { label: "Update available", tone: "warning" },
  updated: { label: "Updated", tone: "success" },
  skipped: { label: "Skipped", tone: "secondary" },
  error: { label: "Error", tone: "destructive" },
  auto: { label: "Automatic", tone: "info" },
  configured: { label: "Source stored", tone: "success" },
};

export const statusOf = (status: string) =>
  STATUS[status] ?? { label: status || "Unknown", tone: "secondary" as Tone };

const POST_UPDATE = /^post_update:\s*(.*?)(\s+\(not run without --allow-commands\))?$/;

/** The `post_update` command of a server, read from the plan lines of its report. `held` is
 * true while the report says it is not run without `--allow-commands`. */
export function postUpdateOf(
  server: UpdateServer,
): { command: string; held: boolean } | null {
  for (const line of server.plan) {
    const found = POST_UPDATE.exec(line);
    if (found) return { command: found[1], held: found[2] !== undefined };
  }
  return null;
}

export const canUpdate = (server: UpdateServer) => server.status === "update-available";

const UPDATE_OP: Record<string, PlanOp> = {
  "update-available": "update",
  updated: "update",
  configured: "create",
};

export function planOfUpdate(data: Data, done: boolean): PlanV1 {
  const report = updateReport(data);
  const steps: PlanStep[] = [];
  const warnings: string[] = [];
  for (const server of report.servers) {
    steps.push({
      op: UPDATE_OP[server.status] ?? "note",
      path: server.id,
      detail: `${kindLabel(server.kind)}: ${server.message}`,
    });
    for (const line of server.plan) {
      const hook = line.startsWith("post_update:");
      steps.push({
        op: hook ? "exec" : "note",
        path: server.id,
        detail: hook ? line.replace(/^post_update:/, "Update command:") : line,
      });
    }
    for (const step of server.steps) {
      steps.push({
        op: "note",
        path: server.id,
        detail: `${step.ok ? "Done" : "Failed"}: ${step.name}, ${step.detail}`,
      });
    }
    if (server.status === "error") warnings.push(`${server.id}: ${server.message}`);
  }
  const changed = report.servers.filter((s) => s.status in UPDATE_OP).length;
  return {
    summary:
      report.servers.length === 0
        ? "No servers to update"
        : changed === 0
          ? "Nothing to change"
          : `${done ? "Updated" : "Update"} ${plural(changed, "server")}`,
    steps,
    effects: {},
    warnings,
    undo: "",
  };
}

// ---- council and self-management MCP ---------------------------------------------------

export function planOfCouncilInstall(): PlanV1 {
  return {
    summary: "Register the council server",
    steps: [
      {
        op: "create",
        path: "registry",
        detail: "Add the server council",
        keys: ["servers.council"],
      },
      {
        op: "note",
        detail: "The API key is stored separately in the vault; set it after installing",
      },
    ],
    effects: {},
    warnings: [],
    undo: "toolportctl council uninstall",
  };
}

export function planOfCouncilUninstall(purgeKey: boolean): PlanV1 {
  return {
    summary: "Remove the council server",
    steps: [
      {
        op: "delete",
        path: "registry",
        detail: "Remove the server council",
        keys: ["servers.council"],
      },
      purgeKey
        ? { op: "delete", path: "vault", detail: "Delete the stored API key" }
        : { op: "note", detail: "The stored API key stays in the vault" },
    ],
    effects: {},
    warnings: purgeKey ? ["A deleted key cannot be restored"] : [],
    undo: "toolportctl council install",
  };
}

export function planOfMcpInstall(profile: string): PlanV1 {
  return {
    summary: `Register the self-management server${profile ? ` and enable it in ${profile}` : ""}`,
    steps: [
      {
        op: "create",
        path: "registry",
        detail: "Add the server toolport-plus-self",
        keys: ["servers.toolport-plus-self"],
      },
      {
        op: "update",
        path: profile ? `profile ${profile}` : "profiles",
        detail: profile ? `Enable it in ${profile}` : "Enable it in the active profile",
      },
    ],
    effects: {},
    warnings: [],
    undo: "toolportctl mcp uninstall",
  };
}

export function planOfMcpUninstall(): PlanV1 {
  return {
    summary: "Remove the self-management server and keep it removed",
    steps: [
      {
        op: "delete",
        path: "registry",
        detail: "Remove the server toolport-plus-self",
        keys: ["servers.toolport-plus-self"],
      },
      { op: "note", detail: "It stays removed until you install it again" },
    ],
    effects: {},
    warnings: ["Agents that use it lose the tools to manage Toolport"],
    undo: "toolportctl mcp install",
  };
}

// ---- import ----------------------------------------------------------------------------

const ACTION_OP: Array<[RegExp, PlanOp]> = [
  [/creat/, "create"],
  [/updat|replac/, "update"],
  [/remov|prun|delet/, "delete"],
];

function actionOp(action: string): PlanOp {
  return ACTION_OP.find(([pattern]) => pattern.test(action))?.[1] ?? "note";
}

function importRows(label: string, value: unknown): PlanStep[] {
  return list(value).map((entry): PlanStep => {
    const row = record(entry) ?? {};
    const action = str(row.action);
    const error = typeof row.error === "string" ? `, ${row.error}` : "";
    return {
      op: actionOp(action),
      path: typeof row.path === "string" ? row.path : undefined,
      detail: `${label} ${str(row.id)}: ${action || "no change"}${error}`,
    };
  });
}

export interface RejectRow {
  id: string;
  reason: string;
}

export function planOfImport(data: Data, done: boolean): PlanV1 {
  const servers = list(data.servers).length;
  const profiles = list(data.profiles).length;
  const secrets = list(data.secrets).length;
  const warnings = [
    ...list(data.rejects).map((entry) => {
      const row = record(entry) ?? {};
      return `Not imported, ${str(row.id)}: ${str(row.reason)}`;
    }),
    ...list(data.warnings).map(itemText),
  ];
  return {
    summary: `${done ? "Imported" : "Import"} ${plural(servers, "server")}, ${plural(profiles, "profile")} and ${plural(secrets, "secret")} from mcpm`,
    steps: [
      ...importRows("Server", data.servers),
      ...importRows("Profile", data.profiles),
      ...importRows("Secret", data.secrets),
      ...importRows("Client", data.clients),
      ...importRows("Client scope", data.clientScopes),
      ...importRows("Client discovery", data.clientDiscovery),
      ...importRows("Skills sync", data.skillsSync),
      ...list(data.scripts).map((script): PlanStep => ({
        op: "note",
        detail: `Launch script: ${itemText(script)}`,
      })),
    ],
    effects: {},
    warnings,
    undo: "Restore the cutover backup (see Undo)",
  };
}

export interface RefRow {
  path: string;
  reference: string;
  reason: string;
  rule: string | null;
}

export function refRows(value: unknown): RefRow[] {
  return list(value).map((entry) => {
    const row = record(entry) ?? {};
    return {
      path: str(row.path),
      reference: str(row.reference),
      reason: str(row.reason),
      rule: typeof row.rule === "string" ? row.rule : null,
    };
  });
}

export function planOfRenameRefs(data: Data, done: boolean): PlanV1 {
  const files = list(data.files).map((entry) => {
    const row = record(entry) ?? {};
    return { path: str(row.path), replaced: Number(row.replaced ?? 0) };
  });
  const orphans = refRows(data.orphans);
  const dead = refRows(data.dead);
  const blocking = orphans.filter((o) => o.rule === "ask" || o.rule === "deny");
  return {
    summary: `${done ? "Rewrote" : "Rewrite"} ${plural(Number(data.replaced ?? 0), "reference")} in ${plural(files.length, "file")} (${Number(data.scanned ?? 0)} scanned)`,
    steps: [
      ...files.map((file): PlanStep => ({
        op: "update",
        path: file.path,
        detail: `${plural(file.replaced, "reference")} renamed`,
      })),
      ...orphans.map((o): PlanStep => ({
        op: "note",
        path: o.path,
        detail: `Left as it is: ${o.reference} (${o.reason})`,
      })),
      ...dead.map((o): PlanStep => ({
        op: "note",
        path: o.path,
        detail: `Names a server that is gone: ${o.reference}`,
      })),
    ],
    effects: {},
    warnings: blocking.map(
      (o) => `${o.reference} is in a ${o.rule} rule and was not renamed (${o.path})`,
    ),
    undo: "",
  };
}

export interface NameMap {
  count: number;
  map: Array<[string, string]>;
}

export function nameMapOf(data: unknown): NameMap {
  const row = record(data) ?? {};
  const map = record(row.map) ?? {};
  return {
    count: Number(row.count ?? Object.keys(map).length),
    map: Object.entries(map).map(([from, to]) => [from, str(to)]),
  };
}

// ---- doctor checks and tool catalogues -------------------------------------------------

const CHECK_LABEL: Record<string, string> = {
  registry_entry: "Registry entry",
  launcher_on_path: "Launcher on the path",
  enabled_in_active_profile: "Enabled in the active profile",
  enabled_in_client_profiles: "Enabled in client profiles",
  key_declared_secret: "API key declared as a secret",
  key_in_vault: "API key in the vault",
  command_matches_binary: "Command matches the binary",
  binary_present: "Binary present",
  handshake: "Handshake",
  catalog: "Catalogue",
};

export function checkLabel(name: string): string {
  return (
    CHECK_LABEL[name] ?? name.replace(/_/g, " ").replace(/^./, (c) => c.toUpperCase())
  );
}

export interface Check {
  name: string;
  ok: boolean;
  detail: string;
}

export function checksOf(data: unknown): Check[] {
  return list(record(data)?.checks).map((entry) => {
    const row = record(entry) ?? {};
    return { name: str(row.name), ok: row.ok === true, detail: str(row.detail) };
  });
}

export const TIER_LABEL: Record<number, string> = {
  1: "Read-only",
  2: "Write",
  3: "Confirm",
  4: "Destructive",
};

export const TIER_TONE: Record<number, Tone> = {
  1: "secondary",
  2: "info",
  3: "warning",
  4: "destructive",
};

export const GATE_LABEL: Record<string, string> = {
  none: "No confirmation",
  always: "Needs confirmation every time",
  "unless-dry-run": "Needs confirmation unless it is a dry run",
};

export interface ToolEntry {
  name: string;
  description: string;
  tier: number;
  gate: string | null;
}

export function toolEntries(value: unknown): ToolEntry[] {
  return list(value).map((entry) => {
    const row = record(entry) ?? {};
    return {
      name: str(row.name),
      description: str(row.description ?? row.summary),
      tier: Number(row.tier ?? 1),
      gate: typeof row.gate === "string" ? row.gate : null,
    };
  });
}

export function filterTools(
  tools: ToolEntry[],
  text: string,
  tier: number | null,
): ToolEntry[] {
  const needle = text.trim().toLowerCase();
  return tools.filter(
    (tool) =>
      (tier === null || tool.tier === tier) &&
      (needle === "" ||
        tool.name.toLowerCase().includes(needle) ||
        tool.description.toLowerCase().includes(needle)),
  );
}

export const SELF_STATE: Record<string, { label: string; tone: Tone }> = {
  enabled: { label: "Enabled", tone: "success" },
  missing: { label: "Not installed", tone: "destructive" },
  "opted-out": { label: "Turned off", tone: "secondary" },
  "not-enabled": { label: "Installed, not enabled in a profile", tone: "warning" },
  disabled: { label: "Disabled", tone: "warning" },
};

export interface ProfileState {
  id: string;
  enabled: boolean;
  optedOut: boolean;
}

export function profileStates(value: unknown): ProfileState[] {
  const entries = Array.isArray(value) ? value : value ? [value] : [];
  return entries.flatMap((entry) => {
    if (typeof entry === "string") return [{ id: entry, enabled: true, optedOut: false }];
    const row = record(entry);
    return row
      ? [
          {
            id: str(row.id),
            enabled: row.enabled === true,
            optedOut: row.optedOut === true,
          },
        ]
      : [];
  });
}

export const SELF_ID = "toolport-plus-self";

/** The snippet a client that is not scoped to a profile needs. */
export function clientSnippet(command: string): string {
  return JSON.stringify({ mcpServers: { [SELF_ID]: { command } } }, null, 2);
}
