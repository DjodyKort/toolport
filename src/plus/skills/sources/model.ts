import type { SourceItem, SourceRow, SourcesRootLsData } from "../../bridge/data";

export type Detector = SourceRow["detector"];
export type State = SourceRow["status"]["state"];
export type Basis = SourceRow["tokens"]["basis"];
export type Root = SourcesRootLsData["roots"][number];

/** How each detector finds its source, in the words of the contract's detector table. */
export const FOUND_BY: Record<Detector, string> = {
  repo: "The git tree of HEAD and of the remote default branch, not the working folder. Worktrees are skipped.",
  client:
    "The git tree of each client repository under the clients folder, one source per repository.",
  vendored: "SKILL.md in submodule or vendored folders, found by a depth-limited scan.",
  plugin:
    "installed_plugins.json, then each plugin's folder. Enabled state comes from your settings.",
  org: "File hashes against the organisation's tool clone and its sync stamp.",
  account: "The manifest.json in each folder Claude synced from your account.",
  library: "A path scan of your skills repository.",
  tap: "taps.json and the clone of each tap.",
  loose:
    "Files in ~/.claude that no other source owns, told apart by hash and the Managed by toolportctl header.",
  inert:
    "CLAUDE.md or SKILL.md files that no load rule reaches (a _claude folder or a configured pattern).",
  "remote-library":
    "The remote status of the library clone and any duplicate clones of it.",
};

const OWNERS: Record<SourceRow["owner"], string> = {
  me: "You",
  org: "Your organisation",
  "third-party": "A third party",
  anthropic: "Anthropic",
  project: "The repository",
};

export const ownerLabel = (owner: SourceRow["owner"]): string => OWNERS[owner];

export const STATE_LABEL: Record<State, string> = {
  ok: "ok",
  stale: "out of date",
  behind: "behind its remote",
  unreachable: "remote unreachable",
  duplicate: "duplicate clone",
  partial: "partial scan",
};

export const STATE_TONE: Record<State, "success" | "warning" | "destructive"> = {
  ok: "success",
  stale: "warning",
  behind: "warning",
  unreachable: "destructive",
  duplicate: "warning",
  partial: "warning",
};

const number = (n: number) => n.toLocaleString("en");

export function basisText(basis: Basis): string {
  if (basis === "measured") return "measured";
  if (basis === "projected") return "projected by Claude Code";
  return "estimate";
}

/** A token number always says what it is (D-065): an estimate (bytes / 4, for ordering only),
 * what Claude Code measured, or the figure its own plugin details report. */
export function tokenText(tokens: { value: number; basis: Basis }): string {
  const value = number(tokens.value);
  if (tokens.basis === "measured") return `${value} tokens (measured)`;
  if (tokens.basis === "projected") return `${value} tokens (projected by Claude Code)`;
  return `about ${value} tokens (estimate)`;
}

/** The sum of a column of sources is measured only when every part is. */
export function totalTokens(rows: SourceRow[]): { value: number; basis: Basis } {
  const value = rows.reduce((sum, row) => sum + row.tokens.value, 0);
  const bases = new Set(rows.map((row) => row.tokens.basis));
  const basis: Basis = bases.size === 1 && rows.length > 0 ? [...bases][0] : "estimate";
  return { value, basis };
}

const NOUNS: Array<[keyof SourceRow["counts"], string, string]> = [
  ["skill", "skill", "skills"],
  ["command", "command", "commands"],
  ["agent", "agent", "agents"],
  ["rule", "rule", "rules"],
  ["memory", "CLAUDE.md file", "CLAUDE.md files"],
];

export function countsText(counts: SourceRow["counts"]): string {
  const parts = NOUNS.filter(([key]) => counts[key] > 0).map(
    ([key, one, many]) => `${counts[key]} ${counts[key] === 1 ? one : many}`,
  );
  return parts.length > 0 ? parts.join(" · ") : "nothing found";
}

export function itemTotal(counts: SourceRow["counts"]): number {
  return counts.skill + counts.command + counts.agent + counts.rule + counts.memory;
}

export function visibleTotals(rows: SourceRow[]): { seen: number; total: number } {
  return rows.reduce(
    (sum, row) => ({
      seen: sum.seen + row.visible.skill,
      total: sum.total + row.visible.skillTotal,
    }),
    { seen: 0, total: 0 },
  );
}

/** What wants a look: every source that is not plainly ok, and the skills Claude Code cannot see. */
export function needsLook(rows: SourceRow[]): string[] {
  const count = (state: State) => rows.filter((row) => row.status.state === state).length;
  const hidden = visibleTotals(rows);
  const parts: Array<[number, string]> = [
    [count("behind"), "behind its remote"],
    [count("stale"), "out of date"],
    [count("duplicate"), "with a duplicate clone"],
    [count("unreachable"), "remote unreachable"],
    [count("partial"), "partly scanned"],
    [hidden.total - hidden.seen, "skills hidden from Claude"],
  ];
  return parts.filter(([n]) => n > 0).map(([n, text]) => `${n} ${text}`);
}

/** A URL in a message loses any user:password part before it is shown, so a credential that
 * slipped into a remote never reaches the screen. */
export function plain(text: string): string {
  return text.replace(/\b([a-z][a-z0-9+.-]*:\/\/)[^\s/@]+@/gi, "$1");
}

export const isGitTree = (row: SourceRow): boolean =>
  row.root !== null && !row.root.startsWith("/") && /^[\w.-]+\/[\w./-]+$/.test(row.root);

export const baseName = (path: string): string =>
  path.split("/").filter(Boolean).at(-1) ?? path;

export function timeText(iso: string | null): string {
  if (!iso) return "never";
  const at = new Date(iso);
  return Number.isNaN(at.getTime())
    ? iso
    : at.toLocaleString("en", { dateStyle: "medium", timeStyle: "short" });
}

export const ITEM_NOUN: Record<SourceItem["kind"], string> = {
  skill: "skill",
  command: "command",
  agent: "agent",
  rule: "rule",
  memory: "CLAUDE.md",
};

/** The actions the mockup draws on a source that no command backs yet. Each is shown disabled
 * with its reason, never as a button that does nothing. */
export const MISSING_ACTIONS: Partial<
  Record<Detector, Array<{ label: string; reason: string }>>
> = {
  org: [
    {
      label: "Show what it wrote",
      reason:
        "No command lists the files the organisation sync wrote. The items list below shows what it provides.",
    },
  ],
  plugin: [
    {
      label: "Choose per folder…",
      reason:
        "Switching a plugin off per folder belongs to Context > Profiles and Library > Plugins (MIG-GUI-12).",
    },
  ],
  repo: [
    {
      label: "Audit",
      reason:
        "`skills audit` checks your library only. Repository skills stay untrusted until a command audits them.",
    },
  ],
  client: [
    {
      label: "Audit",
      reason:
        "`skills audit` checks your library only. Client repository skills stay untrusted.",
    },
  ],
  loose: [
    {
      label: "Adopt into library…",
      reason:
        "No command copies a loose file into the library yet. Move it into the skills repository by hand.",
    },
  ],
  tap: [
    {
      label: "Add tap…",
      reason: "Taps are managed on the Skills tab, under Taps.",
    },
  ],
};
