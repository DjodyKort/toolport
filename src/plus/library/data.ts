/** The `data` of the toolportctl envelopes the Library screen reads. The Rust goldens under
 * plus/fixtures/ctl-envelopes (skills-*, library-*, sources-*, agents-*, styles-*) are the source;
 * data.test.ts parses them against these shapes. */

export type ChangeOp =
  "add" | "update" | "adopt" | "differs" | "edited" | "remove" | "hook" | string;

/** What a plan row is about; skills and rules carry no kind, agents and output styles do. */
export type ChangeKind = "agent" | "style";

export interface PlanChange {
  kind?: ChangeKind;
  skill: string;
  client: string;
  path: string;
  op: ChangeOp;
  held: boolean;
}

export interface PlanCounts {
  add: number;
  update: number;
  adopt: number;
  unchanged: number;
  differs: number;
  edited: number;
  remove: number;
  hook: number;
  kept: number;
}

/** A step of a run that did not complete while the rest went on; `id` is what `--only` takes. */
export interface FailedStep {
  id: string;
  kind: "skill" | "agent" | "style";
  name: string;
  client: string;
  step: "files" | "hooks" | string;
  message: string;
}

export interface SyncPlan {
  counts: PlanCounts;
  held: number;
  changes: PlanChange[];
  skipped: Array<{ skill: string; path: string; reason: string }>;
  failed?: FailedStep[];
}

export interface HeldRow {
  kind?: ChangeKind;
  skill: string;
  client: string;
  path: string;
  op: ChangeOp;
  reason: "removal" | "new-hook" | "differs" | "edited" | string;
}

export interface SnapshotInfo {
  id: string;
  files: number;
  hooks: number;
  createdAt?: string;
}

export interface VerifyMismatch {
  client: string;
  path: string;
  reason: string;
  skill: string;
}

export interface VerifyData {
  clean: boolean;
  matched: number;
  total: number;
  mismatched: VerifyMismatch[];
  lockfilePresent?: boolean;
  warnings?: string[];
}

export interface CollisionRow {
  skill: string;
  client: string;
  collisionPath: string;
  syncedPath: string;
  action: string;
  backupPath: string | null;
}

export interface SyncData {
  dryRun: boolean;
  unattended: boolean;
  paused: boolean;
  skipped?: boolean;
  message?: string;
  reason?: string;
  since?: string | null;
  repo: string;
  skillCount: number;
  ruleCount: number;
  targetedClients: string[];
  entries: Array<{
    name: string;
    type: "skill" | "rule" | string;
    clientsSynced: string[];
    warnings: string[];
  }>;
  collisions: CollisionRow[];
  replaced: number;
  kept: number;
  plan: SyncPlan;
  held: HeldRow[];
  snapshot: SnapshotInfo | null;
  verify: VerifyData | null;
  syncedAt: string;
  outputRoot: string;
  backupRoot: string;
}

export interface UndoFile {
  action: "remove" | "restore" | "conflict" | string;
  path: string;
  reason: string | null;
}

export interface UndoData {
  dryRun: boolean;
  files: UndoFile[];
  hooks: unknown[];
  removed: number;
  restored: number;
  conflicts: number;
  snapshot: SnapshotInfo;
  autoSync: { willPause?: boolean; paused?: boolean; reason?: string; since?: string };
}

/** What the last unattended sync did. `held` are the changes it left for the user. */
export interface AutoLastRun {
  at: string;
  applied: number;
  held: HeldRow[];
  skipped: number;
  error?: string;
}

/** The shipped task that runs the unattended sync. `nextRun` is null while it is off. */
export interface AutoSchedule {
  task: string;
  /** False once the task was removed: nothing runs the sync. */
  present: boolean;
  enabled: boolean;
  cron: string | null;
  when: string | null;
  autoRun: boolean;
  nextRun: string | null;
  /** The task's own last run, which says whether the ctl started and exited cleanly. */
  lastRun: { runId: string; status: string; trigger: string; startedAt: string } | null;
}

export interface AutoStatus {
  paused: boolean;
  reason: string | null;
  since: string | null;
  lastRun: AutoLastRun | null;
  schedule: AutoSchedule | null;
}

export interface AutoSwitchData {
  dryRun: boolean;
  task: string;
  enabled: boolean;
  schedule: AutoSchedule | null;
}

/** What Claude Code counts of a skill's listing text: `length` is the description and
 * `when_to_use` together, against the two limits one meter draws. */
