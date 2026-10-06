import type { CommandsData, ProfileLsData } from "../bridge/data";
import { toolCallArgv } from "../allcommands/model";
import {
  kindLabel,
  plural,
  postUpdateOf,
  type UpdateServer,
  updateReport,
} from "../system/model";
import type { PlanStep, PlanV1 } from "../ui";

/** What the Servers screen needs of the self-management tools that no command covers
 * (contract section 15): the policy row, the call, and the plans in plain words. */

export type ToolTier = "read" | "write" | "destructive";

export interface ToolPolicy {
  name: string;
  tier: ToolTier;
  /** Tier 3 and up refuse to run without `confirm: true`. */
  needsConfirm: boolean;
}

export const NO_MCP_CALL =
  "This build of toolportctl has no `mcp call` command yet, so this cannot run from here.";

export function hasMcpCall(registry: CommandsData | null): boolean {
  return !!registry?.commands.some((row) => row.id === "mcp call");
}

/** The registry's word on one tool; a tool it does not list is not run from here. */
export function toolPolicyOf(
  registry: CommandsData | null,
  name: string,
): ToolPolicy | null {
  if (!hasMcpCall(registry)) return null;
  const row = registry?.tools.find((tool) => tool.name === name);
  if (!row?.tier) return null;
  return { name, tier: row.tier, needsConfirm: row.toolTier >= 3 };
}

export const toolArgv = toolCallArgv;

/** The arguments of a call: they go through stdin, never argv. */
export const toolArgs = (value: Record<string, unknown>): string => JSON.stringify(value);

type Data = Record<string, unknown>;

const record = (value: unknown): Data | null =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Data)
    : null;
const text = (value: unknown): string | undefined =>
  typeof value === "string" && value !== "" ? value : undefined;
const count = (value: unknown): number | undefined =>
  typeof value === "number" ? value : undefined;
const strings = (value: unknown): string[] =>
  Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];

/** `data` of `mcp call`: `{ isError, result, tier, tool }`. A tool that failed answers with an
 * error envelope, so `isError` is only a second line of defence here. */
export function callResult(
  data: unknown,
): { ok: true; result: Data } | { ok: false; message: string } {
  const wrapped = record(data);
  const result = record(wrapped?.result);
  if (!wrapped || wrapped.isError === true || !result) {
    const message = text(record(result?.error)?.message) ?? text(result?.message);
    return { ok: false, message: message ?? "The tool did not return a result." };
  }
  return { ok: true, result };
}

// ---- source and git -------------------------------------------------------------------

export interface SourceInfo {
  kind: string;
  /** True when the source is stored in the registry; false when it was only detected. */
  stored: boolean;
  path?: string;
  branch?: string;
  remote?: string;
  reason?: string;
  drift?: boolean;
  upstream?: { remote: string; branch: string };
  /** Every remote the checkout already knows, in the order the registry reports them. */
  remotes: string[];
  /** The branches known locally for each remote (`gitops::remote_branches`); a remote that
   * was never fetched reports an empty list here, not an error. */
  branches: Record<string, string[]>;
}

const branchesOf = (value: unknown): Record<string, string[]> => {
  const rec = record(value);
  if (!rec) return {};
  const out: Record<string, string[]> = {};
  for (const [remote, names] of Object.entries(rec)) out[remote] = strings(names);
  return out;
};

export function sourceOf(result: Data): SourceInfo {
  const detected = record(result.detected);
  const meta = record(detected?.meta);
  const upstream = record(meta?.upstream);
  return {
    kind: text(detected?.kind) ?? "unknown",
    stored: result.stored === true,
    path: text(meta?.path),
    branch: text(meta?.branch),
    remote: text(meta?.remote),
    reason: text(meta?.reason),
    drift: typeof meta?.drift === "boolean" ? meta.drift : undefined,
    upstream:
      upstream && text(upstream.remote) && text(upstream.branch)
        ? { remote: text(upstream.remote)!, branch: text(upstream.branch)! }
        : undefined,
    remotes: strings(result.remotes),
    branches: branchesOf(result.branches),
  };
}

export interface GitState {
  isGit: boolean;
  message?: string;
  pathExists: boolean;
  path?: string;
  branch?: string;
  remoteRef?: string;
  ahead?: number;
  behind?: number;
  dirty: boolean;
  summaries: string[];
  error?: string;
}

