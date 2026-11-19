/** What the Setup & backup screens read from `toolportctl setup ...` and how they build its argv.
 * The Rust goldens under plus/fixtures/ctl-envelopes are the source of the shapes. */

export interface HeldBack {
  file: string;
  kind: "claude-user" | "instruction" | "memory";
  /** What the scan found, in words; never the text. */
  reason: string;
}

export interface SetupPart {
  id: string;
  label: string;
  count: number;
  default: boolean;
  sealed: boolean;
  error: string | null;
  notes: string[];
  fileSize?: number;
  info?: Record<string, unknown>;
  flagged?: string[];
  heldBack?: HeldBack[];
}

export interface RepoSummary {
  name: string;
  path: string;
  remote: string | null;
  branch: string | null;
  upstream: string | null;
  dirty: number;
  untracked: number;
  stashes: number;
  unpushed: number;
  neverPushed: string[];
  noRemote: boolean;
  worktrees: number;
  error: string | null;
}

export interface SessionRepo {
  name: string;
  path: string;
  sessions: number;
  fileSize: number;
}

export interface LastExport {
  file: string;
  madeAt: string;
  bytes: number;
  parts: Record<string, { count: number }>;
}

export interface PartsData {
  encryption: boolean;
  minPassphrase: number;
  lastExport: LastExport | null;
  roots?: string[];
  repos?: RepoSummary[];
  totalSize?: number;
  parts: SetupPart[];
}

export interface ExportData {
  dryRun: boolean;
  file: string | null;
  fileSize?: number;
  name: string;
  parts: { id: string; label?: string; count?: number; heldBack?: HeldBack[] }[];
  tree?: { name: string; fileSize: number; sealed?: boolean; count?: number }[];
  totalSize?: number;
  verified?: boolean;
  notIncluded?: string[];
  heldBackSealed?: boolean;
  skipped?: string[];
}

export interface PeekData {
  peek: {
    part: string;
    label: string;
    files: { name: string; json?: unknown; fileSize?: number }[];
  };
}

export type RowState = "add" | "here" | "conflict" | "needs" | "skip";

export interface RestoreRow {
  id: string;
  label: string;
  part: string;
  state: RowState;
  note: string;
  takeable?: boolean;
  refused?: boolean;
}

export interface RestoreStep {
  id: string;
  part: string;
  label: string;
  ok: boolean;
  note: string;
}

export interface RestoreData {
  applied: boolean;
  dryRun: boolean;
  plan: string;
  locked?: boolean;
  rows: RestoreRow[];
  summary: Record<RowState, number>;
  refused?: { id: string; label: string; note: string }[];
  /** Set once applied. */
  run?: string;
  steps?: RestoreStep[];
  failed?: number;
  needsYou?: { id: string; label: string; note: string }[];
}

export interface InspectPart {
  id: string;
  label: string;
  count: number;
  fileSize: number;
  sealed: boolean;
  standing: "ok" | "damaged" | "missing" | "unknown";
}

export interface InspectData {
  file: string;
  format: string;
  version: number;
  madeAt: string;
  machine: string;
  toolport: string;
  parts: InspectPart[];
  problems: string[];
  intact: boolean;
}

export type UndoResult = "done" | "kept" | "failed";

export interface UndoStep {
  what: string;
  ok: boolean;
  result: UndoResult;
  note: string;
}

export interface UndoData {
  run: string;
  dryRun: boolean;
  undone: boolean;
  failed: string[];
  steps: UndoStep[];
  left: { servers: string[]; secretValues: number; note: string };
}

export interface CheckFinding {
  id: string;
  kind: string;
  repo: string;
  note: string;
  commands: string[];
  acked: boolean;
}

export interface SinceRow {
  id: string;
  label: string;
  added: number;
  removed: number;
  changed: number;
  countWas: number;
  countNow: number;
  branchChanges?: number;
}

export interface CheckData {
  repos: number;
  blockers: CheckFinding[];
  amber: CheckFinding[];
  carry: { id: string; label: string; ticked: boolean }[];
  ready: boolean;
  open: { blockers: number; carry: number };
  lastExport: LastExport | null;
  since: { parts: SinceRow[] } | null;
  sessions: { count: number; fileSize: number; repos: SessionRepo[] };
}

export const STATE_LABEL: Record<RowState, string> = {
  add: "Will be added",
  here: "Already here",
  conflict: "Differs, yours is kept",
  needs: "Needs you",
  skip: "Skipped",
};

/** The part whose values need `--secrets`; it comes only from the checklist. */
export const SECRETS_PART = "secrets";
export const SESSIONS_PART = "history";

export interface SubChoice {
  id: string;
  label: string;
  help: string;
}

/** The sub-choices a part opens to; each is on until switched off, and goes out as `--without`. */
export const SUB_CHOICES: Record<string, SubChoice[]> = {
  claudeUser: [
    {
      id: "claude-instructions",
      label: "Instructions and rules",
      help: "~/.claude/CLAUDE.md and ~/.claude/rules/.",
    },
    {
      id: "claude-settings",
      label: "settings.json, secrets filtered",
      help: "Hooks, permissions and model are kept; values under env and any key that looks like a token are removed and named in the preview.",
    },
    {
      id: "claude-plugins",
      label: "Installed plugins and marketplaces",
      help: "A list of names and sources. The plugin files are installed again from the marketplaces.",
    },
    {
      id: "claude-own",
      label: "Your own agents, commands, output styles and keybindings",
      help: "Files you wrote in ~/.claude/.",
    },
  ],
  memory: [
    {
      id: "memory-auto",
      label: "Auto memory Claude wrote",
      help: "MEMORY.md and the topic files, kept per repository by its remote.",
    },
    {
      id: "memory-switches",
      label: "Per-folder switches",
      help: "Which repositories have memory turned off and which instruction files are left out. A restore only sets a switch that is not set here.",
    },
    {
      id: "memory-instructions",
      label: "Instruction files that git does not hold",
      help: "CLAUDE.local.md and untracked or ignored instruction files in your repositories.",
    },
  ],
};