export interface ListingMeter {
  length: number;
  description: number;
  whenToUse: number;
  limit: number;
  specLimit: number;
}

export type LintLevel = "error" | "warning" | "info";

export interface LintRow {
  code: string;
  consequence: string;
  file?: string;
  fixable?: string;
  level: LintLevel;
  line?: number;
  message: string;
  name: string;
  meter?: ListingMeter;
  reason?: string;
  target?: string;
}

export interface LintData {
  errors: number;
  warnings: number;
  limits: { description: number; specDescription: number };
  consequences: Record<string, number>;
  messages: LintRow[];
}

export interface AuditFinding {
  severity?: string;
  skill?: string;
  message?: string;
  line?: number | null;
}

export interface AuditData {
  clean: boolean;
  findings: AuditFinding[];
  high: number;
  medium: number;
  low: number;
  skillCount: number;
}

export interface FixData {
  count: number;
  dryRun: boolean;
  fix: string;
  fixed: Array<{ event: string; file: string; from: string; skill: string; to: string }>;
}

export interface MoveLinkData {
  dryRun: boolean;
  skill: string;
  link: string;
  from: string;
  /** Where the copy sits in the skill folder, and the text the link has afterwards. */
  to: string;
  reused: boolean;
  size: number;
  count: number;
  rewrites: Array<{ line: number; before: string; after: string }>;
}

export type FileStatus =
  | "would-copy"
  | "copied"
  | "differs"
  | "hand-edited"
  | "not-copied"
  | "not-yours"
  | "removed-on-next-sync"
  | "kept"
  | string;

export interface SkillFile {
  client: string | null;
  op: string | null;
  path: string;
  reason: string | null;
  size: number;
  status: FileStatus;
}

export interface SkillLink {
  line: number;
  reason: string | null;
  state: "installed" | "missing" | "outside-folder" | "not-installed" | string;
  target: string;
  /** `move-link` when the file it points at can be moved into the skill. */
  fixable?: string | null;
}

export interface FilesData {
  skill: string;
  type: string;
  source: string;
  counts: Record<string, number>;
  denyList: string[];
  description: ListingMeter;
  files: SkillFile[];
  installed: Array<{ client: string; dir: string; present: boolean }>;
  links: SkillLink[];
  lockfilePresent: boolean;
  targetedClients: string[];
}

export interface CompareSide {
  present: boolean;
  size: number;
  /** Null for a file that is not text or is not there. */
  text: string | null;
}

export interface CompareFile {
  path: string;
  client: string;
  status: FileStatus;
  installedPath: string;
  binary: boolean;
  truncated: boolean;
  identical: boolean;
  library: CompareSide;
  installed: CompareSide;
  /** The unified diff, library to installed; null for a binary file. */
  diff: string | null;
  added: number;
  removed: number;
}

export interface CompareData {
  repo: string;
  skill: string;
  type: string;
  lockfilePresent: boolean;
  differing: number;
  files: CompareFile[];
}

export interface KeepData {
  skill: string;
  dryRun: boolean;
  count: number;
  kept: Array<{ client: string; path: string; status: string }>;
}

export interface StatusEntry {
  name: string;
  type: "skill" | "rule" | string;
  clientsSynced: string[];
  drifted: boolean;
  knownToLockfile: boolean;
}

/** One installed copy the lock records: a skill, rule, agent or style of a client. */
export interface OutputRow {
  name: string;
  client: string;
  /** False when the file the lock says was written is gone. */
  present: boolean;
}

export interface StatusData {
  drift: boolean;
  entries: StatusEntry[];
  lockedCount: number;
  lockfilePresent: boolean;
  lockfileSyncedAt: string | null;
  outputs: OutputRow[];
  repo: string;
  targetedClients: string[];
}

export interface LibrarySkill {
  name: string;
  type: "skill" | "rule" | string;
  description: string;
  activation: string;
  visible: boolean;
  invisibleReason: string | null;
  writable: boolean;
  path: string;
  origin: { kind: string; name: string };
}

export interface SkillListData {
  repo: string;
  skills: LibrarySkill[];
  partial?: boolean;
}

export interface RemoteStatus {
  repo: string;
  branch: string;
  remote: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  uncommitted: number;
  lastFetch: string | null;
  auth: { checked: boolean; method: string; ok: boolean };
  fetch: { requested: boolean; ok: boolean | null; error: string | null };
  duplicateClones: Array<{ path: string; skills: number; ahead: number; behind: number }>;
}