export function gitStateOf(result: Data): GitState {
  return {
    isGit: result.isGit === true,
    message: text(result.message),
    pathExists: result.pathExists !== false,
    path: text(result.path),
    branch: text(result.branch),
    remoteRef: text(result.remoteRef),
    ahead: count(result.ahead),
    behind: count(result.behind),
    dirty: result.dirty === true,
    summaries: strings(result.summaries),
    error: text(result.error),
  };
}

/** Why a git state cannot be synced, or null when it can. */
export function syncBlocker(git: GitState): string | null {
  if (!git.isGit) return git.message ?? "This server is not tracked as a git checkout.";
  if (!git.pathExists) return "The checkout folder is gone.";
  if (git.error) return git.error;
  if (git.dirty)
    return "The checkout has uncommitted changes. Commit or stash them first.";
  return null;
}

// ---- updates ----------------------------------------------------------------------------

export function updateOf(result: Data, id?: string): UpdateServer | null {
  const servers = updateReport(result).servers;
  return (
    (id ? servers.find((server) => server.id === id) : undefined) ?? servers[0] ?? null
  );
}

const HELD = /\s*\(not run without --allow-commands\)$/;

/** The plan to confirm before an update: what the check found, and the stored update command,
 * which runs as part of the update (D-079). */
export function applyUpdatePlan(name: string, server: UpdateServer | null): PlanV1 {
  const steps: PlanStep[] = [];
  const warnings: string[] = [];
  const hook = server ? postUpdateOf(server) : null;
  if (server) {
    steps.push({
      op: "update",
      path: name,
      detail: `${kindLabel(server.kind)}: ${server.message}`,
    });
    for (const line of server.plan) {
      if (line.startsWith("post_update:")) continue;
      steps.push({ op: "note", path: name, detail: line });
    }
  } else {
    steps.push({ op: "update", path: name, detail: "Update to the latest version" });
  }
  steps.push({
    op: "exec",
    path: name,
    detail: hook
      ? `Runs the stored update command (post_update): ${hook.command.replace(HELD, "")}`
      : "Runs the stored update command (post_update) of this server, if it has one",
  });
  warnings.push(
    "Confirming also runs the server's stored update command, on this computer, with your rights.",
  );
  return {
    summary: `Update ${name}`,
    steps,
    effects: {},
    warnings,
    undo: "",
  };
}

// ---- profile tags ----------------------------------------------------------------------

/** The profile a typed name refers to, the way the tool reads it: an exact id, or a name that
 * differs only in case. */
export function profileMatch(profiles: ProfileLsData, tag: string) {
  const key = tag.trim();
  return (
    profiles.profiles.find((profile) => profile.id === key) ??
    profiles.profiles.find((profile) => profile.name.toLowerCase() === key.toLowerCase())
  );
}

export function addTagPlan(server: string, tag: string, exists: boolean): PlanV1 {
  const steps: PlanStep[] = [];
  if (!exists) {
    steps.push({
      op: "create",
      path: "registry",
      detail: `Create the profile ${tag}`,
      keys: [`profiles.${tag}`],
    });
  }
  steps.push({
    op: "update",
    path: "registry",
    detail: `Add ${server} to the profile ${tag}`,
    keys: [`profiles.${tag}.servers`],
  });
  return {
    summary: exists ? `Add ${server} to ${tag}` : `Create ${tag} and add ${server}`,
    steps,
    effects: {},
    warnings: exists
      ? []
      : [`There is no profile called ${tag}, so confirming creates one.`],
    undo: `Switch ${server} off in the profile ${tag}`,
  };
}

export function tagResultPlan(result: Data, tag: string, done: boolean): PlanV1 {
  const tags = strings(result.profileTags);
  const name = text(result.name) ?? "The server";
  return {
    summary: done
      ? `${name} is in ${plural(tags.length, "profile")}`
      : "Add to a profile",
    steps: [
      {
        op: "note",
        path: tag,
        detail: tags.length ? `Profiles now: ${tags.join(", ")}` : "In no profile now",
      },
    ],
    effects: {},
    warnings: [],
    undo: "",
  };
}

// ---- mode -------------------------------------------------------------------------------

export const MODES = ["auto", "direct", "router", "legacy", "bridge"] as const;
export type Mode = (typeof MODES)[number];

export const MODE_NOTE =
  "Toolport+ has no per-server proxy modes: the shared gateway exposes every server.";

export function setModePlan(server: string, mode: string): PlanV1 {
  return {
    summary: `Set the mode of ${server} to ${mode}`,
    steps: [{ op: "note", path: server, detail: MODE_NOTE }],
    effects: {},
    warnings: ["Nothing is stored: this mode has no effect in Toolport+."],
    undo: "",
  };
}

