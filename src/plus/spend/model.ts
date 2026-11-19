import type {
  Badge,
  DayRow,
  McpPart,
  McpPartReady,
  Money,
  PriceTable,
  RangeId,
  RangeTile,
  SetupRecord,
  SessionRow,
  SpendRow,
  StatusData,
  SummaryData,
  TokenCounts,
} from "./data";

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const MB = 1024 * 1024;

export const RANGES: { id: RangeId; label: string; days: number; key: string }[] = [
  { id: "today", label: "Today", days: 1, key: "1" },
  { id: "7d", label: "7 days", days: 7, key: "2" },
  { id: "30d", label: "30 days", days: 30, key: "3" },
];

export const DEFAULT_RANGE: RangeId = "7d";

export const INTERACTIVE = "Interactive";
export const UNATTRIBUTED = "Unattributed";
export const UNKNOWN_PROJECT = "Unknown";

const OTHER_ROWS = "other";
const PROJECT_ROWS = 5;
const MODEL_ROWS = 6;
const TASK_ROWS = 6;
const NEAR_FULL = 0.9;

export function money(usd: number): string {
  return `$${usd.toFixed(2)}`;
}

/** Breakdown rows show fewer digits as the figure grows: $164, $72.4, $0.30. */
export function moneyShort(usd: number): string {
  if (usd >= 100) return `$${Math.round(usd)}`;
  if (usd >= 1) return `$${usd.toFixed(1)}`;
  return `$${usd.toFixed(2)}`;
}

/** `millions` is the decimals of an "M" figure: one on the tiles and rows, two on the MCP card. */
export function tokens(count: number, millions = 1): string {
  if (count >= 1e9) return `${(count / 1e9).toFixed(2)}B`;
  if (count >= 1e6) return `${(count / 1e6).toFixed(millions)}M`;
  if (count >= 1e3) return `${(count / 1e3).toFixed(1)}k`;
  return String(count);
}

export const count = (n: number) => n.toLocaleString("en-US");

export function percent(fraction: number, digits = 0): string {
  return `${(fraction * 100).toFixed(digits)}%`;
}

/** "+12%" or "-5%"; null when there was no period to compare with. */
export function delta(fraction: number | null): string | null {
  if (fraction === null) return null;
  const value = Math.round(fraction * 100);
  return value === 0 ? "±0%" : `${value > 0 ? "+" : "-"}${Math.abs(value)}%`;
}

const parse = (iso: string | null | undefined): number | null => {
  if (!iso) return null;
  const at = Date.parse(iso);
  return Number.isNaN(at) ? null : at;
};

/** "2026-11-18" or an ISO time as "11-18", in UTC like the days of the report. */
export function monthDay(value: string): string {
  const at = value.length === 10 ? Date.parse(`${value}T00:00:00Z`) : Date.parse(value);
  if (Number.isNaN(at)) return value;
  return new Date(at).toISOString().slice(5, 10);
}

export function weekday(day: string): string {
  return new Date(`${day}T00:00:00Z`).toLocaleDateString("en-GB", {
    weekday: "short",
    timeZone: "UTC",
  });
}

export function age(iso: string | null, now: number): string | null {
  const at = parse(iso);
  if (at === null) return null;
  const gap = Math.max(0, now - at);
  if (gap < MINUTE) return `${Math.round(gap / SECOND)} s ago`;
  if (gap < HOUR) return `${Math.round(gap / MINUTE)} min ago`;
  if (gap < DAY) return `${Math.round(gap / HOUR)} h ago`;
  return `${Math.round(gap / DAY)} d ago`;
}

export function megabytes(bytes: number): string {
  return bytes < MB / 10
    ? `${(bytes / 1024).toFixed(1)} KB`
    : `${(bytes / MB).toFixed(1)}`;
}

/** The home folder as `~` in the paths Claude Code reports. */
export function tildePath(path: string): string {
  return path.replace(/^\/(?:Users|home)\/[^/]+/, "~");
}

/** The sentence that goes next to an estimate when its list prices were never checked against
 * Anthropic's published pricing; null once they were. */
