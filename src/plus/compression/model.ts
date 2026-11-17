import type {
  CompressionStatusData,
  EntryHealth,
  HookCounter,
  HookEntry,
  HookProviderId,
  HooksStatusData,
  Interval,
  PeriodState,
  Trust,
} from "./data";

export const HOOK_IDS: HookProviderId[] = ["rtk", "read-once"];

export const HOOK_INFO: Record<
  HookProviderId,
  { summary: string; matcher: string; wrapper: string }
> = {
  rtk: {
    summary: "Shortens noisy Bash output before it enters context.",
    matcher: "Bash",
    wrapper: "toolportctl compression hook rtk",
  },
  "read-once": {
    summary: "Blocks a repeated Read of a file that has not changed since the last read.",
    matcher: "Read",
    wrapper: "toolportctl compression hook read-once",
  },
};

export const PARSEC_CONSENT =
  "toolportctl compression enable --provider parsec --accept-egress parsec";

export const PROXY_COMMANDS = [
  "toolportctl compression proxy up",
  "toolportctl compression proxy down",
  "toolportctl compression proxy restart",
];

/** A worse health outranks a better one when a provider is in several settings files; a file
 * without the entry says nothing unless every file lacks it. */
const WORST: EntryHealth[] = ["missing", "edited", "foreign", "ok"];

export interface HookRow {
  provider: HookProviderId;
  health: EntryHealth;
  /** Settings files that hold the entry. */
  files: number;
  command: string | null;
  wrappedAt: string | null;
  counter: HookCounter | null;
}

export function overall(entries: HookEntry[]): EntryHealth {
  for (const health of WORST) {
    if (entries.some((e) => e.health === health)) return health;
  }
  return "absent";
}

export function hookRows(status: HooksStatusData): HookRow[] {
  return HOOK_IDS.map((provider) => {
    const entries = status.entries.filter((e) => e.provider === provider);
    const health = overall(entries);
    const shown = entries.find((e) => e.health === health) ?? entries[0];
    return {
      provider,
      health,
      files: entries.filter((e) => e.health !== "absent").length,
      command: shown?.command ?? null,
      wrappedAt: entries.find((e) => e.wrappedAt)?.wrappedAt ?? null,
      counter: status.counters.find((c) => c.provider === provider) ?? null,
    };
  });
}

export type HeadlineKind = "attention" | "paused" | "adopt" | "on" | "none";

export interface Headline {
  kind: HeadlineKind;
  owned: number;
  foreign: HookProviderId[];
  broken: HookProviderId[];
}

/** One leading card per tab: what needs fixing first, then adopting, then the plain state. */
export function headline(rows: HookRow[], paused: boolean): Headline {
  const ofHealth = (...healths: EntryHealth[]) =>
    rows.filter((r) => healths.includes(r.health)).map((r) => r.provider);
  const broken = ofHealth("missing", "edited");
  const foreign = ofHealth("foreign");
  const owned = rows.filter((r) => r.health === "ok" || r.health === "edited").length;
  const kind: HeadlineKind =
    broken.length > 0
      ? "attention"
      : paused && owned > 0
        ? "paused"
        : foreign.length > 0
          ? "adopt"
          : owned > 0
            ? "on"
            : "none";
  return { kind, owned, foreign, broken };
}

const clock = (at: Date) =>
  at.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });

/** `today 09:12`, `tomorrow 09:12` or `Mon 23 Nov 09:12`: when a pause resumes by itself. */
export function untilText(iso: string, now: Date = new Date()): string {
  const at = new Date(iso);
  if (Number.isNaN(at.getTime())) return iso;
  const days = Math.round(
    (new Date(at.getFullYear(), at.getMonth(), at.getDate()).getTime() -
      new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()) /
      86_400_000,
  );
  const day =
    days === 0
      ? "today"
      : days === 1
        ? "tomorrow"
        : at.toLocaleDateString("en-GB", {
            weekday: "short",
            day: "numeric",
            month: "short",
          });
  return `${day} ${clock(at)}`;
}

export const checkedAt = (at: Date = new Date()) => clock(at);

