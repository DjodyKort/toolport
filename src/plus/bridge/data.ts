import {
  arr,
  bool,
  any,
  lit,
  nullable,
  num,
  obj,
  opt,
  rec,
  str,
  type Infer,
  type Shape,
} from "./shape";

/** The `data` of a `toolportctl --json` envelope, per command (D-058). Each shape is checked
 * against the golden envelopes in `src-tauri/tests/fixtures/ctl-envelopes/` by
 * `data.test.ts`, so a change in the CLI output that this file does not follow fails a test. A
 * field whose real type the synthetic world does not exercise is `any` until a golden does. */

export const errorShape = obj({ code: str, message: str });
export type CtlErrorBody = Infer<typeof errorShape>;

export const envelopeShape = obj({
  ok: bool,
  command: str,
  schemaVersion: num,
  data: opt(any),
  error: opt(errorShape),
});

const authCounts = obj({
  expiring: num,
  misconfigured: num,
  needs_reauth: num,
  ok: num,
  revoked: num,
  unknown: num,
  unreachable: num,
});

const gatewayBuild = obj({
  building: bool,
  pid: num,
  role: str,
  servers: arr(obj({ id: str, state: str, tools: num })),
  serversConnected: num,
  serversFailed: num,
  serversTotal: num,
  startedAtMs: num,
  toolsSoFar: num,
  updatedAtMs: num,
});

export const statusData = obj({
  activeProfile: str,
  auth: obj({ counts: authCounts, servers: arr(any) }),
  dataDir: str,
  gateway: obj({
    build: nullable(gatewayBuild),
    builds: arr(gatewayBuild),
    path: str,
    present: bool,
  }),
  profileCount: num,
  registry: obj({ error: nullable(str), exists: bool, path: str, readable: bool }),
  secretsBackend: str,
  serverCount: num,
  version: str,
});
export type StatusData = Infer<typeof statusData>;

export const doctorData = obj({
  checks: arr(obj({ detail: str, name: str, status: str })),
  healthy: bool,
});
export type DoctorData = Infer<typeof doctorData>;

export const serverLsData = obj({
  activeProfile: str,
  servers: arr(obj({ enabled: bool, id: str, name: str, transport: str })),
});
export type ServerLsData = Infer<typeof serverLsData>;

export const serverInfoData = obj({
  id: str,
  name: str,
  transport: str,
  command: nullable(str),
  args: arr(str),
  url: nullable(str),
  cwd: nullable(str),
  source: nullable(any),
  env: arr(obj({ key: str, secret: bool })),
  disabledTools: arr(str),
  declareClientCapabilities: bool,
  forwardInstructions: bool,
  profiles: arr(str),
});
export type ServerInfoData = Infer<typeof serverInfoData>;

export const serverUninstallData = obj({
  clients: arr(any),
  dryRun: bool,
  id: str,
  name: str,
  secretsRemoved: arr(str),
});
export type ServerUninstallData = Infer<typeof serverUninstallData>;

export const profileLsData = obj({
  activeProfile: str,
  profiles: arr(
    obj({
      active: bool,
      clients: arr(any),
      id: str,
      name: str,
      servers: arr(obj({ id: str, name: str, target: str })),
    }),
  ),
});
export type ProfileLsData = Infer<typeof profileLsData>;

export const profileCreateData = obj({ created: bool, dryRun: bool, id: str, name: str });
export type ProfileCreateData = Infer<typeof profileCreateData>;

export const profileEditData = obj({
  changed: bool,
  dryRun: bool,
  id: str,
  name: str,
  notInProfile: arr(str),
  oldName: str,
  renamed: bool,
  servers: obj({ added: arr(str), after: arr(str), before: arr(str), removed: arr(str) }),
});
export type ProfileEditData = Infer<typeof profileEditData>;

export const profileRmData = obj({
  clients: arr(any),
  dryRun: bool,
  id: str,
  left: arr(any),
  name: str,
  servers: num,
});
export type ProfileRmData = Infer<typeof profileRmData>;