export function unverifiedPriceNote(prices: PriceTable | undefined): string | null {
  if (!prices || prices.verified) return null;
  const list = prices.families
    .filter((p) => !p.verified)
    .map(
      (p) =>
        `${p.family[0].toUpperCase()}${p.family.slice(1)} $${p.inputUsd} in / $${p.outputUsd} out`,
    )
    .join(", ");
  return `The list prices behind these estimates (${list} per million tokens${
    prices.factorsVerified
      ? ""
      : `, cache reads at ${prices.cacheReadFactor}x and cache writes at ${prices.cacheWriteFactor}x`
  }) are not verified against Anthropic's published pricing yet.`;
}

const WEEKDAYS = [
  "Sunday",
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
];

/** "Every Monday at 09:00" for the one-day-a-week schedules the weekly measure uses; any other
 * schedule is shown as the cron text it is. */
export function weeklySchedule(cron: string | null): string {
  const match = /^(\d{1,2}) (\d{1,2}) \* \* ([0-6])$/.exec(cron ?? "");
  if (!match) return cron ?? "no schedule";
  const [, minute, hour, day] = match;
  return `Every ${WEEKDAYS[Number(day)]} at ${hour.padStart(2, "0")}:${minute.padStart(2, "0")}`;
}

/** Cost is an estimate as soon as any of it comes from list prices. */
export function rowBadge(cost: Money): Badge {
  return cost.estimatedUsd > 0 ? "Estimate" : cost.badge;
}

/* ---------------------------------------------------------------- state machine */

export type SpendState =
  | "neverSetUp"
  | "offWithTranscripts"
  | "waitingFirstEvent"
  | "computing"
  | "receiverCannotListen"
  | "healthy";

/** Where the page stands. `summaryReady` is false until the first report came back: while the
 * transcripts are still being read for the first time that is "computing", and the page shows the
 * count instead of numbers. */
export function spendState(status: StatusData, summaryReady: boolean): SpendState {
  const { files, done } = status.transcripts;
  if (!summaryReady && files > done) return "computing";
  if (status.state === "error" || status.state === "not-listening")
    return "receiverCannotListen";
  if (status.state === "waiting") return "waitingFirstEvent";
  if (status.state === "collecting") return "healthy";
  const nothingStored =
    files === 0 && status.store.events === 0 && status.store.dailyLines === 0;
  return nothingStored ? "neverSetUp" : "offWithTranscripts";
}

/** Per-task figures need the session ids that only OTel events carry. */
export const taskSplitAvailable = (summary: SummaryData) =>
  summary.sources.otelSessions > 0;

export type Tone = "ok" | "info" | "warning" | "danger" | "off";

export interface Strip {
  tone: Tone;
  title: string;
  /** The facts under the title. */
  detail: string;
  /** What the person can do next. */
  action: "setup" | "port" | "details" | "cancel" | null;
}

export function receiverLine(status: StatusData): string {
  const { receiver } = status;
  return receiver.listening ? `Receiver on ${receiver.address}` : "Receiver not running";
}

export function storeLine(status: StatusData): string {
  const { store } = status;
  return `${megabytes(store.bytes)} of ${Math.round(store.maxBytes / MB)} MB`;
}

export function strip(state: SpendState, status: StatusData, now: number): Strip {
  const { receiver } = status;
  switch (state) {
    case "neverSetUp":
      return {
        tone: "off",
        title: "Collecting is off",
        detail: "Numbers come from Claude Code's transcripts only.",
        action: "setup",
      };
    case "offWithTranscripts":
      return {
        tone: "off",
        title: "Collecting is off",
        detail: "Numbers come from Claude Code's transcripts only.",
        action: "setup",
      };
    case "waitingFirstEvent":
      return {
        tone: "warning",
        title: "Waiting for the first event",
        detail: `${receiverLine(status)} · ${receiver.events} events`,
        action: "details",
      };
    case "computing":
      return {
        tone: "info",
        title: "Reading transcripts",
        detail: `${count(status.transcripts.done)} of ${count(status.transcripts.files)} files`,
        action: "cancel",
      };
    case "receiverCannotListen":
      return {
        tone: "danger",
        title: "Receiver is not listening",
        detail: receiver.error ?? `Nothing answers on ${receiver.address}.`,
        action: "port",
      };
    case "healthy": {
      const last = age(status.lastEvent, now);
      const since = status.setup
        .map((s) => s.appliedAt)
        .sort()
        .at(0);
      return {
        tone: "ok",
        title: "Collecting",
        detail: [
          receiverLine(status),
          last ? `last event ${last}` : null,
          storeLine(status),
          since ? `since ${monthDay(since)}` : null,
        ]
          .filter(Boolean)
          .join(" · "),
        action: "details",
      };
    }
  }
}

