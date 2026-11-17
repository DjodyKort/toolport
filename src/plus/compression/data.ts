/** The `data` of the toolportctl envelopes the Hooks & compression tab reads. The Rust goldens under
 * plus/fixtures/ctl-envelopes (compression-hooks-*, compression-status, compression-doctor,
 * compression-measure) are the source; data.test.ts parses them against these shapes. */

export type HookProviderId = "rtk" | "read-once";
export type ProviderName = "headroom" | "rtk-only" | "parsec" | "none";

export type EntryState = "absent" | "foreign" | "ours";
/** What the ledger says next to what the file says. */
export type EntryHealth = "absent" | "foreign" | "ok" | "edited" | "missing";

export interface HookEntry {
  file: string;
  provider: HookProviderId;
  state: EntryState;
  command: string | null;
  health: EntryHealth;
  wrappedAt: string | null;
}

export interface HookCounter {
  provider: HookProviderId;
  basis: "mechanical";
  calls: number;
  answered: number;
  failed: number;
  medianLatencyMs: number | null;
  p95LatencyMs: number | null;
  maxLatencyMs: number | null;
  firstCall: string | null;
  lastCall: string | null;
}

export interface Trust {
  closedEngine: boolean;
  installsGlobalHooks: boolean;
  needsEgressConsent: boolean;
  sendsCodeOffMachine: boolean;
}

export interface HooksStatusData {
  paused: boolean;
  pausedSince: string | null;
  /** When a pause with an end resumes by itself; `null` for a pause with no end. */
  pausedUntil: string | null;
  entries: HookEntry[];
  counters: HookCounter[];
  wrapped: number;
  epochs: number;
  cannotTell: string;
  trust: Array<{ provider: ProviderName; trust: Trust }>;
}

export type HookAction =
  | "wrapped"
  | "would wrap"
  | "kept"
  | "none to wrap"
  | "restored"
  | "would restore"
  | "restored ours"
  | "would restore ours"
  | "re-applied"
  | "would re-apply"
  | "edited since, left alone"
  | "gone, dropped from the ledger";

export interface HookWriteRow {
  file: string;
  provider: HookProviderId;
  action: HookAction;
  state?: EntryState;
  command?: string | null;
}

export interface HooksWriteData {
  dryRun: boolean;
  rows: HookWriteRow[];
}

export interface PauseData {
  paused: boolean;
  changed: boolean;
  pausedSince: string | null;
  pausedUntil: string | null;
}

export interface CompressionStatusData {
  configExists: boolean;
  provider: ProviderName;
  runtime: string;
  preset: { name: string; mode: string; port: number };
  pin: {
    pin: string;
    installed: string | null;
    drift: boolean | null;
    requirement: string;
  };
  shims: { exists: boolean; path: string };
}

export interface DoctorCheck {
  name: string;
  ok: boolean;
  detail: string;
}

export interface CompressionDoctorData {
  provider: ProviderName;
  healthy: boolean;
  checks: DoctorCheck[];
}

export interface Population {
  sessions: number;
  from: string | null;
  to: string | null;
}

export interface SourceRow {
  kind: string;
  share: number;
}

export interface SourcesSection {
  status: "data" | "no_transcripts" | "no_results";
  basis: "estimate";
  method: string;
  rows: SourceRow[];
  bashCeiling: number | null;
  population: Population;
  cannotTell: string;
}

export interface HookFigure {
  provider: HookProviderId;
  wrapped: boolean;
  calls: number;
  answered: number;
  failed: number;
  p50Ms: number | null;
  p95Ms: number | null;
  firstCall: string | null;
  lastCall: string | null;
}

export interface HooksSection {
  status: "data" | "no_calls" | "off";
  basis: "mechanical";
  providers: HookFigure[];
  outputBytes: { available: false; reason: string };
  providerReported: Array<{
    provider: string;
    entries: number;
    tokensBefore: number;
    tokensAfter: number;
  }>;
  population: {
    hooks: number;
    calls: number;
    from: string | null;
    to: string | null;
  };
  cannotTell: string;
}

export type PeriodState = "off" | "on" | "paused";

export interface Interval {
  from: string | null;
  to: string | null;
}

export interface OutcomeGroup {
  state: PeriodState;
  label: string;
  intervals: Interval[];
  sessions: number;
  medianCost: number | null;
  lowerQuartile: number | null;
  upperQuartile: number | null;
  /** Cost relative to the median of the hooks-on window (= 100). */
  relative: number | null;
  relativeLow: number | null;
  relativeHigh: number | null;
}

export interface Verdict {
  status: "none" | "refused" | "compared";
  reason: string;
  needsSessions?: number;
  overlap?: boolean;
}

export interface OutcomeSection {
  status: "data" | "no_sessions";
  timeline: Array<{
    state: PeriodState;
    label: string;
    from: string | null;
    to: string | null;
  }>;
  groups: OutcomeGroup[];
  referenceState: "on";
  verdict: Verdict;
  population: { sessions: number; minTurns: number; unplaced: number };
  costUnits: Record<string, number | string>;
  cannotTell: string;
}

export interface ReworkGroup {
  state: PeriodState;
  label: string;
  sessions: number;
  repeatedCommands: number | null;
  repeatedReads: number | null;
}

export interface ReworkSection {
  status: "data" | "no_sessions";
  groups: ReworkGroup[];
  population: { sessions: number; minTurns: number; unplaced: number };
  cannotTell: string;
}

export interface MeasureReport {
  transcripts: { root: string; sessions: number; since: string | null };
  sources: SourcesSection;
  hooks: HooksSection;
  outcome: OutcomeSection;
  rework: ReworkSection;
  upstream: { note: string };
}
