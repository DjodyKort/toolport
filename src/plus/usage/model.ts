import type { CommandRow } from "../bridge/data";
import type { Tier } from "../allcommands/model";
import type { PlanOp, PlanStep, PlanV1 } from "../ui";

/** The view of `toolportctl usage` and `obs otel status`. The envelopes are read loosely
 * (a missing member is zero or empty) because the byMcpServer, bySession, byDay and byModel
 * members are keyed by names the CLI makes up from the transcripts. */

export interface Counts {
  messages: number;
  input: number;
  output: number;
  cacheCreation: number;
  cacheRead: number;
}

export interface DayRow extends Counts {
  day: string;
}

export interface ModelRow extends Counts {
  name: string;
}

export interface SessionRow extends Counts {
  id: string;
  cwd: string;
  firstTs: string;
  lastTs: string;
}

export interface ProjectRow extends Counts {
  cwd: string;
  name: string;
  sessions: number;
}

export interface ServerRow {
  name: string;
  calls: number;
  tools: Array<{ name: string; calls: number }>;
}

export interface McpFailure {
  session: string;
  server: string;
  ts: string;
}

export interface UsageView {
  totals: Counts;
  days: DayRow[];
  models: ModelRow[];
  sessions: SessionRow[];
  projects: ProjectRow[];
  servers: ServerRow[];
  failures: McpFailure[];
  index: { files: number; messages: number };
  sources: { transcriptMessages: number; otelRequests: number; otelOnly: number };
  otel: {
    events: number;
    costUsd: number;
    apiRequests: { count: number; costUsd: number; durationMs: number };
    toolDecisions: Array<[string, number]>;
    connections: { total: number; failures: Array<{ server: string; count: number }> };
  };
}

type Rec = Record<string, unknown>;

const rec = (value: unknown): Rec =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Rec)
    : {};
const num = (value: unknown): number =>
  typeof value === "number" && Number.isFinite(value) ? value : 0;
const text = (value: unknown): string => (typeof value === "string" ? value : "");
const entries = (value: unknown): Array<[string, Rec]> =>
  Object.entries(rec(value)).map(([key, item]) => [key, rec(item)]);

const counts = (value: Rec): Counts => ({
  messages: num(value.messages),
  input: num(value.input),
  output: num(value.output),
  cacheCreation: num(value.cacheCreation),
  cacheRead: num(value.cacheRead),
});

export const NO_FOLDER = "(no folder recorded)";