export interface Notice {
  id: "not-exporting" | "dropped" | "skipped" | "size";
  tone: Tone;
  title: string;
  detail: string;
}

/** Warnings under the strip. Nothing is dropped or skipped silently. */
export function notices(
  summary: SummaryData | null,
  status: StatusData,
  now: number,
): Notice[] {
  const out: Notice[] = [];
  const stale = summary?.stale;
  if (stale && stale.sessionsSince > 0) {
    const last = age(status.lastEvent, now);
    out.push({
      id: "not-exporting",
      tone: "warning",
      title: "Claude Code is not exporting",
      detail: `Last event ${last ?? "never"}; ${stale.sessionsSince} ${
        stale.sessionsSince === 1 ? "session" : "sessions"
      } ran since (seen in transcripts).`,
    });
  }
  if (status.receiver.dropped > 0) {
    out.push({
      id: "dropped",
      tone: "warning",
      title: "Events were dropped",
      detail: `${count(status.receiver.dropped)} events were dropped because the receiver was busy.`,
    });
  }
  if (status.store.skippedLines > 0) {
    out.push({
      id: "skipped",
      tone: "warning",
      title: "Some stored lines were skipped",
      detail: `${count(status.store.skippedLines)} lines in events.jsonl were skipped (not valid JSON). The rest is still counted.`,
    });
  }
  if (
    status.store.maxBytes > 0 &&
    status.store.bytes / status.store.maxBytes >= NEAR_FULL
  ) {
    out.push({
      id: "size",
      tone: "warning",
      title: "The event store is nearly full",
      detail: `${storeLine(status)}. The oldest events are removed first, every hour.`,
    });
  }
  return out;
}

/** What the not-exporting strip lists as likely causes, with what was found. */
export function staleCauses(summary: SummaryData, status: StatusData): string[] {
  const stale = summary.stale;
  if (!stale) return [];
  const causes = [
    status.receiver.listening ? "receiver is listening" : "receiver is not running",
  ];
  for (const file of stale.projectOverrides)
    causes.push(`${tildePath(file)} sets OTEL_EXPORTER_OTLP_ENDPOINT to another address`);
  if (stale.startedBeforeSetup > 0)
    causes.push(
      `${stale.startedBeforeSetup} sessions started before setup (restart them)`,
    );
  return causes;
}

/* ---------------------------------------------------------------- range tiles */

export interface Tile {
  id: RangeId;
  label: string;
  /** The headline: dollars, or tokens when the person turned cost off. */
  headline: string;
  badge: Badge;
  /** Tokens, with "so far" on today. */
  detail: string;
  /** "+12% vs the 7 days before", "Yesterday $56.20 in total", the transcript share. */
  notes: string[];
}

function tileNotes(id: RangeId, tile: RangeTile, showCost: boolean): string[] {
  const notes: string[] = [];
  if (id === "today") {
    if (showCost) notes.push(`Yesterday ${money(tile.previousUsd)} in total`);
  } else {
    const change = showCost ? delta(tile.costDelta) : null;
    if (change) notes.push(`${change} vs the ${tile.days} days before`);
  }
  if (tile.estimatedDays > 0 && id !== "today") {
    notes.push(
      tile.estimatedDays >= tile.days
        ? "from transcripts only (estimate)"
        : `${tile.estimatedDays} of ${tile.days} days from transcripts (estimate)`,
    );
  }
  return notes;
}

