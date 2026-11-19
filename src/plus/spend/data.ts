/** The `data` of the `toolportctl spend ...` envelopes the Spend page reads. The Rust goldens
 * under plus/fixtures/ctl-envelopes (spend-*) are the source; data.test.ts parses them against
 * these shapes. */

export type Badge = "Measured" | "Counted" | "Estimate" | "Not itemised";

export type RangeId = "today" | "7d" | "30d";

export interface Money {
  usd: number;
  /** The part of `usd` that comes from list prices (transcripts). */
  estimatedUsd: number;
  badge: Badge;
}

export interface TokenCounts {
  input: number;
  output: number;
  cacheRead: number;
  cacheCreation: number;
  total: number;
  badge: Badge;
  /** Tokens that come from transcripts instead of OTel events. */
  fromTranscript: number;
}

export interface SpendRow {
  key: string;
  tokens: TokenCounts;
  cost: Money;
  requests: number;
  sessions: number;
  /** Share of the range's cost, 0 to 1. */
  share: number;
}

export interface DayRow extends SpendRow {
  /** Cost per model name in USD; the stacked bar's segments. */
  byModel: Record<string, number>;
}

export interface SessionRow extends SpendRow {
  start: string;
  source: "otel" | "transcript";
  /** The run's task, "Interactive" or "Unattributed". */
  task: string;
  /** The model that cost most in the session. */
  model: string | null;
  modelCount: number;
  lengthMs: number;
}

export interface RangeTile {
  tokens: number;
  cost: Money;
  /** Against the period before, as a fraction; null without one. */
  costDelta: number | null;
  previousUsd: number;
  estimatedDays: number;
  days: number;
}

export interface Tiles {
  today: RangeTile;
  "7d": RangeTile;
  "30d": RangeTile;
}

export interface ToolDefinitions {
  sessions: number;
  exposedTokens: number;
  fullTokens: number;
  medianExposedTokens: number;
  medianFullTokens: number;
  tokenizer: string;
  badge: Badge;
}

export interface CompressionPart {
  hooks: number;
  calls: number;
  firstCall: string | null;
  lastCall: string | null;
  /** Null without savings-ledger rows. */
  tokensCut: number | null;
  badge: Badge;
  note: string;
}

export interface FirstRequest {
  cwd: string;
  model: string;
  measuredAt: string;
  total: number;
  /** Estimated size of the instruction files; may be null. */
  instructionTokens: number | null;
  badge: Badge;
}

export interface McpPartReady {
  state: "ready" | "empty";
  toolDefinitions: ToolDefinitions;
  shareOfInputTokens: { value: number | null; badge: Badge };
  savedTokens: number | null;
  compression: CompressionPart;
  firstRequest: FirstRequest | null;
  note: string;
}

export interface McpPartFailed {
  state: "error";
  error: string;
}

export type McpPart = McpPartReady | McpPartFailed;

export interface StaleCheck {
  sessionsSince: number;
  startedBeforeSetup: number;
  /** Project settings files that point the export at another address. */
  projectOverrides: string[];
}

export type CollectState = "off" | "waiting" | "collecting" | "not-listening" | "error";

export interface ReceiverStatus {
  address: string;
  listening: boolean;
  owner: "app" | "gateway" | null;
  requests: number;
  events: number;
  rejected: number;
  dropped: number;
  error: string | null;
}

export interface StoreStatus {
  events: number;
  bytes: number;
  maxBytes: number;
  oldest: string | null;
  retentionDays: number;
  dailyLines: number;
  skippedLines: number;
  /** Whether pruned events are folded into one line per day, project and model first. */
  dailyTotals: boolean;
}

/** The shipped weekly task that runs `context measure` for one folder. */
export interface WeeklyMeasure {
  task: string;
  /** Whether the task file exists in the data folder; a shipped task is listed before that. */
  exists: boolean;
  enabled: boolean;
  /** The folder the task measures; null until one is chosen. */
  folder: string | null;
  cron: string;
  model: string;
}

/** What `setup plan` says about the optional third step. */
export interface WeeklyMeasurePlan extends WeeklyMeasure {
  /** Whether this plan asks for the task (`--weekly-measure`). */
  requested: boolean;
  action: "add" | "already on" | "needs a folder" | "off";
}

export interface SetupRecord {
  profile: string;
  appliedAt: string;
}

export interface TranscriptProgress {
  files: number;
  done: number;
}