export const clientLsData = obj({
  clients: arr(
    obj({
      entries: arr(str),
      gateway: str,
      id: str,
      launchers: arr(any),
      managed: bool,
      name: str,
      path: str,
      scope: nullable(str),
    }),
  ),
});
export type ClientLsData = Infer<typeof clientLsData>;

export const clientSyncData = obj({ clients: arr(any), dryRun: bool });
export type ClientSyncData = Infer<typeof clientSyncData>;

export const secretSetData = obj({ key: str, server: str, stored: bool });
export type SecretSetData = Infer<typeof secretSetData>;

export const secretGetData = obj({ key: str, server: str, set: bool });
export type SecretGetData = Infer<typeof secretGetData>;

const tokenBasis = lit("estimate", "measured", "projected");
export const tokens = obj({ value: num, basis: tokenBasis });
export type Tokens = Infer<typeof tokens>;

export const origin = obj({
  kind: lit(
    "library",
    "org",
    "plugin",
    "account",
    "repo",
    "client",
    "vendored",
    "tap",
    "loose",
    "managed",
    "user",
    "project",
    "inert",
    "remote-library",
  ),
  name: str,
});
export type Origin = Infer<typeof origin>;

export const planV1 = obj({
  summary: str,
  steps: arr(
    obj({
      op: lit("create", "merge", "update", "delete", "exec", "note"),
      path: opt(str),
      detail: str,
      keys: opt(arr(str)),
      diff: opt(obj({ before: str, after: str })),
    }),
  ),
  effects: obj({
    tokens: opt(obj({ before: num, after: num, basis: tokenBasis })),
  }),
  warnings: arr(str),
  undo: str,
});
export type PlanV1 = Infer<typeof planV1>;

export const resultV1 = obj({
  applied: bool,
  changed: arr(str),
  undo: str,
  backups: arr(str),
});
export type ResultV1 = Infer<typeof resultV1>;

export const skillsLsData = obj({
  repo: nullable(str),
  skills: arr(
    obj({
      activation: str,
      description: str,
      invisibleReason: nullable(str),
      name: str,
      origin,
      path: str,
      type: str,
      visible: bool,
      writable: bool,
    }),
  ),
  partial: opt(bool),
  skipped: opt(arr(obj({ detector: str, reason: str }))),
});
export type SkillsLsData = Infer<typeof skillsLsData>;

const sourceKind = lit("skill", "command", "agent", "rule", "memory");

export const sourceItem = obj({
  kind: sourceKind,
  name: str,
  path: str,
  sourceId: str,
  origin,
  writable: bool,
  lazy: bool,
  shadowedBy: nullable(str),
  audit: lit("clean", "warn", "high", "unchecked"),
  tokens,
});
export type SourceItem = Infer<typeof sourceItem>;

export const sourceRow = obj({
  id: str,
  origin,
  detector: lit(
    "library",
    "org",
    "plugin",
    "account",
    "loose",
    "repo",
    "client",
    "vendored",
    "tap",
    "remote-library",
    "inert",
  ),
  root: nullable(str),
  owner: lit("me", "org", "third-party", "anthropic", "project"),
  writable: bool,
  managedBy: nullable(str),
  status: obj({
    state: lit("ok", "stale", "behind", "unreachable", "duplicate", "partial"),
    detail: str,
    checkedAt: str,
  }),
  freshness: nullable(
    obj({
      ref: str,
      behind: num,
      ahead: num,
      inCheckout: bool,
      lastSync: nullable(str),
    }),
  ),
  counts: obj({ skill: num, command: num, agent: num, rule: num, memory: num }),
  tokens,
  visible: obj({ skill: num, skillTotal: num }),
  warnings: arr(str),
  enabled: opt(bool),
});
export type SourceRow = Infer<typeof sourceRow>;