export function tiles(summary: SummaryData, showCost: boolean): Tile[] {
  return RANGES.map(({ id, label }) => {
    const tile = summary.tiles[id];
    return {
      id,
      label,
      headline: showCost ? money(tile.cost.usd) : `${tokens(tile.tokens)} tokens`,
      badge:
        showCost && tile.estimatedDays >= tile.days ? rowBadge(tile.cost) : "Measured",
      detail: showCost
        ? `${tokens(tile.tokens)} tokens${id === "today" ? " · so far" : ""}`
        : id === "today"
          ? "so far"
          : "",
      notes: tileNotes(id, tile, showCost),
    };
  });
}

/* ---------------------------------------------------------------- cost by day */

export type ModelFamily = "Opus" | "Sonnet" | "Haiku" | "Other";

export function modelFamily(model: string): ModelFamily {
  const name = model.toLowerCase();
  if (name.includes("opus")) return "Opus";
  if (name.includes("sonnet")) return "Sonnet";
  if (name.includes("haiku")) return "Haiku";
  return "Other";
}

export const FAMILIES: ModelFamily[] = ["Opus", "Sonnet", "Haiku", "Other"];

export interface Segment {
  family: ModelFamily;
  usd: number;
}

export interface Bar {
  day: string;
  weekday: string;
  monthDay: string;
  usd: number;
  tokens: number;
  /** Height against the tallest bar, 0 to 1. */
  height: number;
  segments: Segment[];
  estimatedUsd: number;
  /** 0 to 1: the part of the bar drawn hatched. */
  hatched: number;
  /** "$6.9 est." for a day that is estimate all through, "$38.6" otherwise. */
  label: string;
  /** The last day is still running. */
  soFar: boolean;
}

const dayStart = (iso: string) => Date.parse(`${iso.slice(0, 10)}T00:00:00Z`);

const dayKey = (at: number) => new Date(at).toISOString().slice(0, 10);

/** One bar per day of the window, empty days included, stacked by model family. Days come as
 * UTC dates, like the report's. */
export function bars(summary: SummaryData, now: number, showCost = true): Bar[] {
  const length = RANGES.find((r) => r.id === summary.range)?.days ?? 7;
  const today = dayStart(dayKey(now));
  const byDay = new Map<string, DayRow>(summary.byDay.map((row) => [row.key, row]));
  const raw = Array.from({ length }, (_, index) => {
    const day = dayKey(today - (length - 1 - index) * DAY);
    const row = byDay.get(day);
    const families = new Map<ModelFamily, number>();
    for (const [model, usd] of Object.entries(row?.byModel ?? {}))
      families.set(modelFamily(model), (families.get(modelFamily(model)) ?? 0) + usd);
    const segments: Segment[] = FAMILIES.filter((f) => families.has(f)).map((family) => ({
      family,
      usd: families.get(family) ?? 0,
    }));
    return { day, row, segments };
  });
  const basis = (row: DayRow | undefined) =>
    showCost ? (row?.cost.usd ?? 0) : (row?.tokens.total ?? 0);
  const tallest = Math.max(0, ...raw.map(({ row }) => basis(row)));
  return raw.map(({ day, row, segments }, index) => {
    const usd = row?.cost.usd ?? 0;
    const estimatedUsd = row?.cost.estimatedUsd ?? 0;
    const hatched = usd > 0 ? Math.min(1, estimatedUsd / usd) : 0;
    return {
      day,
      weekday: weekday(day),
      monthDay: monthDay(day),
      usd,
      tokens: row?.tokens.total ?? 0,
      height: tallest > 0 ? basis(row) / tallest : 0,
      segments,
      estimatedUsd,
      hatched,
      label: hatched >= 0.999 ? `${moneyShort(usd)} est.` : moneyShort(usd),
      soFar: index === raw.length - 1,
    };
  });
}

export interface TableRow {
  day: string;
  cost: string;
  tokens: string;
  basis: string;
}

/** The chart as a table: its accessible twin, and the "T" view. */
export function chartTable(chart: Bar[]): TableRow[] {
  return chart.map((bar) => ({
    day: `${bar.weekday} ${bar.monthDay}${bar.soFar ? " (so far)" : ""}`,
    cost: moneyShort(bar.usd),
    tokens: tokens(bar.tokens),
    basis:
      bar.usd === 0
        ? ""
        : bar.hatched >= 0.999
          ? "Estimate"
          : bar.hatched > 0
            ? "Part estimate"
            : "Measured",
  }));
}

