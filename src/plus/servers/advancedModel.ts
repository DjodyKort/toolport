import { plural } from "../settings/model";
import type { InteropData, ServerInfoData } from "./data";

export const MIN_LIMIT_MINUTES = 1;
export const MAX_LIMIT_MINUTES = 1_440;
const MINUTE_MS = 60_000;

export type Limit =
  { kind: "default" } | { kind: "ok"; ms: number } | { kind: "error"; message: string };

/** The call limit as typed, in whole or decimal minutes. Empty means the default; a value outside
 * 1 to 1,440 is explained, never clamped. */
export function parseLimit(text: string): Limit {
  const raw = text.trim().replace(",", ".");
  if (raw === "") return { kind: "default" };
  const minutes = /^\d*\.?\d+$/.test(raw) ? Number(raw) : Number.NaN;
  if (!Number.isFinite(minutes)) {
    return { kind: "error", message: "Enter a number of minutes, for example 90." };
  }
  if (minutes < MIN_LIMIT_MINUTES) {
    return {
      kind: "error",
      message: "At least 1 minute. A shorter wait belongs in the request timeout (Edit).",
    };
  }
  if (minutes > MAX_LIMIT_MINUTES) {
    return { kind: "error", message: "At most 1,440 minutes (24 hours)." };
  }
  return { kind: "ok", ms: Math.round(minutes * MINUTE_MS) };
}

/** Minutes as the field shows them: `90`, `1.5`. */
export function minutesText(ms: number): string {
  return String(Number((ms / MINUTE_MS).toFixed(3)));
}

