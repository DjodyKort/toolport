import type {
  MissedRecord,
  RunStatus,
  RunTrigger,
  RunView,
  TaskRow,
  TaskTriggers,
} from "./data";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export const ms = (iso: string | null | undefined): number | null => {
  if (!iso) return null;
  const value = Date.parse(iso);
  return Number.isNaN(value) ? null : value;
};

export function clockTime(at: number): string {
  return new Date(at).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" });
}

const dayKey = (at: number) => new Date(at).toDateString();

/** "09:33" today, "Mon 16 Nov 22:07" otherwise, in the local time zone. */
export function stamp(at: number, now: number): string {
  if (dayKey(at) === dayKey(now)) return clockTime(at);
  const date = new Date(at).toLocaleDateString("en-GB", {
    weekday: "short",
    day: "numeric",
    month: "short",
  });
  return `${date} ${clockTime(at)}`;
}

/** "Today", "Yesterday" or "Monday 16 November". */
export function dayLabel(at: number, now: number): string {
  if (dayKey(at) === dayKey(now)) return "Today";
  if (dayKey(at) === dayKey(now - DAY)) return "Yesterday";
  return new Date(at).toLocaleDateString("en-GB", {
    weekday: "long",
    day: "numeric",
    month: "long",
  });
}

export function duration(valueMs: number | null): string {
  if (valueMs === null) return "";
  const seconds = Math.round(valueMs / 1000);
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return rest === 0 || minutes >= 10 ? `${minutes} min` : `${minutes} min ${rest} s`;
}

