/** The `data` of the toolportctl envelopes the Context screen reads. The Rust goldens under
 * plus/fixtures/ctl-envelopes (context-*, plugins-*) are the source; data.test.ts parses them
 * against these shapes. */

export type LoadKind = "memory" | "import" | "rule" | string;
export type LoadSource =
  "policy" | "org" | "user" | "project" | "local" | "plugin" | string;

export interface LoadItem {
  kind: LoadKind;
  name: string;
  path: string | null;
  source: LoadSource;
  loaded: boolean;
  /** Loads on demand, when Claude reads matching files; counted apart. */
  lazy: boolean;
  reason: string;
  tokens: number;
  /** Who keeps a left-out row out: `toolport` or `settings`. */
  leftOutBy: "toolport" | "settings" | null;
}

export interface Calibration {
  factor: number;
  measured: number;
  estimated: number;
  measuredAt: string;
  stale: boolean;
  calibratedTotal: number;
  meaning: string;
}

export interface MeasuredSummary {
  total: number;
  windowPercent: number;
  model: string;
  claudeCodeVersion: string;
  measuredAt: string;
  stale: boolean;
}

export interface VerifyMismatch {
  path: string;
  expected: string;
  actual: string;
}

export interface Verified {
  status: string;
  checked: number;
  loaded: number;
  absent: number;
  mismatches: VerifyMismatch[];
  unknown: string[];
  measuredAt: string;
  claudeCodeVersion: string;
  stale: boolean;
}

export interface LoadsData {
  cwd: string;
  totalTokens: number;
  basis: "estimate";
  tokenizer: string;
  calibration: Calibration | null;
  notItemised: string;
  items: LoadItem[];
  /** Present with `--measured`. */
  measured?: MeasuredSummary | null;
  verified?: Verified | null;
}

export interface AppliedBundle {
  bundle: string;
  appliedAt: string;
  changedKeys: string[];
  drift: boolean;
  ownedKeys: Record<string, string[]>;
}

export interface BundleStatusData {
  folder: string;
  applied: AppliedBundle | null;
  conflicts: string[];
}

export interface FolderStatusData {
  cwd: string;
  bundle: BundleStatusData;
  leftOut: { files: string[]; memoryOff: boolean };
  pluginsOff: string[];
  estimate: { totalTokens: number; basis: "estimate"; tokenizer: string };
  measured: MeasuredSummary | null;
  calibration: Calibration | null;
}

export interface PlanStep {
  op: "create" | "update" | "delete" | "note" | string;
  path: string;
  detail: string;
  keys?: string[];
  diff?: { before: string; after: string };
}

export interface Plan {
  summary: string;
  steps: PlanStep[];
  effects: { tokens?: { before: number; after: number; basis: string } };
  warnings: string[];
  undo: string;
}

export interface WriteResult {
  applied: boolean;
  changed: string[];
  undo: string;
  backups: string[];
}

/** `leave-out add|rm`, `plugins off|on`, `bundle apply|undo` and `use` all answer with a plan and,
 * once applied, a result. */
export interface WriteData {
  id?: string;
  scope?: "folder" | string;
  cwd?: string;
  /** Where the settings file is written: the repository root (the main checkout in a worktree). */
  root?: string;
  folder?: string;
  dryRun: boolean;
  plan: Plan;
  result: WriteResult | null;
  conflicts?: unknown[];
  bundle?: string | null;
  leftOut?: string[];
  memoryOff?: boolean;
}

export interface MeasurePlan {
  cost: string;
  model: string;
  requests: number;
  sends: string;
  variants: string[];
  writes: string;
}

export interface MeasureVariant {
  label: string;
  total: number;
  deltaTokens: number;
  deltaPercent: string | number;
  cached: boolean;
}

export interface MeasureData {
  cwd: string;
  ran: boolean;
  plan?: MeasurePlan;
  start?: string;
  claudeCodeVersion?: string;
  model?: string;
  measuredAt?: string;
  cached?: boolean;
  stale?: boolean;
  basis?: "measurement";
  total?: number;
  contextWindow?: number;
  windowPercent?: number;
  plugins?: string[];
  mcpServers?: string[];
  itemised?: { tokens: number; basis: "estimate"; rows: number };
  notItemised?: { tokens: number; percent: number; includes: string };
  variants?: MeasureVariant[];
  verified?: Verified | null;
  notes?: string[];
}

export interface BundleRow {
  name: string;
  description: string;
  /** The access set the bundle names; shown as a link to Settings > Access. */
  servers: string | null;
  skills: { off: number; nameOnly: number; allow: number };
  plugins: { off: string[] };
  layers: { add: string[]; exclude: string[] };
  agents: { off: string[] };
  bind: string[];
  appliedTo: string[];
  path: string;
  issues: number;
  memory?: { off: boolean };
  error?: string;
}

export interface BundleListData {
  bundles: BundleRow[];
  directory: string;
}

export interface BundleIssue {
  level?: string;
  message?: string;
  [key: string]: unknown;
}

export interface BundleShowData {
  name: string;
  path: string;
  description: string;
  servers: string | null;
  skills: { off: string[]; nameOnly: string[]; allow: string[] };
  plugins: { off: string[] };
  mcp: { deny: string[] };
  layers: { add: string[]; exclude: string[] };
  dirs: { add: string[] };
  agents: { off: string[] };
  bind: string[];
  yaml: string;
  issues: BundleIssue[];
  appliedTo: string[];
  memory?: { off: boolean };
}

export interface BundleSaveData {
  name: string;
  created?: boolean;
  issues?: BundleIssue[];
  dryRun: boolean;
  plan: Plan;
  result: WriteResult | null;
}

export interface BundleConfigData {
  autoApply: boolean;
  file: string;
}

export interface BundleLaunchData {
  bundle: string;
  settingsFile: string;
  command: string;
  cwd: string | null;
  notes: string[];
}

export interface SyncData {
  dryRun: boolean;
  autoApply: boolean;
  bundles: Array<{
    bundle: string;
    folder: string;
    applied: boolean;
    error?: string;
  }> | null;
  deploy: { built: boolean; note: string };
}

export interface ComposePart extends LoadItem {
  text: string;
}

export interface ComposeData {
  cwd: string;
  parts: ComposePart[];
  totalTokens: number;
  basis: "estimate";
}