/** The models that appear in the chart, for its legend. */
export function legend(chart: Bar[]): ModelFamily[] {
  const seen = new Set(chart.flatMap((bar) => bar.segments.map((s) => s.family)));
  return FAMILIES.filter((f) => seen.has(f));
}

/* ---------------------------------------------------------------- cache vs fresh */

export interface CacheLine {
  id: "cacheRead" | "cacheCreation" | "input" | "output";
  label: string;
  tokens: number;
  text: string;
  /** Share of all tokens, as text with one decimal. */
  share: string;
}

export function cacheLines(counts: TokenCounts): CacheLine[] {
  const total = counts.total || 1;
  const line = (id: CacheLine["id"], label: string, value: number): CacheLine => ({
    id,
    label,
    tokens: value,
    text: tokens(value),
    share: percent(value / total, 1),
  });
  return [
    line("cacheRead", "Cache read", counts.cacheRead),
    line("cacheCreation", "Cache write", counts.cacheCreation),
    line("input", "Fresh input", counts.input),
    line("output", "Output", counts.output),
  ];
}

/** Fresh tokens next to the cache share: "Fresh tokens: 4%". */
export function freshShare(cacheShare: number): string {
  return percent(1 - cacheShare);
}

/* ---------------------------------------------------------------- breakdown rows */

export type BreakdownKind = "project" | "model" | "task";

export interface BreakdownRow {
  key: string;
  label: string;
  /** Dollars, or tokens when the person switched dollar figures off. */
  cost: string;
  share: string;
  sub: string;
  badge: Badge;
  /** Enter on the row narrows the page to it. */
  filterable: boolean;
  /** Bar length against the largest row, 0 to 1. */
  fraction: number;
  /** Rolled-up remainder row. */
  rollup: boolean;
}

function sessions(n: number): string {
  return `${count(n)} ${n === 1 ? "session" : "sessions"}`;
}

function rowFor(
  kind: BreakdownKind,
  row: SpendRow,
  showCost: boolean,
  top: number,
): BreakdownRow {
  const base = {
    key: row.key,
    fraction: top > 0 ? Math.min(1, row.share / top) : 0,
    cost: showCost ? moneyShort(row.cost.usd) : tokens(row.tokens.total),
    share: percent(row.share),
    badge: rowBadge(row.cost),
    rollup: false,
  };
  if (kind === "project") {
    return {
      ...base,
      label: row.key === UNKNOWN_PROJECT ? "Unknown folder" : tildePath(row.key),
      sub: sessions(row.sessions),
      filterable: row.key !== UNKNOWN_PROJECT,
    };
  }
  if (kind === "model") {
    return {
      ...base,
      label: modelFamily(row.key) === "Other" ? row.key : modelFamily(row.key),
      sub: `${row.key} · ${count(row.requests)} requests`,
      filterable: true,
    };
  }
  if (row.key === INTERACTIVE)
    return {
      ...base,
      label: "Interactive (no task)",
      sub: sessions(row.sessions),
      filterable: true,
    };
  if (row.key === UNATTRIBUTED)
    return {
      ...base,
      label: UNATTRIBUTED,
      sub: "history without a session id",
      filterable: true,
    };
  return {
    ...base,
    label: row.key,
    sub: `task · ${sessions(row.sessions)}`,
    filterable: true,
  };
}

/** The first `limit` rows and one "N other ..." row for the rest. */
export function breakdown(
  kind: BreakdownKind,
  rows: SpendRow[],
  options: { limit?: number; showCost?: boolean } = {},
): BreakdownRow[] {
  const showCost = options.showCost ?? true;
  const limit =
    options.limit ??
    (kind === "project" ? PROJECT_ROWS : kind === "model" ? MODEL_ROWS : TASK_ROWS);
  const top = Math.max(0, ...rows.map((row) => row.share));
  const shown = rows.slice(0, limit).map((row) => rowFor(kind, row, showCost, top));
  const rest = rows.slice(limit);
  if (rest.length === 0) return shown;
  const usd = rest.reduce((sum, row) => sum + row.cost.usd, 0);
  const share = rest.reduce((sum, row) => sum + row.share, 0);
  const noun = kind === "project" ? "folders" : kind === "model" ? "models" : "tasks";
  return [
    ...shown,
    {
      key: OTHER_ROWS,
      label: `${rest.length} other ${noun}`,
      cost: showCost
        ? moneyShort(usd)
        : tokens(rest.reduce((sum, row) => sum + row.tokens.total, 0)),
      share: percent(share),
      fraction: top > 0 ? Math.min(1, share / top) : 0,
      sub: sessions(rest.reduce((sum, row) => sum + row.sessions, 0)),
      badge: rest.some((row) => row.cost.estimatedUsd > 0) ? "Estimate" : "Measured",
      filterable: false,
      rollup: true,
    },
  ];
}