export const sourcesLsData = obj({
  generatedAt: str,
  partial: bool,
  skipped: arr(obj({ detector: str, reason: str })),
  sources: arr(sourceRow),
  items: opt(arr(sourceItem)),
});
export type SourcesLsData = Infer<typeof sourcesLsData>;

export const sourcesRootLsData = obj({
  roots: arr(
    obj({
      path: str,
      origin: lit("default", "config"),
      exists: bool,
      repo: bool,
    }),
  ),
});
export type SourcesRootLsData = Infer<typeof sourcesRootLsData>;

export const sourcesRootChangeData = obj({
  dryRun: bool,
  plan: planV1,
  result: nullable(resultV1),
});
export type SourcesRootChangeData = Infer<typeof sourcesRootChangeData>;

export const skillsLintData = obj({
  errors: num,
  messages: arr(obj({ level: str, message: str, name: str })),
  warnings: num,
});
export type SkillsLintData = Infer<typeof skillsLintData>;

export const skillsSyncData = obj({
  backupRoot: str,
  cleaned: arr(any),
  clientCount: num,
  clientSource: str,
  collisions: arr(any),
  dryRun: bool,
  entries: arr(
    obj({ clientsSynced: arr(str), name: str, type: str, warnings: arr(str) }),
  ),
  globalMode: bool,
  kept: num,
  outputRoot: str,
  replaced: num,
  repo: str,
  ruleCount: num,
  skillCount: num,
  syncedAt: str,
  targetedClients: arr(str),
});
export type SkillsSyncData = Infer<typeof skillsSyncData>;

export const agentsLsData = obj({
  agents: arr(
    obj({ description: str, model: str, name: str, path: str, tools: arr(str) }),
  ),
  discoveryWarnings: arr(str),
  repo: str,
});
export type AgentsLsData = Infer<typeof agentsLsData>;

export const stylesLsData = obj({
  active: arr(str),
  discoveryWarnings: arr(str),
  lockfilePresent: bool,
  repo: str,
  styles: arr(
    obj({
      clientsSynced: arr(str),
      description: str,
      keepCodingInstructions: bool,
      name: str,
      path: str,
      synced: bool,
    }),
  ),
});
export type StylesLsData = Infer<typeof stylesLsData>;

export const authStatuslineData = obj({
  auth: obj({
    expiring: num,
    misconfigured: num,
    needs_reauth: num,
    ok: num,
    revoked: num,
    text: str,
    unreachable: num,
    worst: arr(str),
  }),
});
export type AuthStatuslineData = Infer<typeof authStatuslineData>;

export const compressionStatusData = obj({
  configExists: bool,
  configPath: str,
  contexts: num,
  migrationNotes: arr(str),
  pin: obj({
    drift: nullable(bool),
    installed: nullable(str),
    package: str,
    pin: str,
    requirement: str,
  }),
  preset: obj({
    knobCount: num,
    mode: str,
    name: str,
    port: num,
    savingsProfile: nullable(str),
    snapshotVersion: nullable(str),
  }),
  provider: str,
  runtime: str,
  scope: arr(str),
  shims: obj({ exists: bool, path: str }),
});
export type CompressionStatusData = Infer<typeof compressionStatusData>;

export const syncStatusData = obj({
  backend: nullable(str),
  branch: nullable(str),
  configured: bool,
  keyfilePresent: bool,
  lastDirection: str,
  lastSyncAt: str,
  machineId: nullable(str),
  projects: rec(any),
  repoUrl: nullable(str),
  tracked: num,
});
export type SyncStatusData = Infer<typeof syncStatusData>;

export const ccListData = obj({
  mode: str,
  plugins: arr(
    obj({
      available: nullable(str),
      blocked: bool,
      enabled: bool,
      error: nullable(str),
      id: str,
      installed: str,
      marketplace: str,
      name: str,
      outcome: nullable(str),
      status: str,
    }),
  ),
  refreshError: nullable(str),
  restartRequired: bool,
});
export type CcListData = Infer<typeof ccListData>;