/** `45 s`, `1 min 30 s`, `30 min`, `1 h 30 min`: seconds only below an hour. */
export function span(ms: number): string {
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds} s`;
  if (seconds < 3_600) {
    const [minutes, rest] = [Math.floor(seconds / 60), seconds % 60];
    return rest === 0 ? `${minutes} min` : `${minutes} min ${rest} s`;
  }
  const minutes = Math.round(seconds / 60);
  const [hours, rest] = [Math.floor(minutes / 60), minutes % 60];
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}

/** Whether a stored limit can be typed in the field: one under a minute only comes from the
 * command line and stays read-only. */
export function editable(ms: number | null): boolean {
  return ms === null || ms >= MINUTE_MS;
}

export interface Bridge {
  ceilingMs: number;
  /** `null` while the bridge keeps its base wait. */
  setBy: string | null;
}

/** What the bridge waits when this server's limit is `limitMs` (`null`: the default), from the
 * longest of the other enabled servers. The same rule as the backend's `adapter_ceiling`. */
export function bridgeWith(
  info: ServerInfoData,
  limitMs: number | null,
  name: string,
): Bridge {
  const { interop } = info;
  const window = info.requestTimeoutMs ?? interop.defaultRequestTimeoutMs;
  const cap =
    info.command === null ? window : Math.max(limitMs ?? interop.defaultLimitMs, window);
  const own = info.enabled ? cap : 0;
  const other = interop.otherLongest;
  const longest = Math.max(own, other?.ceilingMs ?? 0);
  if (longest + interop.bridgeGraceMs <= interop.bridgeBaseMs) {
    return { ceilingMs: interop.bridgeBaseMs, setBy: null };
  }
  return {
    ceilingMs: longest + interop.bridgeGraceMs,
    setBy: own >= (other?.ceilingMs ?? 0) ? name : (other?.name ?? name),
  };
}

export const MIN_BUDGET_SECONDS = 1;
export const MAX_BUDGET_SECONDS = 86_400;

export interface BudgetRow {
  key: number;
  tool: string;
  seconds: string;
}

export interface BudgetCheck {
  /** Rows that carry a budget, by tool name. */
  budgets: Record<string, number>;
  errors: Record<number, string>;
}

/** A row with no seconds carries no budget and is dropped on save; a seconds value outside 1 to
 * 86,400 is explained, never clamped. */
export function checkBudgets(rows: BudgetRow[]): BudgetCheck {
  const budgets: Record<string, number> = {};
  const errors: Record<number, string> = {};
  const seen = new Set<string>();
  for (const row of rows) {
    const tool = row.tool.trim();
    const raw = row.seconds.trim().replace(",", ".");
    if (tool === "" && raw === "") continue;
    if (tool === "") {
      errors[row.key] = "Give the tool's name.";
      continue;
    }
    if (tool.includes("=")) {
      errors[row.key] = "A tool name cannot contain =.";
      continue;
    }
    if (seen.has(tool)) {
      errors[row.key] = `${tool} is listed twice. One budget per tool.`;
      continue;
    }
    seen.add(tool);
    if (raw === "") continue;
    const seconds = /^\d*\.?\d+$/.test(raw) ? Number(raw) : Number.NaN;
    if (!Number.isFinite(seconds)) {
      errors[row.key] = "Enter a number of seconds, for example 600.";
    } else if (seconds < MIN_BUDGET_SECONDS) {
      errors[row.key] = "At least 1 second.";
    } else if (seconds > MAX_BUDGET_SECONDS) {
      errors[row.key] = "At most 86,400 seconds (24 hours).";
    } else {
      budgets[tool] = seconds;
    }
  }
  return { budgets, errors };
}

/** What `server edit` must be told to turn the stored budgets into `next`. */
export function budgetChanges(
  stored: Record<string, number>,
  next: Record<string, number>,
): Array<[string, number | null]> {
  const changes: Array<[string, number | null]> = [];
  for (const [tool, seconds] of Object.entries(next)) {
    if (stored[tool] !== seconds) changes.push([tool, seconds]);
  }
  for (const tool of Object.keys(stored)) if (!(tool in next)) changes.push([tool, null]);
  return changes;
}

export function budgetRows(stored: Record<string, number>): BudgetRow[] {
  return Object.entries(stored)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([tool, seconds], key) => ({ key, tool, seconds: String(seconds) }));
}

export interface Values {
  mode: ServerInfoData["mode"];
  declareClientCapabilities: boolean;
  forwardInstructions: boolean;
  serial: boolean;
  limitMs: number | null;
  toolBudgets: Record<string, number>;
}

export function valuesOf(info: ServerInfoData): Values {
  return {
    mode: info.mode,
    declareClientCapabilities: info.declareClientCapabilities,
    forwardInstructions: info.forwardInstructions,
    serial: info.serial,
    limitMs: info.maxRequestTimeoutMs,
    toolBudgets: info.toolBudgets,
  };
}

export function changedCount(values: Values): number {
  return (
    (values.mode === "auto" ? 0 : 1) +
    (values.declareClientCapabilities ? 1 : 0) +
    (values.forwardInstructions ? 1 : 0) +
    (values.serial ? 1 : 0) +
    (values.limitMs === null ? 0 : 1) +
    (Object.keys(values.toolBudgets).length === 0 ? 0 : 1)
  );
}

/** The collapsed row's line: what differs from the defaults, in the order of the open row. */
export function summaryLine(
  values: Values,
  interop: InteropData,
  local: boolean,
): string {
  const switches =
    (values.declareClientCapabilities ? 1 : 0) + (values.forwardInstructions ? 1 : 0);
  const parts = [
    values.mode === "search" ? "Search only" : "All tools listed",
    switches === 0
      ? "no interop switches"
      : `${switches} ${switches === 1 ? "switch" : "switches"} on`,
  ];
  if (values.serial) parts.push("one call at a time");
  const budgets = Object.keys(values.toolBudgets).length;
  if (local && budgets > 0) parts.push(plural(budgets, "tool budget"));
  if (local) parts.push(`calls up to ${span(values.limitMs ?? interop.defaultLimitMs)}`);
  if (changedCount(values) > 0) return parts.join(" · ");
  const [first, ...rest] = parts;
  return ["Defaults", first.toLowerCase(), ...rest].join(" · ");
}

export function toolsLine(mode: Values["mode"], tools: number | null): string {
  const count = tools === null ? "Its tools" : `Its ${plural(tools, "tool")}`;
  if (mode === "search") {
    return `${count} ${tools === 1 ? "is" : "are"} left out of the list for clients that list every tool. The model still finds them by searching and can call them by name. Clients that already discover tools by search see no difference.`;
  }
  return tools === null
    ? "Its tools are listed to every client, like any other server's."
    : `Its tools are listed to every client, like any other server's. ${plural(tools, "tool")} in the list.`;
}