export function baseName(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** Everything a token counts: what was sent, what came back, what was written to the cache
 * and what was read from it. */
export const tokensOf = (value: Counts): number =>
  value.input + value.output + value.cacheCreation + value.cacheRead;

const sum = (rows: Counts[]): Counts =>
  rows.reduce<Counts>(
    (total, row) => ({
      messages: total.messages + row.messages,
      input: total.input + row.input,
      output: total.output + row.output,
      cacheCreation: total.cacheCreation + row.cacheCreation,
      cacheRead: total.cacheRead + row.cacheRead,
    }),
    { messages: 0, input: 0, output: 0, cacheCreation: 0, cacheRead: 0 },
  );

function projectsOf(sessions: SessionRow[]): ProjectRow[] {
  const byFolder = new Map<string, SessionRow[]>();
  for (const session of sessions) {
    const key = session.cwd || NO_FOLDER;
    byFolder.set(key, [...(byFolder.get(key) ?? []), session]);
  }
  return [...byFolder.entries()]
    .map(([cwd, rows]) => ({
      ...sum(rows),
      cwd,
      name: cwd === NO_FOLDER ? NO_FOLDER : baseName(cwd),
      sessions: rows.length,
    }))
    .sort((a, b) => tokensOf(b) - tokensOf(a) || a.cwd.localeCompare(b.cwd));
}

export function parseUsage(data: unknown): UsageView {
  const root = rec(data);
  const sessions = entries(root.bySession)
    .map(([id, value]) => ({
      ...counts(value),
      id,
      cwd: text(value.cwd),
      firstTs: text(value.firstTs),
      lastTs: text(value.lastTs),
    }))
    .sort((a, b) => tokensOf(b) - tokensOf(a) || a.id.localeCompare(b.id));
  const servers = entries(root.byMcpServer)
    .map(([name, value]) => ({
      name,
      calls: num(value.calls),
      tools: Object.entries(rec(value.tools))
        .map(([tool, calls]) => ({ name: tool, calls: num(calls) }))
        .sort((a, b) => b.calls - a.calls || a.name.localeCompare(b.name)),
    }))
    .sort((a, b) => b.calls - a.calls || a.name.localeCompare(b.name));
  const otel = rec(root.otel);
  const requests = rec(otel.apiRequests);
  const connections = rec(otel.mcpConnections);
  const sources = rec(root.sources);
  const index = rec(root.index);
  return {
    totals: counts(rec(root.totals)),
    days: entries(root.byDay)
      .map(([day, value]) => ({ ...counts(value), day }))
      .sort((a, b) => a.day.localeCompare(b.day)),
    models: entries(root.byModel)
      .map(([name, value]) => ({ ...counts(value), name }))
      .sort((a, b) => tokensOf(b) - tokensOf(a) || a.name.localeCompare(b.name)),
    sessions,
    projects: projectsOf(sessions),
    servers,
    failures: (Array.isArray(root.mcpFailures) ? root.mcpFailures : []).map((item) => ({
      session: text(rec(item).session),
      server: text(rec(item).server),
      ts: text(rec(item).ts),
    })),
    index: { files: num(index.files), messages: num(index.messages) },
    sources: {
      transcriptMessages: num(sources.transcriptMessages),
      otelRequests: num(sources.otelRequests),
      otelOnly: num(sources.otelOnly),
    },
    otel: {
      events: num(otel.events),
      costUsd: num(otel.costUsd),
      apiRequests: {
        count: num(requests.count),
        costUsd: num(requests.costUsd),
        durationMs: num(requests.durationMs),
      },
      toolDecisions: Object.entries(rec(otel.toolDecisions)).map(([name, count]) => [
        name,
        num(count),
      ]),
      connections: {
        total: num(connections.total),
        failures: (Array.isArray(connections.failures) ? connections.failures : []).map(
          (item) => ({ server: text(rec(item).server), count: num(rec(item).count) }),
        ),
      },
    },
  };
}

/** True while the index has never run: nothing was read from any transcript and no OTel
 * request was stored. */
export function isEmptyIndex(view: UsageView): boolean {
  return (
    view.index.files === 0 && view.index.messages === 0 && view.totals.messages === 0
  );
}

export const PERIODS = [7, 14, 30, 90] as const;
export type Period = (typeof PERIODS)[number];
export const DEFAULT_PERIOD: Period = 14;

const DAY = /^\d{4}-\d{2}-\d{2}$/;

export function addDays(day: string, delta: number): string {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(Date.UTC(y, m - 1, d + delta)).toISOString().slice(0, 10);
}

export const todayOf = (now: Date): string => now.toISOString().slice(0, 10);

/** The `period` days that end on `today`, one row per day, a day without messages as zeros. */
export function windowOf(days: DayRow[], today: string, period: number): DayRow[] {
  const byDay = new Map(days.map((row) => [row.day, row]));
  const empty = { messages: 0, input: 0, output: 0, cacheCreation: 0, cacheRead: 0 };
  if (!DAY.test(today)) return [];
  return Array.from({ length: period }, (_, i) => {
    const day = addDays(today, i - period + 1);
    return byDay.get(day) ?? { ...empty, day };
  });
}

/** The newest day the index holds a message for. */
export function newestDay(days: DayRow[]): string | null {
  return days.length > 0 ? days[days.length - 1].day : null;
}

export function share(part: number, whole: number): number | null {
  return whole > 0 ? part / whole : null;
}

export function percent(value: number | null): string {
  if (value === null) return "n/a";
  const pct = value * 100;
  return `${pct >= 99.95 || pct === 0 ? Math.round(pct) : pct.toFixed(1)}%`;
}

export const exact = (n: number): string => n.toLocaleString("en");

export function compact(n: number): string {
  const abs = Math.abs(n);
  const unit = (divisor: number, suffix: string) => {
    const value = n / divisor;
    return `${value >= 100 ? Math.round(value) : Math.round(value * 10) / 10}${suffix}`;
  };
  if (abs >= 1e9) return unit(1e9, "B");
  if (abs >= 1e6) return unit(1e6, "M");
  if (abs >= 1e3) return unit(1e3, "K");
  return String(n);
}

/** `2026-10-01T10:00:00Z` as `2026-10-01 10:00 UTC`; anything that is not a time stays as it is. */
export function formatTs(value: string): string {
  const ms = Date.parse(value);
  if (!value || Number.isNaN(ms)) return value || "unknown";
  return `${new Date(ms).toISOString().slice(0, 16).replace("T", " ")} UTC`;
}

export function latestTs(view: UsageView): string | null {
  const times = view.sessions
    .map((session) => session.lastTs)
    .filter((value) => !Number.isNaN(Date.parse(value)));
  if (times.length === 0) return null;
  return times.reduce((a, b) => (Date.parse(b) > Date.parse(a) ? b : a));
}

export interface Tick {
  value: number;
  label: string;
}

/** A top of the axis and three ticks (0, half, top) that are round numbers. */
export function axisOf(max: number): { top: number; ticks: Tick[] } {
  if (max <= 0) return { top: 1, ticks: [{ value: 0, label: "0" }] };
  const power = 10 ** Math.floor(Math.log10(max));
  const top = [1, 2, 4, 5, 10].map((step) => step * power).find((v) => v >= max) ?? max;
  return {
    top,
    ticks: [0, top / 2, top].map((value) => ({ value, label: compact(value) })),
  };
}

export function usageArgv(options: { refresh: boolean; root: string }): string[] {
  return [
    "usage",
    ...(options.refresh ? [] : ["--no-refresh"]),
    ...(options.root ? ["--root", options.root] : []),
  ];
}

export const STATUS_ARGV = ["obs", "otel", "status"];

export const enableArgv = (port: number) => [
  "obs",
  "otel",
  "enable",
  "--port",
  String(port),
];
export const DISABLE_ARGV = ["obs", "otel", "disable"];

/** A port as the CLI takes it: a whole number from 1 to 65535. */
export function parsePort(value: string): number | null {
  const trimmed = value.trim();
  if (!/^\d{1,5}$/.test(trimmed)) return null;
  const port = Number(trimmed);
  return port >= 1 && port <= 65535 ? port : null;
}

export interface Policy {
  tier: Tier;
  previewFlag: string | null;
  terminal: boolean;
}

/** What the registry says about a command; one it lacks, or marks as planned, has no policy
 * and the screen does not run it. */
export function policyOf(rows: CommandRow[] | null, id: string): Policy | null {
  const row = rows?.find((c) => c.kind === "command" && c.id === id);
  if (!row || row.planned || !row.tier) return null;
  return {
    tier: row.tier,
    previewFlag: row.preview?.mode === "flag" ? row.preview.flag : null,
    terminal: row.surface === "terminal" || row.needs.includes("terminal-only"),
  };
}

export interface OtelWrite {
  actions: string[];
  warnings: string[];
  endpoint: string;
  port: number;
  settingsPath: string;
  changed: boolean;
  kept: string[];
}

const asText = (value: unknown) =>
  typeof value === "string" ? value : (JSON.stringify(value) ?? "");

export function otelWriteOf(data: unknown): OtelWrite | null {
  const root = rec(data);
  if (!Array.isArray(root.actions)) return null;
  return {
    actions: root.actions.map(asText),
    warnings: (Array.isArray(root.warnings) ? root.warnings : []).map(asText),
    endpoint: text(root.endpoint),
    port: num(root.port),
    settingsPath: text(root.settingsPath),
    changed: root.changed === true,
    kept: (Array.isArray(root.kept) ? root.kept : []).map(asText),
  };
}

const STEP_OPS: Array<[RegExp, PlanOp]> = [
  [/: added$/, "create"],
  [/: updated$/, "update"],
  [/: removed$/, "delete"],
];

/** One of the CLI's own action lines, word for word, as a step of the plan. */
export function stepOf(action: string, path: string): PlanStep {
  const op = STEP_OPS.find(([pattern]) => pattern.test(action))?.[1] ?? "note";
  return { op, detail: action, ...(path ? { path } : {}) };
}

/** The dry run of `obs otel enable` or `disable` as a plan. The steps are the action lines
 * the CLI prints, so the preview is the dry run and nothing else. */
export function otelPlan(
  data: unknown,
  kind: "enable" | "disable",
  undo: string,
): PlanV1 | null {
  const write = otelWriteOf(data);
  if (!write) return null;
  const keys = write.actions.length;
  const summary =
    kind === "enable"
      ? `Send Claude Code telemetry to ${write.endpoint}: ${keys} keys in the env block of your Claude settings`
      : `Stop the receiver and remove the telemetry keys Toolport wrote (${keys} keys checked)`;
  const warnings = [...write.warnings];
  for (const key of write.kept)
    warnings.push(
      `env.${key} stays in your settings: you changed it after Toolport set it`,
    );
  return {
    summary,
    steps: write.actions.map((action) => stepOf(action, write.settingsPath)),
    effects: {},
    warnings,
    undo,
  };
}

export const ENABLE_NOTICE =
  "The receiver runs inside the Toolport gateway and listens on this computer only. Restart running Claude Code sessions after enabling so they pick up the new settings.";

/** Name of the receiver state for a person. */
export function receiverLabel(state: string): {
  label: string;
  tone: "success" | "warning" | "destructive" | "secondary";
} {
  switch (state) {
    case "listening":
      return { label: "Listening", tone: "success" };
    case "port-in-use":
      return { label: "Port in use", tone: "destructive" };
    case "stopped":
      return { label: "Stopped", tone: "warning" };
    case "disabled":
      return { label: "Off", tone: "secondary" };
    default:
      return { label: state || "Unknown", tone: "secondary" };
  }
}

export function keyLabel(state: string): {
  label: string;
  tone: "success" | "warning" | "secondary";
} {
  switch (state) {
    case "ok":
      return { label: "set", tone: "success" };
    case "differs":
      return { label: "set to another value", tone: "warning" };
    case "missing":
      return { label: "not set", tone: "secondary" };
    default:
      return { label: state, tone: "secondary" };
  }
}

export function settingsLabel(state: string): string {
  switch (state) {
    case "configured":
      return "Every telemetry key is set";
    case "partial":
      return "Some keys set";
    case "missing":
      return "No telemetry keys";
    case "unreadable":
      return "Settings file unreadable";
    default:
      return state;
  }
}

export interface OtelStatus {
  enabled: boolean;
  endpoint: string;
  port: number;
  events: { count: number; latest: string | null };
  receiver: { state: string; listening: boolean; error: string | null };
  settings: {
    exists: boolean;
    state: string;
    path: string;
    keys: Array<[string, string]>;
    warnings: string[];
    error: string | null;
  };
}

const orNull = (value: unknown) => (typeof value === "string" && value ? value : null);

/** `obs otel status` as the card shows it. Only these members are read: nothing else the
 * command prints reaches the screen. */
export function parseStatus(data: unknown): OtelStatus {
  const root = rec(data);
  const events = rec(root.events);
  const receiver = rec(root.receiver);
  const settings = rec(root.settings);
  return {
    enabled: root.enabled === true,
    endpoint: text(root.endpoint),
    port: num(root.port),
    events: { count: num(events.count), latest: orNull(events.latest) },
    receiver: {
      state: text(receiver.state),
      listening: receiver.listening === true,
      error: orNull(receiver.error),
    },
    settings: {
      exists: settings.exists === true,
      state: text(settings.state),
      path: text(settings.path),
      keys: Object.entries(rec(settings.keys)).map(([key, state]) => [key, text(state)]),
      warnings: (Array.isArray(settings.warnings) ? settings.warnings : []).map(asText),
      error: orNull(settings.error),
    },
  };
}