export const updateData = obj({
  counts: rec(num),
  mode: str,
  servers: arr(obj({ detected: bool, id: str, kind: str, message: str, status: str })),
});
export type UpdateData = Infer<typeof updateData>;

const flagShape = obj({
  name: str,
  aliases: arr(str),
  valueType: str,
  required: bool,
  repeatable: bool,
  escalates: bool,
  hidden: bool,
  sensitive: bool,
  effect: str,
  choices: opt(arr(str)),
});

const commandRow = obj({
  id: str,
  path: arr(str),
  group: str,
  kind: lit("command", "group"),
  parent: nullable(str),
  summary: str,
  planned: bool,
  tier: nullable(lit("read", "write", "destructive")),
  baseTier: nullable(lit("read", "write", "destructive")),
  dryRun: bool,
  preview: nullable(
    obj({
      mode: lit("none", "flag", "unless-applied"),
      flag: nullable(str),
    }),
  ),
  needs: arr(lit("stdin", "browser", "long-running", "network", "terminal-only")),
  cost: bool,
  surface: nullable(lit("screen", "terminal")),
  operands: arr(obj({ name: str, required: bool, variadic: bool })),
  maxOperands: nullable(num),
  flags: arr(flagShape),
  operandEscalates: bool,
  oneOf: arr(arr(str)),
  tools: arr(str),
});

const toolRow = obj({
  name: str,
  tier: nullable(lit("read", "write", "destructive")),
  toolTier: num,
  dryRun: lit("none", "param", "default-on"),
  command: nullable(str),
});

/** `toolportctl commands --json`: what the All commands page and the parity manifest read. */
export const commandsData = obj({
  tiers: arr(str),
  needs: arr(str),
  counts: obj({ rows: num, commands: num, groups: num, tools: num }),
  commands: arr(commandRow),
  tools: arr(toolRow),
});
export type CommandsData = Infer<typeof commandsData>;
export type CommandRow = Infer<typeof commandRow>;
export type CommandFlag = Infer<typeof flagShape>;

/** Golden file stem (`profile-create.preview`) to the shape of its envelope `data`. */
export const ctlShapes: Record<string, Shape<unknown>> = {
  commands: commandsData,
  status: statusData,
  doctor: doctorData,
  "server-ls": serverLsData,
  "server-info": serverInfoData,
  "server-uninstall.preview": serverUninstallData,
  "profile-ls": profileLsData,
  "profile-create.preview": profileCreateData,
  "profile-create.apply": profileCreateData,
  "profile-create.after": profileLsData,
  "profile-edit.preview": profileEditData,
  "profile-edit.apply": profileEditData,
  "profile-rm.setup": profileCreateData,
  "profile-rm.preview": profileRmData,
  "client-ls": clientLsData,
  "client-sync.preview": clientSyncData,
  "client-sync.apply": clientSyncData,
  "secret-set.apply": secretSetData,
  "secret-set.get": secretGetData,
  "skills-ls.repo": skillsLsData,
  "skills-ls.library": skillsLsData,
  "skills-ls.source": skillsLsData,
  "sources-ls.summary": sourcesLsData,
  "sources-ls.items": sourcesLsData,
  "sources-ls.partial": sourcesLsData,
  "sources-ls.org": sourcesLsData,
  "sources-root-ls": sourcesRootLsData,
  "sources-root-add.preview": sourcesRootChangeData,
  "sources-root-add.apply": sourcesRootChangeData,
  "sources-root-rm.preview": sourcesRootChangeData,
  "sources-root-rm.apply": sourcesRootChangeData,
  "skills-lint": skillsLintData,
  "skills-sync.preview": skillsSyncData,
  "skills-sync.apply": skillsSyncData,
  "agents-ls": agentsLsData,
  "styles-ls": stylesLsData,
  "auth-statusline": authStatuslineData,
  "compression-status": compressionStatusData,
  "sync-status": syncStatusData,
  "cc-list": ccListData,
  "update.check": updateData,
  "update.preview": updateData,
};
