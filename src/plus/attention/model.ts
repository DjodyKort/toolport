import type { CommandRow } from "../bridge/data";
import type { PlusView } from "../nav";
import type { AttentionAction, AttentionItem, AttentionLevel } from "../types/attention";

export type Tone = "bad" | "warn" | "mut";

/** The three groups of the mockup, in the order the list shows them. */
export const LEVELS: Array<{ level: AttentionLevel; label: string; tone: Tone }> = [
  { level: "needs-you", label: "Needs you", tone: "bad" },
  { level: "look", label: "Worth a look", tone: "warn" },
  { level: "fyi", label: "For your information", tone: "mut" },
];

export function groupItems(
  items: AttentionItem[],
): Record<AttentionLevel, AttentionItem[]> {
  const groups: Record<AttentionLevel, AttentionItem[]> = {
    "needs-you": [],
    look: [],
    fyi: [],
  };
  for (const item of items) groups[item.level].push(item);
  return groups;
}

const ROUTES: Record<string, PlusView> = {
  servers: "control",
  control: "control",
  logins: "logins",
  library: "library",
  context: "context",
  tokens: "tokens",
  tasks: "tasks",
  system: "system",
};

/** The view a row's `target.route` opens; a route this build does not know has no link. */
export function viewOfRoute(route: string): PlusView | null {
  return ROUTES[route] ?? null;
}

/** The argv a row's action runs through the bridge: everything after `toolportctl`. Anything
 * that does not start with it is not an action of ours. */
export function actionArgs(action: AttentionAction): string[] | null {
  const [program, ...args] = action.command;
  return program === "toolportctl" && args.length > 0 ? args : null;
}

/** The registry row whose path is the longest prefix of the argv (`task resume run-1` is the
 * row `task resume`). Its policy decides how the action runs; no row, no run. */
export function commandOf(rows: CommandRow[] | null, args: string[]): CommandRow | null {
  let best: CommandRow | null = null;
  let bestLength = 0;
  for (const row of rows ?? []) {
    if (row.kind !== "command") continue;
    const path = row.id.split(" ");
    if (path.length > args.length || path.length <= bestLength) continue;
    if (path.every((word, i) => args[i] === word)) {
      best = row;
      bestLength = path.length;
    }
  }
  return best;
}

export type DismissChoice = "tomorrow" | "week" | "forever";

export const DISMISS_CHOICES: Array<{ id: DismissChoice; label: string }> = [
  { id: "tomorrow", label: "Until tomorrow" },
  { id: "week", label: "For a week" },
  { id: "forever", label: "Forever" },
];

const pad = (n: number) => String(n).padStart(2, "0");

/** The date a dismissed row returns (`--until`), as a local calendar date; `forever` has none. */
export function untilDate(choice: DismissChoice, now: Date = new Date()): string | null {
  if (choice === "forever") return null;
  const day = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  day.setDate(day.getDate() + (choice === "tomorrow" ? 1 : 7));
  return `${day.getFullYear()}-${pad(day.getMonth() + 1)}-${pad(day.getDate())}`;
}

export function dismissArgv(id: string, until: string | null): string[] {
  return ["attention", "dismiss", id, ...(until ? ["--until", until] : [])];
}

/** How long a row has been waiting: "just now", "5 min", "3 h", "2 d". */
export function ageText(since: string, now: number = Date.now()): string {
  const then = Date.parse(since);
  if (Number.isNaN(then)) return "";
  const minutes = Math.max(0, Math.floor((now - then) / 60_000));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min`;
  if (minutes < 60 * 48) return `${Math.floor(minutes / 60)} h`;
  return `${Math.floor(minutes / (60 * 24))} d`;
}