export interface ExportChoice {
  parts: string[];
  /** Sub-choices that are switched off. */
  without: string[];
  roots: string[];
  worktrees: boolean;
  skipNoRemote: boolean;
  libraryZip: boolean;
  /** Repository names whose sessions go in; `null` means every repository that has some. */
  sessionRepos: string[] | null;
}

export const NO_CHOICE: ExportChoice = {
  parts: [],
  without: [],
  roots: [],
  worktrees: true,
  skipNoRemote: false,
  libraryZip: false,
  sessionRepos: null,
};

/** `passphrase` says the passphrase goes on stdin; the caller sends it only when a sealed part is ticked. */
export function exportArgv(
  choice: ExportChoice,
  out: string,
  options: { passphrase: boolean; dryRun: boolean; peek?: string },
): string[] {
  const { parts } = choice;
  const argv = ["setup", "export", "--parts", parts.join(","), "--out", out];
  if (parts.includes(SECRETS_PART)) argv.push("--secrets");
  if (options.passphrase) argv.push("--passphrase-stdin");
  for (const root of choice.roots) argv.push("--roots", root);
  if (parts.includes("repos")) {
    if (!choice.worktrees) argv.push("--no-worktrees");
    if (choice.skipNoRemote) argv.push("--skip-no-remote");
  }
  if (parts.includes("library") && choice.libraryZip) argv.push("--library-zip");
  const off = choice.without.filter((id) =>
    Object.entries(SUB_CHOICES).some(
      ([part, subs]) => parts.includes(part) && subs.some((s) => s.id === id),
    ),
  );
  if (off.length > 0) argv.push("--without", off.join(","));
  if (parts.includes(SESSIONS_PART) && choice.sessionRepos) {
    for (const name of choice.sessionRepos) argv.push("--session-repo", name);
  }
  if (options.peek) argv.push("--peek", options.peek);
  if (options.dryRun || options.peek) argv.push("--dry-run");
  return argv;
}

export function partsArgv(roots: string[]): string[] {
  const argv = ["setup", "parts"];
  for (const root of roots) argv.push("--roots", root);
  return argv;
}

/** Whether a part is sealed with the passphrase, so the passphrase is needed. */
export function needsPassphrase(parts: string[], all: SetupPart[]): boolean {
  return all.some((p) => p.sealed && parts.includes(p.id));
}

export function restoreArgv(
  file: string,
  options: {
    sealed: boolean;
    plan?: string;
    take?: string[];
    parts?: string[];
  },
): string[] {
  const argv = ["setup", "restore", file];
  if (options.sealed) argv.push("--passphrase-stdin");
  if (options.parts && options.parts.length > 0)
    argv.push("--parts", options.parts.join(","));
  for (const row of options.take ?? []) argv.push("--take", row);
  if (options.plan) argv.push("--apply", "--plan", options.plan);
  return argv;
}

export function undoArgv(run: string | null, dryRun: boolean): string[] {
  const argv = ["setup", "undo"];
  if (run) argv.push(run);
  if (dryRun) argv.push("--dry-run");
  return argv;
}

/** The part labels the Covered list and the plan groups use. */
export const PART_LABEL: Record<string, string> = {
  repos: "Repositories",
  servers: "Servers",
  access: "Clients and access sets",
  library: "Skills library",
  tasks: "Tasks",
  settings: "Toolport settings",
  claudeUser: "Claude Code user files",
  memory: "Memory",
  history: "Sessions",
  secrets: "Secrets",
};

export const KB = 1024;

export function size(bytes: number): string {
  if (bytes >= KB * KB) return `${(bytes / (KB * KB)).toFixed(1)} MB`;
  if (bytes >= KB) return `${Math.ceil(bytes / KB)} KB`;
  return `${bytes} B`;
}

const DAY_MS = 86_400_000;

/** Whole days between an RFC 3339 time and now, or null when the time does not parse. */
export function ageInDays(madeAt: string, now: number): number | null {
  const then = Date.parse(madeAt);
  return Number.isNaN(then) ? null : Math.max(0, Math.floor((now - then) / DAY_MS));
}

export function ageText(days: number): string {
  if (days === 0) return "today";
  return days === 1 ? "1 day old" : `${days} days old`;
}

/** Plural: `plural(3, "repository", "repositories")`. */
export function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

export function sinceLine(since: SinceRow[]): string {
  if (since.length === 0) return "Nothing changed since.";
  const bits = since.map((row) => {
    if (row.id === "repos" && row.branchChanges) {
      return plural(
        row.branchChanges,
        "repository changed branch",
        "repositories changed branch",
      );
    }
    const n = row.added + row.removed + row.changed;
    return `${row.label}: ${plural(n, "file", "files")} changed`;
  });
  return `Since then: ${bits.join(", ")}.`;
}