export function latencyText(counter: HookCounter | null): string {
  if (!counter || counter.calls === 0) {
    return "No calls counted yet. The hook counts from the next Claude Code session.";
  }
  const since = counter.firstCall ? ` since ${counter.firstCall}` : "";
  const median =
    counter.medianLatencyMs === null ? "" : ` · median ${counter.medianLatencyMs} ms`;
  const p95 = counter.p95LatencyMs === null ? "" : ` · p95 ${counter.p95LatencyMs} ms`;
  const failed = counter.failed > 0 ? ` · ${counter.failed} failed` : "";
  return `Answered ${counter.answered.toLocaleString("en-US")} of ${counter.calls.toLocaleString("en-US")} calls${since}${median}${p95}${failed} (counted at the hook)`;
}

export type TileState =
  "active" | "paused" | "not-managed" | "attention" | "not-found" | "off" | "locked";

export interface Tile {
  id: "rtk" | "read-once" | "headroom" | "parsec";
  kind: "hook" | "proxy" | "hosted";
  summary: string;
  trust: string;
  state: TileState;
}

function trustLine(trust: Trust | undefined, kind: Tile["kind"]): string {
  if (!trust) return "";
  return [
    trust.closedEngine ? "Closed engine" : "Open source",
    trust.sendsCodeOffMachine ? "sends your code off this machine" : "runs locally",
    kind === "proxy"
      ? "changes the API route"
      : trust.installsGlobalHooks
        ? "installs a global hook entry"
        : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

const HOOK_TILE_STATE: Record<EntryHealth, TileState> = {
  ok: "active",
  foreign: "not-managed",
  edited: "attention",
  missing: "attention",
  absent: "not-found",
};

/** The four providers of the design. The hook tiles follow the entries in settings.json; the
 * proxy follows the policy; the hosted engine is locked until it is chosen with consent. */
export function providerTiles(
  hooks: HooksStatusData,
  compression: CompressionStatusData | null,
): Tile[] {
  const trust = (name: string) => hooks.trust.find((t) => t.provider === name)?.trust;
  const hookTile = (row: HookRow): Tile => ({
    id: row.provider,
    kind: "hook",
    summary: HOOK_INFO[row.provider].summary,
    trust: trustLine(trust("rtk-only"), "hook"),
    state: row.health === "ok" && hooks.paused ? "paused" : HOOK_TILE_STATE[row.health],
  });
  const parsec = trust("parsec");
  return [
    ...hookRows(hooks).map(hookTile),
    {
      id: "headroom",
      kind: "proxy",
      summary: "Local proxy that rewrites the request body. Optional.",
      trust: trustLine(trust("headroom"), "proxy"),
      state: compression?.provider === "headroom" ? "active" : "off",
    },
    {
      id: "parsec",
      kind: "hosted",
      summary: "Hosted compression engine.",
      trust: trustLine(parsec, "hosted"),
      state:
        compression?.provider === "parsec"
          ? "active"
          : parsec?.needsEgressConsent === false
            ? "off"
            : "locked",
    },
  ];
}

export const pct = (share: number) => `${Math.round(share * 100)}%`;

export const count = (value: number) => value.toLocaleString("en-US");

export function intervalText(interval: Interval): string {
  if (!interval.from && !interval.to) return "the whole period";
  if (!interval.from) return `before ${interval.to}`;
  if (!interval.to) return `since ${interval.from}`;
  return `${interval.from} to ${interval.to}`;
}

export const PERIOD_ORDER: PeriodState[] = ["on", "paused", "off"];

/** `118 sessions · median cost 100 · middle half 71 to 160` of one window. */
export function windowText(group: {
  sessions: number;
  relative: number | null;
  relativeLow: number | null;
  relativeHigh: number | null;
}): string {
  if (group.sessions === 0) return "No sessions with enough turns";
  const sessions = `${count(group.sessions)} ${group.sessions === 1 ? "session" : "sessions"}`;
  if (group.relative === null) return `${sessions} · no hooks-on window to compare with`;
  const half =
    group.relativeLow !== null && group.relativeHigh !== null
      ? ` · middle half ${count(group.relativeLow)} to ${count(group.relativeHigh)}`
      : "";
  return `${sessions} · median cost ${count(group.relative)}${half}`;
}

export function dayRange(from: string | null, to: string | null): string {
  if (from && to) return from === to ? from : `${from} to ${to}`;
  return from ?? to ?? "no dates";
}