export interface StatusData {
  state: CollectState;
  enabled: boolean;
  receiver: ReceiverStatus;
  lastEvent: string | null;
  store: StoreStatus;
  showCost: boolean;
  profilesSetUp: number;
  setup: SetupRecord[];
  transcripts: TranscriptProgress;
  weeklyMeasure: WeeklyMeasure;
}

/** The list prices behind every estimated cost. A price nobody checked against Anthropic's
 * published pricing says so, and the page repeats it next to the estimate. */
export interface PriceTable {
  /** True only when every family and both cache factors are verified. */
  verified: boolean;
  unit: string;
  families: Array<{
    family: string;
    inputUsd: number;
    outputUsd: number;
    verified: boolean;
  }>;
  cacheReadFactor: number;
  cacheWriteFactor: number;
  factorsVerified: boolean;
}

export interface SummaryData {
  range: RangeId;
  project: string | null;
  model: string | null;
  task: string | null;
  since: string;
  totals: {
    key: string;
    tokens: TokenCounts;
    cost: Money;
    requests: number;
    sessions: number;
    share: number;
  };
  previous: { costDelta: number | null; tokensDelta: number | null };
  cacheShare: { value: number; badge: Badge };
  byDay: DayRow[];
  byProject: SpendRow[];
  byModel: SpendRow[];
  byTask: SpendRow[];
  sources: { otelSessions: number; transcriptSessions: number; estimatedShare: number };
  prices: PriceTable;
  tiles: Tiles;
  /** Present when a project, model or task filter is set; largest first. */
  sessionList?: SessionRow[];
  mcpPart: McpPart;
  /** Null unless collecting has data. */
  stale: StaleCheck | null;
  status: StatusData;
  notes: string[];
}

export type PlanAction = "add" | "update" | "already set" | "conflict" | "overwrite";

export interface PlanEntry {
  key: string;
  wanted: string;
  current: string | null;
  action: PlanAction;
}

export interface PlanProfile {
  profile: string;
  exists: boolean;
  entries: PlanEntry[];
  conflicts: number;
  writes: number;
}

export interface SetupPlanData {
  port: number;
  endpoint: string;
  profiles: PlanProfile[];
  writes: number;
  conflicts: number;
  contentFlags: string;
  note: string;
  weeklyMeasure: WeeklyMeasurePlan;
}

export interface SetupApplyData {
  profiles: (PlanProfile & { applied: boolean })[];
  port: number;
  enabled: boolean;
  weeklyMeasure: {
    applied: boolean;
    enabled: boolean;
    folder: string | null;
    /** Why the task was not added although the settings were written. */
    error?: string;
  };
  /** False when a conflicting key was kept: collection will not work until it matches. */
  complete: boolean;
}

export interface SetupApplyDryRunData {
  dryRun: true;
  plan: SetupPlanData;
}

export interface DataCounts {
  events: number;
  bytes: number;
  dailyLines: number;
}

export interface UndoProfile {
  profile: string;
  restored: string[];
  keptChanged: string[];
}

export interface SetupUndoData {
  profiles: UndoProfile[];
  /** True when other profiles still export. */
  enabled: boolean;
  /** With `--delete-data`: what was deleted. */
  deleted?: DataCounts;
}

export interface SetupUndoDryRunData {
  dryRun: true;
  profilesSetUp: number;
  /** Profiles that stay set up. */
  remaining: number;
  deleteData?: DataCounts;
}

export interface PruneData {
  dryRun: boolean;
  removedEvents: number;
  removedBytes: number;
  keptEvents: number;
  keptBytes: number;
  foldedDays: number;
  retentionDays: number;
  maxBytes: number;
  totalsDeleted: boolean;
  /** Whether the removed events were folded into the daily totals. */
  dailyTotals: boolean;
  /** The age window of this prune, in days: the retention, or a shorter one the person chose. */
  olderThanDays: number;
}

export interface ConfigSetData {
  port: number;
  retentionDays: number;
  maxMb: number;
  showCost: boolean;
  dailyTotals: boolean;
  weeklyMeasure: WeeklyMeasurePlan;
  enabled: boolean;
  portChanged: boolean;
  changed: boolean;
  /** Only on a `--dry-run`, which changes nothing. */
  dryRun?: true;
}

export interface ExportCsvData {
  rows: number;
  /** Without `--out`. */
  csv?: string;
  /** With `--out`. */
  out?: string;
  bytes?: number;
}