/** The by-task card says Estimate: a session is matched to a run by id and start time. */
export const TASK_BADGE: Badge = "Estimate";

/* ---------------------------------------------------------------- filter */

export type FilterKind = BreakdownKind;

export interface Filter {
  project: string | null;
  model: string | null;
  task: string | null;
}

export const NO_FILTER: Filter = { project: null, model: null, task: null };

/** Picking the row that is already the filter clears it; another row of the same list replaces
 * it; filters of different lists combine. */
export function toggleFilter(filter: Filter, kind: FilterKind, key: string): Filter {
  return { ...filter, [kind]: filter[kind] === key ? null : key };
}

export const isFiltered = (filter: Filter) =>
  filter.project !== null || filter.model !== null || filter.task !== null;

export interface FilterChip {
  kind: FilterKind;
  key: string;
  label: string;
}

export function filterChips(filter: Filter): FilterChip[] {
  const chips: FilterChip[] = [];
  if (filter.project)
    chips.push({
      kind: "project",
      key: filter.project,
      label: tildePath(filter.project),
    });
  if (filter.model)
    chips.push({
      kind: "model",
      key: filter.model,
      label:
        modelFamily(filter.model) === "Other" ? filter.model : modelFamily(filter.model),
    });
  if (filter.task)
    chips.push({
      kind: "task",
      key: filter.task,
      label: filter.task === INTERACTIVE ? "Interactive (no task)" : filter.task,
    });
  return chips;
}

/** "Filtered. 41 of 142 sessions, 39% of the spend." `whole` is the same range without filter. */
export function filterNote(
  filtered: SummaryData,
  whole: SummaryData | null,
): string | null {
  if (filtered.project === null && filtered.model === null && filtered.task === null)
    return null;
  const mine = filtered.totals;
  if (!whole) return `Filtered. ${sessions(mine.sessions)}.`;
  const share = whole.totals.cost.usd > 0 ? mine.cost.usd / whole.totals.cost.usd : 0;
  return `Filtered. ${mine.sessions} of ${whole.totals.sessions} sessions, ${percent(share)} of the spend.`;
}