export interface PlanStep {
  op: string;
  detail: string;
  path?: string;
}

export interface WritePlan {
  summary: string;
  steps: PlanStep[];
  warnings: string[];
  undo: string;
  effects: Record<string, unknown>;
}

export interface PullData {
  dryRun: boolean;
  repo: string;
  plan?: WritePlan;
  pulled?: boolean;
  commits?: number;
}

export interface PushCheck {
  audit: { ran: boolean; high: number; skills: number; findings: AuditFinding[] };
  blocked: boolean;
  builtinScan: {
    count: number;
    findings: Array<{ commit: string; file: string; line: number; rule: string }>;
  };
  gitleaks: { available: boolean; ran: boolean; count: number; error: string | null };
}

export interface PushData {
  dryRun: boolean;
  repo: string;
  branch?: string;
  checks: PushCheck;
  commits?: Array<{ sha: string; subject: string }>;
  plan?: WritePlan;
  pushed: boolean;
}

export interface CommitData {
  dryRun: boolean;
  repo: string;
  committed: boolean;
  message?: string;
  commitSha?: string;
  files: string[];
  checks?: PushCheck;
  plan?: WritePlan;
}

export interface ConnectData {
  dryRun: boolean;
  repo: string;
  remote: string;
  connected?: boolean;
  empty?: boolean;
  plan?: WritePlan;
}

export interface RemoteAddData {
  dryRun: boolean;
  repo: string;
  remote: string;
  plan?: WritePlan;
}

export interface MarketplacePlugin {
  id?: string;
  name: string;
  linked?: boolean;
  skills?: number;
  agents?: number;
  hooks?: boolean;
  version?: string;
}

/** A tap that is a Claude Code plugin marketplace. `declared` is `here` when Claude Code's
 * settings point at this clone, `no` when they do not and `elsewhere: <source>` when another
 * source already holds the name. */
export interface TapMarketplace {
  name?: string;
  declared?: string;
  plugins?: MarketplacePlugin[];
  warnings?: string[];
  error?: string;
}

export interface TapRow {
  name: string;
  repo: string;
  url: string;
  path: string;
  cloned: boolean;
  marketplace?: TapMarketplace | null;
}

/** `skills tap link` and `skills tap unlink`: the settings keys written or removed. */
export interface TapLinkData {
  tap: string;
  dryRun: boolean;
  scope: string;
  changes: Array<{ action: string; key: string; path: string; scope: string }>;
  plan: WritePlan;
  plugins?: MarketplacePlugin[];
  marketplace?: string;
  marketplaces?: string[];
  result: { applied: boolean; changed: string[]; undo: string } | null;
}

export interface TapListData {
  tapsRoot: string;
  taps: TapRow[];
}

export interface TapUpdateData {
  dryRun: boolean;
  failed: number;
  results: Array<{
    name: string;
    ok: boolean;
    head: string | null;
    error: string | null;
  }>;
}

export interface TapChangeData {
  name: string;
  dryRun: boolean;
  removed?: boolean;
  cloned?: boolean;
  repo?: string;
}

export interface SearchHit {
  tap: string;
  repo: string;
  name: string;
  description: string;
  type: string;
}

export interface SearchData {
  query: string;
  tapCount: number;
  results: SearchHit[];
  discoveryWarnings: string[];
}

export interface InstallData {
  spec: string;
  tap: string;
  dryRun: boolean;
  blocked: boolean;
  target: string;
  audit: {
    ran: boolean;
    high: number;
    medium: number;
    low: number;
    findings: AuditFinding[];
  };
  skills: Array<{
    name: string;
    type: string;
    path: string;
    files: number | string[];
    status: string;
  }>;
  installedCount: number;
  skippedCount: number;
  discoveryWarnings: string[];
}

export interface SourceRow {
  id: string;
  origin: { kind: string; name: string };
  detector: string;
  owner: string;
  root: string;
  writable: boolean;
  counts: Record<string, number>;
  status: { state: string; detail: string; checkedAt?: string };
  freshness: { ahead: number; behind: number; ref: string } | null;
  warnings: string[];
  tokens: { basis: string; value: number };
}

export interface SourcesData {
  generatedAt: string;
  partial: boolean;
  skipped: unknown[];
  sources: SourceRow[];
}

