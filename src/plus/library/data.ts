/** The `data` of the toolportctl envelopes the Library screen reads. The Rust goldens under
 * plus/fixtures/ctl-envelopes (skills-*, library-*, sources-*, agents-*, styles-*) are the source;
 * data.test.ts parses them against these shapes. */

export type ChangeOp =
  "add" | "update" | "adopt" | "differs" | "edited" | "remove" | "hook" | string;

export interface PlanChange {
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
}

export interface SyncPlan {
  counts: PlanCounts;
  held: number;
  changes: PlanChange[];
  skipped: Array<{ skill: string; path: string; reason: string }>;
}

export interface HeldRow {
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

export interface AutoStatus {
  paused: boolean;
  reason: string | null;
  since: string | null;
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
  meter?: { length: number; limit: number; specLimit: number };
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

export type FileStatus =
  | "would-copy"
  | "copied"
  | "differs"
  | "hand-edited"
  | "not-copied"
  | "not-yours"
  | "removed-on-next-sync"
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
  state: "installed" | "missing" | "outside" | string;
  target: string;
}

export interface FilesData {
  skill: string;
  type: string;
  source: string;
  counts: Record<string, number>;
  denyList: string[];
  description: { length: number; limit: number; specLimit: number };
  files: SkillFile[];
  installed: Array<{ client: string; dir: string; present: boolean }>;
  links: SkillLink[];
  lockfilePresent: boolean;
  targetedClients: string[];
}

export interface StatusEntry {
  name: string;
  type: "skill" | "rule" | string;
  clientsSynced: string[];
  drifted: boolean;
  knownToLockfile: boolean;
}

export interface StatusData {
  drift: boolean;
  entries: StatusEntry[];
  lockedCount: number;
  lockfilePresent: boolean;
  lockfileSyncedAt: string | null;
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
  duplicateClones: string[];
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

export interface TapRow {
  name: string;
  repo: string;
  url: string;
  path: string;
  cloned: boolean;
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

export interface RemoveData {
  dryRun: boolean;
  name: string;
  outputs?: string[];
}

export interface InitData {
  dryRun: boolean;
  alreadyExists: boolean;
  created: string[];
  repo: string;
  name: string;
}