/** "3 h 05 m", "18 min", "40 s". */
export function duration(ms: number): string {
  if (ms < MINUTE) return `${Math.round(ms / SECOND)} s`;
  const minutes = Math.round(ms / MINUTE);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, "0")} m`;
}

export interface SessionLine {
  id: string;
  started: string;
  source: string;
  /** The task's name; null for a session started by a person. */
  task: string | null;
  taskLabel: string;
  model: string;
  length: string;
  tokens: string;
  cost: string;
  badge: Badge;
}

export function sessionLines(list: SessionRow[], showCost = true): SessionLine[] {
  return list.map((row) => ({
    id: row.key,
    started: new Date(row.start).toLocaleString("en-GB", {
      weekday: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      timeZone: "UTC",
    }),
    source: row.source === "otel" ? "OTel" : "Transcript",
    task: row.task === INTERACTIVE || row.task === UNATTRIBUTED ? null : row.task,
    taskLabel: row.task === INTERACTIVE ? "interactive" : row.task.toLowerCase(),
    model: row.model
      ? `${modelFamily(row.model) === "Other" ? row.model : modelFamily(row.model)}${
          row.modelCount > 1 ? ` +${row.modelCount - 1}` : ""
        }`
      : "unknown",
    length: duration(row.lengthMs),
    tokens: tokens(row.tokens.total),
    cost: showCost ? money(row.cost.usd) : "",
    badge: rowBadge(row.cost),
  }));
}

/* ---------------------------------------------------------------- MCP part */

export interface FirstRequestSegment {
  id: "instructions" | "tools" | "rest";
  label: string;
  tokens: number;
  badge: Badge;
  fraction: number;
}

export interface McpCard {
  failed: string | null;
  toolDefinitions: {
    exposed: string;
    full: string;
    sessions: number;
    /** Tokens kept out of the context over the window: sessions times the difference. */
    saved: string;
    tokenizer: string;
  } | null;
  share: { text: string; detail: string; badge: Badge } | null;
  compression: {
    cut: string | null;
    calls: number;
    hooks: number;
    window: string | null;
    note: string;
  };
  /** The sidebar's saved-tokens number, shown only to say it is never added to the spend. */
  upstreamSaved: string | null;
  firstRequest: {
    total: string;
    folder: string;
    measured: string;
    model: string;
    segments: FirstRequestSegment[];
  } | null;
}

function sharePhrase(value: number): string {
  const pct = value * 100;
  return pct > 0 && pct < 1 ? "< 1%" : `≈ ${Math.round(pct)}%`;
}

function segments(
  part: McpPartReady,
  request: NonNullable<McpPartReady["firstRequest"]>,
) {
  const instructions = request.instructionTokens ?? 0;
  const tools = Math.min(
    part.toolDefinitions.medianExposedTokens,
    Math.max(0, request.total - instructions),
  );
  const rest = Math.max(0, request.total - instructions - tools);
  const each = (
    id: FirstRequestSegment["id"],
    label: string,
    value: number,
    badge: Badge,
  ) => ({
    id,
    label,
    tokens: value,
    badge,
    fraction: request.total > 0 ? value / request.total : 0,
  });
  return [
    ...(request.instructionTokens === null
      ? []
      : [each("instructions", "Instruction files", instructions, "Estimate")]),
    each("tools", "Tools (MCP)", tools, "Counted"),
    each("rest", "Everything else, not itemised", rest, "Not itemised"),
  ];
}

/** Claude Code reports no per-skill token figure, so skills sit inside the part that is not
 * itemised and the card says so. */
export const SKILLS_NOTE =
  "Skills, Claude Code's own prompt and its built-in tools are one part: Claude Code does not report them separately.";

export function mcpCard(part: McpPart, requests: number): McpCard {
  if (part.state === "error") {
    return {
      failed: part.error,
      toolDefinitions: null,
      share: null,
      compression: { cut: null, calls: 0, hooks: 0, window: null, note: "" },
      upstreamSaved: null,
      firstRequest: null,
    };
  }
  const defs = part.toolDefinitions;
  const withDefinitions = part.state === "ready";
  const share = part.shareOfInputTokens;
  const compression = part.compression;
  const window =
    compression.firstCall && compression.lastCall
      ? `${monthDay(compression.firstCall)} to ${monthDay(compression.lastCall)}`
      : null;
  return {
    failed: null,
    toolDefinitions: withDefinitions
      ? {
          exposed: tokens(defs.medianExposedTokens),
          full: tokens(defs.medianFullTokens),
          sessions: defs.sessions,
          saved: tokens(Math.max(0, defs.fullTokens - defs.exposedTokens), 2),
          tokenizer: defs.tokenizer,
        }
      : null,
    share:
      share.value === null
        ? null
        : {
            text: sharePhrase(share.value),
            detail: `${tokens(defs.exposedTokens)} of the input-side tokens: ${tokens(defs.medianExposedTokens)} per request × ${count(requests)} requests.`,
            badge: share.badge,
          },
    compression: {
      cut: compression.tokensCut === null ? null : tokens(compression.tokensCut, 2),
      calls: compression.calls,
      hooks: compression.hooks,
      window,
      note: compression.note,
    },
    upstreamSaved: part.savedTokens === null ? null : tokens(part.savedTokens, 1),
    firstRequest: part.firstRequest
      ? {
          total: count(part.firstRequest.total),
          folder: tildePath(part.firstRequest.cwd),
          measured: monthDay(part.firstRequest.measuredAt),
          model: part.firstRequest.model,
          segments: segments(part, part.firstRequest),
        }
      : null,
  };
}

/* ---------------------------------------------------------------- collecting card */

export interface CollectingCard {
  on: boolean;
  /** Size against the cap, 0 to 1. */
  storedFraction: number;
  /** Age of the oldest event against the retention days, 0 to 1; null without events. */
  keptFraction: number | null;
  /** Which cap the store reaches first at the current rate; null while there is too little to tell. */
  rate: string | null;
  receiver: string;
  source: string;
  stored: string;
  keptFor: string;
  dailyTotals: string;
  claudeCode: string;
  drops: string;
}

export function setupLine(setup: SetupRecord[]): string {
  if (setup.length === 0) return "No OpenTelemetry settings written by Toolport.";
  const first = setup[0];
  const where =
    setup.length === 1 ? tildePath(first.profile) : `${setup.length} profiles`;
  return `5 settings written in ${where} on ${monthDay(first.appliedAt)}`;
}

export function collectingCard(
  status: StatusData,
  now: number,
  closedDays: string[] = [],
): CollectingCard {
  const { receiver, store } = status;
  const oldest = parse(store.oldest);
  const kept = oldest === null ? 0 : Math.max(0, Math.round((now - oldest) / DAY));
  const perDay = kept >= 2 ? store.bytes / kept : null;
  const projected = perDay === null ? null : perDay * store.retentionDays;
  const rate =
    projected === null
      ? null
      : projected <= store.maxBytes
        ? `At this rate the ${store.retentionDays}-day limit is reached first (about ${Math.round(projected / MB)} MB), not the size limit. Oldest events are removed first, every hour.`
        : "At this rate the size limit is reached first, so the oldest events are removed early. Daily totals are unaffected.";
  const closed = closedDays.length > 0 ? ` (${closedDays.map(monthDay).join(", ")})` : "";
  return {
    on: status.enabled,
    storedFraction: store.maxBytes > 0 ? Math.min(1, store.bytes / store.maxBytes) : 0,
    keptFraction:
      oldest === null || store.retentionDays <= 0
        ? null
        : Math.min(1, kept / store.retentionDays),
    rate,
    receiver: receiver.listening
      ? `Listening on ${receiver.address} (this computer only) · started with the ${
          receiver.owner === "gateway" ? "gateway" : "Toolport app"
        }`
      : (receiver.error ?? "Not running."),
    source: status.enabled
      ? `Claude Code's OpenTelemetry export; transcripts fill the days before and the days the app was closed${closed}.`
      : "Claude Code's transcripts.",
    stored:
      store.events === 0 && store.dailyLines === 0
        ? "No spend data on this computer."
        : `${storeLine(status)} · ${count(store.events)} events`,
    keptFor:
      oldest === null
        ? ""
        : `${kept} of ${store.retentionDays} days · oldest ${monthDay(store.oldest ?? "")}`,
    dailyTotals: `${count(store.dailyLines)} lines kept after events are pruned`,
    claudeCode: setupLine(status.setup),
    drops: `${count(receiver.dropped)} events dropped`,
  };
}

/* ---------------------------------------------------------------- first-run progress */

export interface Progress {
  title: string;
  detail: string;
  /** 0 to 1; null when there are no files to count. */
  fraction: number | null;
}

export function progress(status: StatusData, elapsedMs: number): Progress {
  const { files, done } = status.transcripts;
  const seconds = Math.round(elapsedMs / SECOND);
  return {
    title: "Reading transcripts",
    detail:
      files === 0
        ? "Looking for transcripts"
        : `${count(done)} of ${count(files)} files · ${seconds} s on the first run, then cached per file`,
    fraction: files === 0 ? null : done / files,
  };
}

/** Set-up copy for the plan dialog: where each key is written. */
export function profileName(path: string): string {
  return tildePath(path);
}

/** What `spend config set` accepts. */
export const SETTING_BOUNDS = {
  retentionDays: { min: 7, max: 90 },
  maxMb: { min: 16, max: 256 },
};

/** The whole number inside the bounds, or null when the text is not plain digits. */
export function boundedInt(text: string, min: number, max: number): number | null {
  const value = Number(text);
  return /^\d+$/.test(text.trim()) && value >= min && value <= max ? value : null;
}