export function modeResultPlan(result: Data): PlanV1 {
  const name = text(result.name) ?? "The server";
  const changed = result.changed === true;
  return {
    summary: changed ? `${name} changed` : `${name} is unchanged`,
    steps: [{ op: "note", path: name, detail: text(result.note) ?? MODE_NOTE }],
    effects: {},
    warnings: [],
    undo: "",
  };
}

// ---- fork sync --------------------------------------------------------------------------

export interface ForkSyncOptions {
  mode: "rebase" | "merge" | "onto-author";
  upstreamRemote: string;
  upstreamBranch: string;
  authorEmail: string;
  push: boolean;
  runPostUpdate: boolean;
}

export const forkSyncDefaults: ForkSyncOptions = {
  mode: "rebase",
  upstreamRemote: "upstream",
  upstreamBranch: "main",
  authorEmail: "",
  push: false,
  runPostUpdate: false,
};

/** The stored upstream when the server has one; otherwise the remote the checkout calls
 * `upstream` (or the first remote that is not the fork's own) and a branch that remote has. */
export function forkSyncDefaultsFor(source: SourceInfo | undefined): ForkSyncOptions {
  if (!source) return forkSyncDefaults;
  const remote =
    source.upstream?.remote ??
    (source.remotes.includes("upstream")
      ? "upstream"
      : source.remotes.find((name) => name !== source.remote)) ??
    forkSyncDefaults.upstreamRemote;
  const known = source.branches[remote] ?? [];
  const branch =
    (source.upstream?.remote === remote ? source.upstream.branch : undefined) ??
    ["main", "master"].find((name) => known.includes(name)) ??
    known[0] ??
    forkSyncDefaults.upstreamBranch;
  return { ...forkSyncDefaults, upstreamRemote: remote, upstreamBranch: branch };
}

const REF = /^[^\s-][^\s]*$/;

export function forkSyncProblems(options: ForkSyncOptions): string[] {
  const problems: string[] = [];
  if (!REF.test(options.upstreamRemote.trim()))
    problems.push("Name the upstream remote.");
  if (!REF.test(options.upstreamBranch.trim()))
    problems.push("Name the upstream branch.");
  if (options.mode === "onto-author" && !options.authorEmail.trim())
    problems.push("Give the author email whose commits are kept.");
  return problems;
}

export function forkSyncArgs(
  name: string,
  options: ForkSyncOptions,
): Record<string, unknown> {
  const args: Record<string, unknown> = {
    name,
    upstream_remote: options.upstreamRemote.trim(),
    upstream_branch: options.upstreamBranch.trim(),
    mode: options.mode,
    confirm: true,
  };
  if (options.mode === "onto-author") args.author_email = options.authorEmail.trim();
  if (options.push) args.push = true;
  if (options.runPostUpdate) args.run_post_update = true;
  return args;
}

export function forkSyncPlan(
  name: string,
  git: GitState,
  options: ForkSyncOptions,
): PlanV1 {
  const upstream = `${options.upstreamRemote.trim()}/${options.upstreamBranch.trim()}`;
  const branch = git.branch ?? "the tracked branch";
  const steps: PlanStep[] = [
    { op: "note", path: git.path, detail: `Fetch ${options.upstreamRemote.trim()}` },
    {
      op: "create",
      path: git.path,
      detail: `Open a temporary worktree of ${branch}; no branch is created and the checkout is not switched`,
    },
    {
      op: "update",
      path: git.path,
      detail:
        options.mode === "rebase"
          ? `Rebase ${branch} onto ${upstream} there`
          : options.mode === "merge"
            ? `Merge ${upstream} into ${branch} there`
            : `Start at ${upstream} there and replay the commits of ${options.authorEmail.trim()}`,
    },
  ];
  for (const line of git.summaries.slice(0, 5)) {
    steps.push({ op: "note", path: git.path, detail: `Upstream: ${line}` });
  }
  if (options.runPostUpdate) {
    steps.push({
      op: "exec",
      path: git.path,
      detail: "Runs the stored update command (post_update) in the worktree",
    });
  }
  steps.push({
    op: "update",
    path: git.path,
    detail: `Move ${branch} to the result once it applies cleanly, then remove the worktree`,
  });
  if (options.push) {
    steps.push({
      op: "exec",
      path: git.path,
      detail: `Push ${branch} to the fork remote${options.mode === "merge" ? "" : " (force-with-lease, the history was rewritten)"}`,
    });
  }
  return {
    summary: `Sync ${name} with ${upstream}`,
    steps,
    effects: {},
    warnings: [
      "A conflict stops the sync and keeps the worktree; the live checkout and the branch stay as they are.",
    ],
    undo: git.branch ? `git branch -f ${git.branch} <old tip> (kept in the reflog)` : "",
  };
}