export function elapsed(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, "0")}`;
}

export function inWords(deltaMs: number): string {
  const minutes = Math.max(1, Math.round(Math.abs(deltaMs) / MINUTE));
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.round(minutes / 60);
  return hours < 48 ? `${hours} h` : `${Math.round(hours / 24)} days`;
}

export function ago(at: number, now: number): string {
  return `${inWords(now - at)} ago`;
}

const TRIGGER_LABEL: Record<RunTrigger, string> = {
  manual: "Manual",
  cli: "CLI",
  selfMcp: "Claude",
  schedule: "Scheduled",
  "catch-up": "Catch-up",
  onAuthFailure: "Login failed",
};

export const triggerLabel = (trigger: RunTrigger) => TRIGGER_LABEL[trigger] ?? trigger;

const STARTED_BY: Record<RunTrigger, string> = {
  manual: "you (Manual)",
  cli: "the terminal",
  selfMcp: "Claude, after you approved it",
  schedule: "the schedule",
  "catch-up": "the schedule, as a catch-up",
  onAuthFailure: "a failed login",
};

export const startedBy = (trigger: RunTrigger) => STARTED_BY[trigger] ?? trigger;

export function slotTimes(note: { firstDue: string; lastDue: string }, now: number) {
  const first = ms(note.firstDue);
  const last = ms(note.lastDue);
  return {
    first: first === null ? note.firstDue : stamp(first, now),
    last: last === null ? note.lastDue : stamp(last, now),
  };
}

export function catchUpSentence(run: Pick<RunView, "missed">, now: number): string {
  if (!run.missed) return "";
  const { first, last } = slotTimes(run.missed, now);
  const slots = run.missed.slots;
  return slots === 1
    ? `Toolport was not running when ${first} came due. One run covers it.`
    : `Toolport was not running for ${slots} slots. One run covers them: first due ${first}, last due ${last}.`;
}

export const isLive = (status: RunStatus) => status === "running" || status === "waiting";

export const stepOf = (run: RunView) => {
  const index = run.steps.findIndex(
    (s) => s.status === "running" || s.status === "waiting",
  );
  return index >= 0 ? index + 1 : null;
};

export function failedStep(run: RunView): { index: number; title: string } | null {
  const index = run.steps.findIndex((s) => s.status === "failed");
  return index >= 0 ? { index: index + 1, title: run.steps[index].title } : null;
}

export function runResult(run: RunView): string {
  switch (run.status) {
    case "ok":
      return "ok";
    case "failed": {
      const step = failedStep(run);
      return step ? `Failed step ${step.index} of ${run.steps.length}` : "Failed";
    }
    case "cancelled":
      return "Cancelled";
    case "waiting":
      return "Waiting for you";
    case "running":
      return "Running";
    default:
      return run.status;
  }
}

/** Describes a UTC cron the way the screen says it; `null` when it has no plain wording. The
 * hour of a daily or weekly slot is read from the next run so it comes out in local time. */
export function describeCron(cron: string, next: number | null): string | null {
  const parts = cron.trim().split(/\s+/);
  if (parts.length !== 5) return null;
  const [minute, hour, dom, month, dow] = parts;
  const everyMinute = /^\*\/(\d+)$/.exec(minute);
  if (hour === "*" && dom === "*" && month === "*" && dow === "*") {
    if (minute === "*") return "Every minute";
    if (everyMinute) return `Every ${everyMinute[1]} minutes`;
    if (/^\d+$/.test(minute)) return `Every hour at :${minute.padStart(2, "0")}`;
  }
  const hours = /^(?:\d+|\d+-\d+)\/(\d+)$|^\*\/(\d+)$/.exec(hour);
  if (/^\d+$/.test(minute) && hours && dom === "*" && month === "*" && dow === "*") {
    const every = hours[1] ?? hours[2];
    return `Every ${every} hours at :${minute.padStart(2, "0")}`;
  }
  if (/^\d+$/.test(minute) && /^\d+$/.test(hour) && dom === "*" && month === "*") {
    const at =
      next === null
        ? `${hour.padStart(2, "0")}:${minute.padStart(2, "0")} UTC`
        : clockTime(next);
    if (dow === "*") return `Every day at ${at}`;
    if (/^\d$/.test(dow) && next !== null) {
      return `${new Date(next).toLocaleDateString("en-GB", { weekday: "long" })}s at ${at}`;
    }
  }
  return null;
}

export function scheduleText(cron: string, next: number | null): string {
  return describeCron(cron, next) ?? `Cron ${cron} (UTC)`;
}

export const SECOND_FIELDS = 5;

export function triggerParts(triggers: TaskTriggers, next: number | null): string[] {
  const parts: string[] = [];
  if (triggers.schedule) {
    const text = scheduleText(triggers.schedule.cron, next);
    parts.push(triggers.schedule.autoRun ? text : `${text} · asks you first`);
  }
  if (!triggers.schedule) parts.push("Manual");
  if (triggers.cli) parts.push("CLI");
  if (triggers.selfMcp.enabled) parts.push("Claude may ask (you approve every run)");
  if (triggers.onAuthFailure.length > 0)
    parts.push(`when the login fails for ${triggers.onAuthFailure.join(", ")}`);
  return parts;
}

export type Tone = "danger" | "warning" | "success" | "info" | "off";

export interface RowState {
  key: "needs-you" | "failed" | "running" | "cannot-run" | "off" | "scheduled" | "manual";
  label: string;
  tone: Tone;
}

export function rowState(row: TaskRow): RowState {
  if (row.waiting) return { key: "needs-you", label: "Needs you", tone: "warning" };
  if (row.problems?.length)
    return { key: "cannot-run", label: "Cannot run", tone: "danger" };
  if (row.lastRun?.status === "running")
    return { key: "running", label: "Running", tone: "info" };
  if (!row.enabled) return { key: "off", label: "Off", tone: "off" };
  if (row.lastRun?.status === "failed")
    return { key: "failed", label: "Last run failed", tone: "danger" };
  if (row.triggers.schedule)
    return { key: "scheduled", label: "Scheduled", tone: "success" };
  return { key: "manual", label: "Manual", tone: "off" };
}

const ORDER: Record<RowState["key"], number> = {
  "needs-you": 0,
  "cannot-run": 1,
  failed: 2,
  running: 3,
  scheduled: 4,
  manual: 5,
  off: 6,
};

/** Needs you first, then what failed, then by next run; Off tasks last. */
export function sortRows(rows: TaskRow[]): TaskRow[] {
  return [...rows].sort((a, b) => {
    const rank = ORDER[rowState(a).key] - ORDER[rowState(b).key];
    if (rank !== 0) return rank;
    const next = (ms(a.nextRun) ?? Infinity) - (ms(b.nextRun) ?? Infinity);
    return next !== 0 && Number.isFinite(next) ? next : a.title.localeCompare(b.title);
  });
}

export function lastRunLine(row: TaskRow, run: RunView | undefined, now: number): string {
  if (!row.lastRun) return "Never run in Toolport";
  const at = ms(row.lastRun.startedAt) ?? now;
  const when = stamp(at, now);
  const tail = [
    row.lastRun.durationMs !== null ? duration(row.lastRun.durationMs) : "",
    run?.trigger === "selfMcp" ? "you ran it when asked" : "",
  ].filter(Boolean);
  switch (row.lastRun.status) {
    case "failed": {
      const step = run ? failedStep(run) : null;
      const where = step && run ? `step ${step.index} of ${run.steps.length}` : "";
      return ["Last run failed " + when, where, ...tail].filter(Boolean).join(" · ");
    }
    case "running":
      return `Running since ${when}`;
    case "waiting":
      return `Waiting for you since ${when}`;
    case "cancelled":
      return ["Last run cancelled " + when, ...tail].join(" · ");
    default:
      return ["Last run ok " + when, ...tail].join(" · ");
  }
}

export type TimelineItem =
  | { kind: "run"; at: number; run: RunView }
  | { kind: "skipped"; at: number; record: MissedRecord };

/** Runs and the slots that started no run, newest first. A caught-up record is left out: its
 * run is in the list as a Catch-up row. */
export function timeline(
  runs: RunView[],
  missed: MissedRecord[],
  showSkipped: boolean,
): TimelineItem[] {
  const items: TimelineItem[] = runs.map((run) => ({
    kind: "run",
    at: ms(run.startedAt) ?? 0,
    run,
  }));
  if (showSkipped) {
    for (const record of missed) {
      if (record.outcome === "caught-up") continue;
      items.push({
        kind: "skipped",
        at: ms(record.lastDue) ?? ms(record.at) ?? 0,
        record,
      });
    }
  }
  return items.sort((a, b) => b.at - a.at);
}

/** Enabled tasks that start on their own, which only works while Toolport is open. */
export function selfStarting(rows: TaskRow[]): TaskRow[] {
  return rows.filter(
    (r) => r.enabled && !r.problems?.length && r.triggers.schedule?.autoRun === true,
  );
}

export const plural = (n: number, one: string, many = `${one}s`) =>
  `${n} ${n === 1 ? one : many}`;

export const SLUG = /^[a-z0-9][a-z0-9-]{0,63}$/;

export function slugify(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

export type Preset = "hours" | "daily" | "weekly" | "custom";

export interface PresetFields {
  every: number;
  minute: number;
  hour: number;
  weekday: number;
}

export function cronOf(preset: Preset, f: PresetFields, custom: string): string {
  switch (preset) {
    case "hours":
      return `${f.minute} */${f.every} * * *`;
    case "daily":
      return `${f.minute} ${f.hour} * * *`;
    case "weekly":
      return `${f.minute} ${f.hour} * * ${f.weekday}`;
    default:
      return custom;
  }
}

export function presetOf(cron: string): { preset: Preset; fields: PresetFields } {
  const base: PresetFields = { every: 4, minute: 0, hour: 8, weekday: 1 };
  const parts = cron.trim().split(/\s+/);
  if (parts.length === 5 && /^\d+$/.test(parts[0])) {
    const minute = Number(parts[0]);
    const hourly = /^\*\/(\d+)$/.exec(parts[1]);
    if (hourly && parts[2] === "*" && parts[3] === "*" && parts[4] === "*")
      return { preset: "hours", fields: { ...base, minute, every: Number(hourly[1]) } };
    if (/^\d+$/.test(parts[1]) && parts[2] === "*" && parts[3] === "*") {
      const hour = Number(parts[1]);
      if (parts[4] === "*") return { preset: "daily", fields: { ...base, minute, hour } };
      if (/^[0-6]$/.test(parts[4]))
        return {
          preset: "weekly",
          fields: { ...base, minute, hour, weekday: Number(parts[4]) },
        };
    }
  }
  return { preset: "custom", fields: base };
}

export function utcHint(hour: number, minute: number, now: number): string {
  const at = new Date(now);
  at.setUTCHours(hour, minute, 0, 0);
  return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")} UTC is ${clockTime(at.getTime())} on this Mac now.`;
}

export const STEP_KINDS = [
  { type: "needs-you", label: "Needs you", hint: "pauses for you" },
  { type: "mcp", label: "Call a server tool", hint: "" },
  { type: "script", label: "Script", hint: "Code Mode is off" },
  { type: "exec", label: "Run a program", hint: "" },
  { type: "prompt", label: "Ask Claude", hint: "uses your plan" },
  { type: "secret-set", label: "Store a secret", hint: "name only" },
  { type: "restart-server", label: "Restart a server", hint: "" },
] as const;

export const kindLabel = (type: string) =>
  STEP_KINDS.find((k) => k.type === type || (type === "routine" && k.type === "script"))
    ?.label ?? type;