export interface RootRow {
  path: string;
  exists: boolean;
  origin: string;
  repo: boolean;
}

export interface RootListData {
  roots: RootRow[];
}

export interface RootChangeData {
  dryRun: boolean;
  path?: string;
  added?: boolean;
  removed?: boolean;
}

export interface CleanData {
  dryRun: boolean;
  managed: string[];
  removed: string[];
  skipped: unknown[];
  lockfileRemoved: boolean;
}

export interface BundleData {
  dryRun: boolean;
  output: string;
  fileCount: number;
  sourceBytes: number;
  bundleBytes: number | null;
  skills: Array<{ name: string; type: string; files: number }>;
}

export interface UnbundleData {
  dryRun: boolean;
  bundle: string;
  names: string[];
  files: string[];
  overwritten: string[];
  skipped: unknown[];
  target: string;
}

export interface UninstallData {
  dryRun: boolean;
  name: string;
  outputs: string[];
  sourcePath: string;
}

export interface AddData {
  dryRun: boolean;
  name: string;
  path: string;
  files: string[];
  type: string;
}

/** `agents add` and `styles add`: the one file a new item starts with. */
export interface ContentAddData {
  dryRun: boolean;
  name: string;
  path: string;
  repo: string;
}

/** `agents diff` and `styles diff`: what changed in the library since the last sync. */
export interface ContentDiffData {
  repo: string;
  noLockfile: boolean;
  clean: boolean;
  new: string[];
  modified: string[];
  removed: string[];
  unchanged: number;
}

/** `agents clean` and `styles clean`: the synced copies removed, the library kept. */
export interface ContentCleanData {
  dryRun: boolean;
  lockfilePresent: boolean;
  managed: string[];
  removed: string[];
  skipped: Array<{ client: string; error: string }>;
}

/** `mcp call agents_get|styles_get`: one item with its body. */
export interface ContentGet {
  name: string;
  description: string;
  path: string;
  body: string;
}

export interface ToolCall<T> {
  tool: string;
  tier: number;
  isError: boolean;
  result: T;
}

export interface AgentRow {
  name: string;
  description: string;
  model: string;
  path: string;
  tools: string[];
}

export interface AgentListData {
  agents: AgentRow[];
  repo: string;
  discoveryWarnings: string[];
}

export interface AgentStatusData {
  drift: boolean;
  lockedCount: number;
  lockfilePresent: boolean;
  outputs: OutputRow[];
  repo: string;
}

export interface AgentSyncData {
  dryRun: boolean;
  agentCount: number;
  agents: Array<{
    name: string;
    clientsSynced: string[];
    outputFiles: Array<{ client: string; files: string[] }>;
    warnings: string[];
  }>;
}

export interface AgentLintData {
  errors: number;
  warnings: number;
  infos: number;
  messages: Array<{ level: string; name: string; message: string }>;
}

export interface StyleRow {
  name: string;
  description: string;
  path: string;
  synced: boolean;
  clientsSynced: string[];
}

export interface StyleListData {
  styles: StyleRow[];
  active: string[];
  repo: string;
  lockfilePresent: boolean;
}

export interface StyleSyncData {
  dryRun: boolean;
  styleCount: number;
  outputs: string[];
  styles: Array<{ name: string; clientsSynced: string[]; warnings: string[] }>;
}

/** `styles status`: the styles each native client received, and the style an apply-and-remove
 * client has switched on. */
export interface StyleStatusData {
  repo: string;
  lockfilePresent: boolean;
  native: Array<{ client: string; name: string; styles: string[] }>;
  applyRemove: Array<{ client: string; name: string; active: string | null }>;
}

export interface StyleClientPair {
  client: string;
  style: string;
}

/** `styles apply`: the rule file each chosen client gets, and the style it replaces. */
export interface StyleApplyData {
  name: string;
  dryRun: boolean;
  scope: string;
  applied: Array<{ client: string; path: string }>;
  appliedCount: number;
  replaced: StyleClientPair[];
  active: StyleClientPair[];
  nativeClients: string[];
}

/** `styles remove`: the rule file taken away from each client. */
export interface RemoveData {
  dryRun: boolean;
  hadActive: boolean;
  removed: Array<{ client: string; style: string; path: string }>;
  active: StyleClientPair[];
}

export interface InitData {
  dryRun: boolean;
  alreadyExists: boolean;
  created: string[];
  repo: string;
  name: string;
}