export function forkSyncResultPlan(result: Data, name: string): PlanV1 {
  const branch = text(result.branch) ?? "the tracked branch";
  const stale = strings(result.staleBranches).map<PlanStep>((stray) => ({
    op: "note",
    path: stray,
    detail: "Old sync branch; delete it yourself when you no longer need it",
  }));
  if (result.synced !== true) {
    const paths = strings(result.conflictedPaths);
    return {
      summary:
        result.conflict === true
          ? `The sync of ${name} stopped on a conflict`
          : `The sync of ${name} stopped: the update command failed`,
      steps: [
        ...paths.map<PlanStep>((path) => ({
          op: "note",
          path,
          detail: "Conflicted file",
        })),
        {
          op: "note",
          path: text(result.worktree),
          detail: text(result.next) ?? "Resolve the conflicts in the worktree.",
        },
        ...stale,
      ],
      effects: {},
      warnings: [`${branch} and the live checkout were not changed.`],
      undo: "",
    };
  }
  const picked = count(result.picked);
  const push = record(result.push);
  const upstream = text(result.upstream) ?? "its upstream";
  return {
    summary:
      result.upToDate === true
        ? `${name} is already up to date with ${upstream}`
        : `${name} synced: ${branch} now follows ${upstream}`,
    steps: [
      {
        op: "note",
        detail: `Mode ${text(result.mode) ?? "rebase"}${picked !== undefined ? `, ${plural(picked, "commit")} kept` : ""}`,
      },
      ...(push
        ? [
            {
              op: "note" as const,
              detail:
                push.pushed === true
                  ? `Pushed ${branch} to ${text(push.remote) ?? "the fork remote"}`
                  : `Push to ${text(push.remote) ?? "the fork remote"} failed: ${text(push.error) ?? "unknown error"}`,
            },
          ]
        : []),
      ...stale,
    ],
    effects: {},
    warnings: push && push.pushed !== true ? ["The branch is updated locally only."] : [],
    undo: "",
  };
}

// ---- set source -------------------------------------------------------------------------

export interface SetSourceOptions {
  remote: string;
  branch: string;
}

export function setSourceDefaults(source: SourceInfo): SetSourceOptions {
  return {
    remote: source.remote ?? source.remotes[0] ?? "",
    branch: source.branch ?? "",
  };
}

export function setSourceProblems(options: SetSourceOptions): string[] {
  const problems: string[] = [];
  if (!options.remote.trim()) problems.push("Pick or name a remote.");
  if (!options.branch.trim()) problems.push("Name a branch.");
  return problems;
}

export function setSourceArgs(
  name: string,
  options: SetSourceOptions,
): Record<string, unknown> {
  return {
    name,
    remote: options.remote.trim(),
    branch: options.branch.trim(),
    confirm: true,
  };
}

export function setSourcePlan(
  name: string,
  source: SourceInfo,
  options: SetSourceOptions,
): PlanV1 {
  const remote = options.remote.trim();
  const branch = options.branch.trim();
  const known = source.branches[remote] ?? [];
  const steps: PlanStep[] = [
    {
      op: "update",
      path: source.path ?? name,
      detail: `Point the stored source of ${name} at ${remote}/${branch}`,
      keys: [`servers.${name}.source`],
    },
  ];
  if (!known.includes(branch)) {
    steps.push({
      op: "note",
      path: source.path ?? name,
      detail: `${remote} has no local record of ${branch} yet; this may fetch it first`,
    });
  }
  const changes =
    (source.remote && source.remote !== remote) ||
    (source.branch && source.branch !== branch);
  return {
    summary: `Switch ${name} to ${remote}/${branch}`,
    steps,
    effects: {},
    warnings: changes
      ? []
      : ["This is already the stored source; confirming re-records the same values."],
    undo:
      source.remote && source.branch
        ? `Switch ${name} back to ${source.remote}/${source.branch}`
        : "",
  };
}

export function setSourceResultPlan(result: Data, name: string): PlanV1 {
  const source = record(result.source);
  const meta = record(source?.meta);
  const remote = text(meta?.remote) ?? "its remote";
  const branch = text(meta?.branch) ?? "its branch";
  return {
    summary: `${name} now points at ${remote}/${branch}`,
    steps: [{ op: "note", path: name, detail: `Stored source: ${remote}/${branch}` }],
    effects: {},
    warnings: [],
    undo: "",
  };
}
