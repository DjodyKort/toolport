import type {
  FolderStatusData,
  LoadItem,
  LoadsData,
  MeasuredSummary,
  Verified,
} from "./data";

export function tokens(value: number): string {
  return Math.round(value).toLocaleString("en-US");
}

/** `/Users/me/work/app` shown as `~/work/app` when the folder sits under the home folder. */
export function shortPath(path: string, home: string | null): string {
  if (home && (path === home || path.startsWith(`${home}/`))) {
    return `~${path.slice(home.length)}`;
  }
  return path;
}

export function homeOf(items: LoadItem[]): string | null {
  const own = items.find((i) => i.source === "user" && i.path);
  const at = own?.path?.indexOf("/.claude/") ?? -1;
  return own?.path && at > 0 ? own.path.slice(0, at) : null;
}

export const LAYERS = [
  "Org",
  "You",
  "Parent folder",
  "Repo",
  "This folder",
  "Local",
] as const;
export type Layer = (typeof LAYERS)[number];

const dirOf = (path: string) => path.slice(0, Math.max(0, path.lastIndexOf("/")));

/** The strip of the stack: where an instruction row comes from. `project` rows are in the
 * folder itself or in a parent of it; the repo root is not told apart from a parent here. */
export function layerOf(item: LoadItem, cwd: string): Layer {
  if (item.source === "policy" || item.source === "org") return "Org";
  if (item.source === "user") return "You";
  if (item.source === "local") return "Local";
  const dir = item.path ? dirOf(item.path) : "";
  if (dir === cwd || dir === `${cwd}/.claude`) return "This folder";
  return "Parent folder";
}

export function sourceLabel(item: LoadItem): string {
  switch (item.source) {
    case "policy":
    case "org":
      return "Org file";
    case "user":
      return item.kind === "rule"
        ? "Your rules · everywhere"
        : "Your memory · everywhere";
    case "local":
      return "Local · CLAUDE.local.md";
    case "plugin":
      return "Plugin";
    default:
      return item.kind === "rule" ? "Rule" : "Project";
  }
}

export interface Stack {
  /** Loaded at the start of every session, one switch each. */
  start: LoadItem[];
  /** Loads on demand: counted apart. */
  lazy: LoadItem[];
  /** Left out here, by Toolport or by other settings. */
  leftOut: LoadItem[];
  /** Not loaded here for another reason (a scoped layer that does not match). */
  notHere: LoadItem[];
  memory: LoadItem[];
}

export function splitStack(items: LoadItem[]): Stack {
  const memory = items.filter((i) => i.kind === "memory-index");
  const rest = items.filter((i) => !memory.includes(i));
  return {
    memory,
    leftOut: rest.filter((i) => i.leftOutBy !== null),
    lazy: rest.filter((i) => i.lazy && i.leftOutBy === null),
    notHere: rest.filter((i) => !i.loaded && !i.lazy && i.leftOutBy === null),
    start: rest.filter((i) => i.loaded && !i.lazy && i.leftOutBy === null),
  };
}

export function layerSums(items: LoadItem[], cwd: string): Array<[Layer, number]> {
  return LAYERS.map((layer) => [
    layer,
    items.filter((i) => layerOf(i, cwd) === layer).reduce((sum, i) => sum + i.tokens, 0),
  ]);
}

export interface Lever {
  item: LoadItem;
  saving: number;
}

/** The biggest instruction files Toolport may leave out here; only what the stack itemises. */
export function leversOf(items: LoadItem[], count = 3): Lever[] {
  return items
    .filter(
      (i) =>
        i.kind !== "memory-index" &&
        i.loaded &&
        !i.lazy &&
        i.leftOutBy === null &&
        i.path &&
        i.tokens > 0,
    )
    .sort((a, b) => b.tokens - a.tokens)
    .slice(0, count)
    .map((item) => ({ item, saving: item.tokens }));
}

export type HeadlineState =
  | { kind: "estimate"; total: number }
  | { kind: "measured"; total: number; measured: MeasuredSummary; notItemised: number }
  | { kind: "stale"; total: number; measured: MeasuredSummary; notItemised: number }
  | { kind: "projected"; total: number; measured: MeasuredSummary; notItemised: number };

/** One number and its basis. A measured total is never edited: after a change the number is a
 * projection (the measured total minus the estimated saving since), and a stale measurement keeps
 * its number with a word. */
export function headlineOf(
  status: FolderStatusData,
  loads: LoadsData | null,
): HeadlineState {
  const estimate = loads?.totalTokens ?? status.estimate.totalTokens;
  const measured = status.measured;
  if (!measured) return { kind: "estimate", total: estimate };
  const at = status.calibration?.estimated ?? estimate;
  const notItemised = Math.max(0, measured.total - at);
  if (!measured.stale)
    return { kind: "measured", total: measured.total, measured, notItemised };
  if (estimate !== at) {
    return {
      kind: "projected",
      total: Math.max(0, measured.total - (at - estimate)),
      measured,
      notItemised,
    };
  }
  return { kind: "stale", total: measured.total, measured, notItemised };
}

export type RowMark = "loaded" | "absent" | "mismatch" | null;

/** The row's mark from Claude Code's InstructionsLoaded log: the stack agrees with it
 * (`loaded` or `absent`) or does not (`mismatch`). */
export function markOf(item: LoadItem, verified: Verified | null | undefined): RowMark {
  if (!verified || item.lazy || !item.path) return null;
  if (verified.mismatches.some((m) => m.path === item.path)) return "mismatch";
  return item.loaded ? "loaded" : "absent";
}

export function plural(count: number, one: string, many = `${one}s`): string {
  return `${count} ${count === 1 ? one : many}`;
}

/** Number of folder-local controls Toolport holds in a folder. */
export function changesIn(status: FolderStatusData): number {
  return (
    status.leftOut.files.length +
    (status.leftOut.memoryOff ? 1 : 0) +
    status.pluginsOff.length +
    (status.bundle.applied ? 1 : 0)
  );
}
