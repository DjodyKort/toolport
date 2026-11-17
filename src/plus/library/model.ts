import type { SkillsInventory, SkillLoadState } from "@/lib/types";
import { plural } from "../settings/model";
import type {
  AutoStatus,
  LibrarySkill,
  LintData,
  LintRow,
  PlanChange,
  RemoteStatus,
  SkillFile,
  StatusData,
  SyncData,
  VerifyData,
} from "./data";

export { plural };

/** The two targets the sync still writes to (D-143). */
export const TARGETS = [
  {
    id: "claude-code",
    label: "Claude Code",
    where: "~/.claude/skills · rules · agents · hooks in settings.json",
  },
  { id: "agents-md", label: "AGENTS.md", where: "~/.agents · rules as one file" },
] as const;

export function bytes(size: number): string {
  if (size < 1024) return `${size} B`;
  const kb = size / 1024;
  if (kb < 1024) return `${kb.toFixed(1)} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

/** `/Users/me/work/lib` shown as `~/work/lib` under the home folder. */
export function tilde(path: string, home: string | null): string {
  return home && (path === home || path.startsWith(`${home}/`))
    ? `~${path.slice(home.length)}`
    : path;
}

export function count(value: number): string {
  return value.toLocaleString("en-US");
}

export function when(iso: string | null | undefined): string {
  if (!iso) return "never";
  const date = new Date(iso);
  return Number.isNaN(date.getTime())
    ? iso
    : date.toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
}

export const GLYPH: Record<string, string> = {
  add: "+",
  update: "~",
  adopt: "=",
  differs: "!",
  edited: "!",
  remove: "−",
  hook: "+",
  error: "×",
};

export const OP_BADGE: Record<string, string> = {
  add: "New",
  update: "Changed",
  adopt: "Adopted",
  differs: "Differs",
  edited: "Edited outside",
  remove: "Removed",
  hook: "Hook",
};

export interface PlanRow {
  skill: string;
  kind: "skill" | "rule";
  /** Strongest op of the row: remove and decisions outrank add and update. */
  op: string;
  changes: PlanChange[];
  held: boolean;
  /** One line that says what happens, in the words of the plan marker. */
  summary: string;
  files: number;
}

export interface PlanGroups {
  skills: PlanRow[];
  rules: PlanRow[];
  hooks: PlanRow[];
  total: number;
  /** Writes of the plan: files written, files removed, hook entries added. */
  written: number;
  removed: number;
  hooksAdded: number;
  held: number;
}

const RANK = ["remove", "differs", "edited", "hook", "add", "update", "adopt"];
const ORDER = ["add", "update", "adopt", "hook", "edited", "differs", "remove"];

function strongest(ops: string[]): string {
  return [...ops].sort((a, b) => RANK.indexOf(a) - RANK.indexOf(b))[0] ?? "update";
}

function summarise(skill: string, changes: PlanChange[]): string {
  const of = (op: string) => changes.filter((c) => c.op === op);
  const parts: string[] = [];
  const add = of("add");
  const update = of("update");
  if (add.length && !update.length && add.length === changes.length) {
    parts.push(`New. ${plural(add.length, "file")}.`);
  } else {
    if (update.length) parts.push(`${plural(update.length, "file")} changed.`);
    if (add.length) parts.push(`${add.length} added.`);
  }
  if (of("remove").length)
    parts.push(
      `Removed from your library. Deletes the ${plural(of("remove").length, "file")} sync installed earlier. Files you added to that folder stay.`,
    );
  if (of("edited").length)
    parts.push(
      `${plural(of("edited").length, "file")} edited in ~/.claude/skills after the last sync; the library copy replaces it and your edit is kept as a backup.`,
    );
  if (of("differs").length)
    parts.push(
      `${plural(of("differs").length, "file")} already exist with different content and wait for your choice.`,
    );
  if (of("adopt").length)
    parts.push(`${of("adopt").length} identical, adopted without a write.`);
  return parts.join(" ") || `${skill}: nothing to write.`;
}

/** The plan as the page draws it: skills, rules and hooks apart, one row per skill. */
export function groupPlan(sync: SyncData): PlanGroups {
  const type = new Map(sync.entries.map((e) => [e.name, e.type]));
  const bySkill = new Map<string, PlanChange[]>();
  for (const change of sync.plan.changes) {
    bySkill.set(change.skill, [...(bySkill.get(change.skill) ?? []), change]);
  }
  const groups: PlanGroups = {
    skills: [],
    rules: [],
    hooks: [],
    total: 0,
    written: 0,
    removed: 0,
    hooksAdded: 0,
    held: 0,
  };
  for (const [skill, changes] of bySkill) {
    const files = changes.filter((c) => c.op !== "hook");
    const hooks = changes.filter((c) => c.op === "hook");
    const kind = type.get(skill) === "rule" ? "rule" : "skill";
    if (files.length) {
      groups[kind === "rule" ? "rules" : "skills"].push({
        skill,
        kind,
        op: strongest(files.map((c) => c.op)),
        changes: files,
        held: files.some((c) => c.held),
        summary: summarise(skill, files),
        files: files.length,
      });
    }
    if (hooks.length) {
      groups.hooks.push({
        skill,
        kind,
        op: "hook",
        changes: hooks,
        held: hooks.some((c) => c.held),
        summary: `New hook entry in ~/.claude/settings.json. It runs a command on its own, with your user rights.`,
        files: 0,
      });
    }
  }
  const sort = (rows: PlanRow[]) =>
    rows.sort(
      (a, b) =>
        ORDER.indexOf(a.op) - ORDER.indexOf(b.op) || a.skill.localeCompare(b.skill),
    );
  sort(groups.skills);
  sort(groups.rules);
  sort(groups.hooks);
  for (const change of sync.plan.changes) {
    groups.total += 1;
    if (change.held) groups.held += 1;
    if (change.op === "add" || change.op === "update") groups.written += 1;
    if (change.op === "remove") groups.removed += 1;
    if (change.op === "hook") groups.hooksAdded += 1;
  }
  return groups;
}

/** The plan counts by skill: one row of the page is one change. */
export const rowCount = (changes: PlanChange[]) =>
  new Set(changes.map((c) => c.skill)).size;

export interface TickNeeds {
  differs: number;
  tapHooks: number;
}

/** What the explicit tick covers: files that exist with other content and are not tracked, and
 * hooks from a tap. Removals, hand edits (the snapshot restores them) and hooks of the user's own
 * library are in the plan on screen and need no tick. */
export function tickNeeds(sync: SyncData, tapSkills: ReadonlySet<string>): TickNeeds {
  const held = sync.plan.changes.filter((c) => c.held);
  return {
    differs: held.filter((c) => c.op === "differs").length,
    tapHooks: held.filter((c) => c.op === "hook" && tapSkills.has(c.skill)).length,
  };
}

export const needsTick = (needs: TickNeeds) => needs.differs + needs.tapHooks > 0;

export function tickLabel(needs: TickNeeds): string {
  const parts: string[] = [];
  if (needs.differs)
    parts.push(
      `replace ${plural(needs.differs, "file")} that already exist with other content (a backup is kept)`,
    );
  if (needs.tapHooks)
    parts.push(`install ${plural(needs.tapHooks, "hook")} that come from a tap`);
  const text = parts.join(" and ");
  return text ? text[0].toUpperCase() + text.slice(1) : "";
}

export interface ApplyChoice {
  argv: string[];
  /** Changes this Apply writes. */
  count: number;
}

/** The sync argv for the plan on screen. Without the tick the changes that wait for a decision
 * stay out; with it they are applied, and tap hooks only when they were ticked for. */
export function applyChoice(
  sync: SyncData,
  needs: TickNeeds,
  ticked: boolean,
  clients: readonly string[] | null,
): ApplyChoice {
  const holdsBack = needsTick(needs);
  const withHeld = !holdsBack || ticked;
  const held = sync.plan.changes.filter((c) => c.held).length;
  const argv = [
    "skills",
    "sync",
    ...(withHeld && held > 0 ? ["--apply-held"] : []),
    ...(withHeld && needs.tapHooks > 0 ? ["--allow-tap-hooks"] : []),
    ...(clients ?? []).flatMap((client) => ["--client", client]),
  ];
  const applied = sync.plan.changes.filter((c) => withHeld || !c.held);
  return { argv, count: rowCount(applied) };
}

export function previewArgv(clients: readonly string[] | null): string[] {
  return [
    "skills",
    "sync",
    "--dry-run",
    ...(clients ?? []).flatMap((client) => ["--client", client]),
  ];
}

export interface HeldBack {
  skill: string;
  rows: LintRow[];
}

/** A lint error holds back only the skill it is about; the rest of the plan still syncs. */
export function heldBack(lint: LintData | null): HeldBack[] {
  const by = new Map<string, LintRow[]>();
  for (const row of lint?.messages ?? []) {
    if (row.level !== "error") continue;
    by.set(row.name, [...(by.get(row.name) ?? []), row]);
  }
  return [...by]
    .map(([skill, rows]) => ({ skill, rows }))
    .sort((a, b) => a.skill.localeCompare(b.skill));
}

export interface LintGroup {
  id: "hides" | "catch" | "advice";
  title: string;
  rows: LintRow[];
  /** Files left out by the deny-list, one line for all skills. */
  notCopied: { files: number; skills: number } | null;
}

const NOT_COPIED = "not-copied";

/** Rows by what Claude does: errors hide the skill, warnings are read with a catch, the rest is
 * advice. The deny-list rows are counted, not listed. */
export function lintGroups(lint: LintData | null): LintGroup[] {
  const rows = lint?.messages ?? [];
  const skipped = rows.filter((r) => r.consequence === NOT_COPIED);
  const rest = rows.filter((r) => r.consequence !== NOT_COPIED);
  const groups: LintGroup[] = [
    {
      id: "hides",
      title: "Claude Code hides these",
      rows: rest.filter((r) => r.level === "error"),
      notCopied: null,
    },
    {
      id: "catch",
      title: "Claude Code reads these, with a catch",
      rows: rest.filter((r) => r.level === "warning"),
      notCopied: null,
    },
    {
      id: "advice",
      title: "Advice and information",
      rows: rest.filter((r) => r.level === "info"),
      notCopied: skipped.length
        ? { files: skipped.length, skills: new Set(skipped.map((r) => r.name)).size }
        : null,
    },
  ];
  return groups.filter((g) => g.rows.length || g.notCopied);
}

export function lintTone(lint: LintData | null): "none" | "warning" | "error" {
  if (!lint) return "none";
  return lint.errors > 0 ? "error" : lint.warnings > 0 ? "warning" : "none";
}

export interface Meter {
  fill: number;
  specTick: number;
  limitTick: number;
  over: boolean;
}

/** Percentages on a scale that always shows both limits and the length, even when it is above. */
export function meter(length: number, limit: number, specLimit: number): Meter {
  const scale = Math.max(limit * 1.2, length * 1.05);
  const pct = (value: number) => Math.min(100, (value / scale) * 100);
  return {
    fill: pct(length),
    specTick: pct(specLimit),
    limitTick: pct(limit),
    over: length > limit,
  };
}

export function claudeState(
  inventory: SkillsInventory | null,
  name: string,
): SkillLoadState | null {
  const entry = inventory?.skills.find((s) => s.name === name);
  return entry?.states["claude-code"]?.state ?? null;
}

export const CLAUDE_LABEL: Record<SkillLoadState, string> = {
  on: "Claude: on",
  "name-only": "Claude: name only",
  off: "Claude: off",
  shadowed: "Claude: shadowed",
  broken: "Claude: broken",
};

export type ItemState =
  "new" | "changed" | "edited" | "differs" | "removed" | "held" | "synced" | "unsynced";

export const ITEM_STATE_LABEL: Record<ItemState, string> = {
  new: "New",
  changed: "Changed",
  edited: "Edited outside",
  differs: "Differs",
  removed: "Removed",
  held: "Held back",
  synced: "In sync",
  unsynced: "Not delivered",
};

export interface LibraryItem {
  name: string;
  kind: "skill" | "rule";
  description: string;
  state: ItemState;
  files: number | null;
  claude: SkillLoadState | null;
  visible: boolean;
}

/** One row per library item, with the state the plan and the lock give it. */
export function libraryItems(
  skills: LibrarySkill[],
  status: StatusData | null,
  sync: SyncData | null,
  held: HeldBack[],
  inventory: SkillsInventory | null,
): LibraryItem[] {
  const locked = new Map((status?.entries ?? []).map((e) => [e.name, e]));
  const changes = new Map<string, PlanChange[]>();
  for (const change of sync?.plan.changes ?? []) {
    changes.set(change.skill, [...(changes.get(change.skill) ?? []), change]);
  }
  const heldNames = new Set(held.map((h) => h.skill));
  return skills
    .map((skill): LibraryItem => {
      const own = changes.get(skill.name) ?? [];
      const ops = own.map((c) => c.op);
      const entry = locked.get(skill.name);
      const state: ItemState = heldNames.has(skill.name)
        ? "held"
        : ops.includes("remove")
          ? "removed"
          : ops.includes("edited")
            ? "edited"
            : ops.includes("differs")
              ? "differs"
              : ops.includes("update")
                ? "changed"
                : ops.includes("add")
                  ? entry?.clientsSynced.length
                    ? "changed"
                    : "new"
                  : entry?.clientsSynced.length || entry?.knownToLockfile
                    ? "synced"
                    : "unsynced";
      return {
        name: skill.name,
        kind: skill.type === "rule" ? "rule" : "skill",
        description: skill.description,
        state,
        files: own.length || null,
        claude: claudeState(inventory, skill.name),
        visible: skill.visible,
      };
    })
    .sort((a, b) => a.name.localeCompare(b.name));
}

export interface ItemFilter {
  query: string;
  kind: "all" | "skill" | "rule";
  state: "any" | "waiting" | ItemState;
}

export function filterItems(items: LibraryItem[], filter: ItemFilter): LibraryItem[] {
  const query = filter.query.trim().toLowerCase();
  return items.filter((item) => {
    if (filter.kind !== "all" && item.kind !== filter.kind) return false;
    if (filter.state === "waiting" && ["synced", "unsynced"].includes(item.state))
      return false;
    if (
      filter.state !== "any" &&
      filter.state !== "waiting" &&
      item.state !== filter.state
    )
      return false;
    return !query || `${item.name} ${item.description}`.toLowerCase().includes(query);
  });
}

export function verifyLine(verify: VerifyData): string {
  return `${count(verify.matched)} of ${count(verify.total)} library files match`;
}

export interface FileBadge {
  label: string;
  variant: "info" | "success" | "warning" | "off" | "destructive" | "outline";
}

/** The status word of a file in a skill's Files list: New, Changed, Same, Not copied. */
export function fileBadge(file: SkillFile): FileBadge {
  switch (file.status) {
    case "would-copy":
      return file.op === "update"
        ? { label: "Changed", variant: "warning" }
        : { label: "New", variant: "info" };
    case "copied":
      return { label: "Same", variant: "off" };
    case "differs":
      return { label: "Differs", variant: "warning" };
    case "hand-edited":
      return { label: "Edited outside", variant: "warning" };
    case "not-copied":
      return { label: "Not copied", variant: "outline" };
    case "not-yours":
      return { label: "Not yours", variant: "outline" };
    case "removed-on-next-sync":
      return { label: "Removed", variant: "destructive" };
    default:
      return { label: file.status, variant: "off" };
  }
}

export const FILE_ORDER = [
  "would-copy",
  "differs",
  "hand-edited",
  "removed-on-next-sync",
  "copied",
  "not-yours",
  "not-copied",
];

export const FILE_COUNT_LABEL: Record<string, string> = {
  "would-copy": "new or changed",
  copied: "unchanged",
  differs: "differ",
  "hand-edited": "edited outside",
  "removed-on-next-sync": "removed on next sync",
  "not-yours": "not yours",
  "not-copied": "not copied",
};

export type RemoteState =
  | { kind: "none" }
  | { kind: "no-remote" }
  | { kind: "dirty"; uncommitted: number; behind: number }
  | { kind: "behind"; behind: number }
  | { kind: "diverged"; ahead: number; behind: number }
  | { kind: "ahead"; ahead: number }
  | { kind: "offline"; error: string }
  | { kind: "current" };

export function remoteState(status: RemoteStatus | null): RemoteState {
  if (!status) return { kind: "none" };
  if (!status.remote) return { kind: "no-remote" };
  if (status.fetch.requested && status.fetch.ok === false)
    return { kind: "offline", error: status.fetch.error ?? "the remote did not answer" };
  if (status.ahead > 0 && status.behind > 0)
    return { kind: "diverged", ahead: status.ahead, behind: status.behind };
  if (status.behind > 0 && status.uncommitted > 0)
    return { kind: "dirty", uncommitted: status.uncommitted, behind: status.behind };
  if (status.behind > 0) return { kind: "behind", behind: status.behind };
  if (status.ahead > 0) return { kind: "ahead", ahead: status.ahead };
  return { kind: "current" };
}

/** Pull and review plan is one step only when the library is behind and clean. */
export const canPull = (state: RemoteState) => state.kind === "behind";

export function pullBlockedReason(state: RemoteState): string | null {
  switch (state.kind) {
    case "dirty":
      return `Pull is off: commit or stash the ${plural(state.uncommitted, "change")} first, or use Push, which commits them for you. Pull never merges.`;
    case "diverged":
      return "Pull is off: the library and its remote both have commits. A fast-forward is not possible; resolve it in git, then fetch.";
    case "no-remote":
      return "Pull and Push are off: this library has no remote.";
    case "offline":
      return `Pull is off: the remote could not be reached (${state.error}).`;
    case "current":
    case "ahead":
      return "Pull is off: nothing to pull.";
    default:
      return null;
  }
}

export function autoLine(auto: AutoStatus | null): { on: boolean; text: string } {
  if (!auto) return { on: false, text: "Auto-sync: unknown" };
  if (auto.paused)
    return {
      on: false,
      text:
        auto.reason === "undo"
          ? "Auto-sync is paused because you undid a sync, so it does not redo it."
          : "Auto-sync is paused.",
    };
  return { on: true, text: "Auto-sync on" };
}

/** Skills that came from a tap: their hooks need an extra tick. */
export function tapSkillNames(skills: LibrarySkill[]): Set<string> {
  return new Set(skills.filter((s) => s.origin.kind === "tap").map((s) => s.name));
}

export interface Headline {
  kind:
    | "in-sync"
    | "changes"
    | "first-sync"
    | "behind"
    | "held-back"
    | "auto-held"
    | "undone";
  tone: "success" | "warning" | "info";
  title: string;
  detail: string;
}

/** The status card of a Sync tab that has a plan, in the words of the state it is in. */
export function headline(
  sync: SyncData,
  held: HeldBack[],
  remote: RemoteState,
  auto: AutoStatus | null,
  lockfilePresent: boolean,
): Headline {
  const total = rowCount(sync.plan.changes);
  if (total === 0 && held.length === 0) {
    return {
      kind: "in-sync",
      tone: "success",
      title: "Claude Code has everything in your library",
      detail: `${plural(sync.skillCount + sync.ruleCount, "item")} in ${sync.repo}. Nothing is waiting to be written.`,
    };
  }
  if (!lockfilePresent && total > 0) {
    return {
      kind: "first-sync",
      tone: "info",
      title: `Your library has ${plural(sync.skillCount + sync.ruleCount, "item")}; none are in Claude Code yet`,
      detail:
        "The plan lists every file first. Files already in ~/.claude/skills with identical content are adopted without a write; different ones wait for your choice.",
    };
  }
  if (auto?.paused && auto.reason === "undo") {
    return {
      kind: "undone",
      tone: "info",
      title: "Undone. Claude Code is back to before the last sync",
      detail:
        "Your library did not change, so those changes are waiting again. Auto-sync is paused so it does not redo what you just undid.",
    };
  }
  if (held.length > 0) {
    return {
      kind: "held-back",
      tone: "warning",
      title: `${plural(total, "change")} can sync; ${plural(held.length, "skill")} ${held.length === 1 ? "is" : "are"} held back`,
      detail: `Claude Code would hide or cut ${held.map((h) => h.skill).join(", ")}, so they are left out and keep their last good copy. Fix them, or apply the rest now.`,
    };
  }
  if (remote.kind === "behind") {
    return {
      kind: "behind",
      tone: "warning",
      title: `Your library is ${plural(remote.behind, "commit")} behind its remote`,
      detail:
        "Sync uses the files in the library as they are now, so those commits are not in Claude Code yet. Pull is fast-forward only; it never merges and never runs by itself.",
    };
  }
  return {
    kind: "changes",
    tone: "warning",
    title: `${plural(total, "change")} ${total === 1 ? "is" : "are"} waiting for Claude Code`,
    detail:
      "Applying takes a snapshot first, so Undo can restore everything it changes. The plan below is what happens to each file.",
  };
}

export function syncedLine(sync: SyncData): string {
  const written = sync.plan.changes.filter((c) => !c.held && c.op !== "hook").length;
  const hooks = sync.plan.counts.hook;
  return `${plural(written, "file")} written, ${sync.plan.counts.remove} removed, ${hooks} ${hooks === 1 ? "hook entry" : "hook entries"} added. Only files whose content differs are written.`;
}

export interface Doing {
  verified: boolean;
  line: string;
  mismatched: string[];
}

export function doneSummary(sync: SyncData): Doing {
  const verify = sync.verify;
  return {
    verified: verify?.clean === true,
    line: verify ? verifyLine(verify) : "Not checked",
    mismatched: (verify?.mismatched ?? []).map((m) => `${m.skill}/${m.path}`),
  };
}
