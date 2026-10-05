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
  reason?: string;
}

export function sourceOf(result: Data): SourceInfo {
  const detected = record(result.detected);
  const meta = record(detected?.meta);
  return {
    kind: text(detected?.kind) ?? "unknown",
    stored: result.stored === true,
    path: text(meta?.path),
    branch: text(meta?.branch),
    reason: text(meta?.reason),
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
  mode: "rebase" | "onto-author";
  upstreamRemote: string;
  upstreamBranch: string;
  targetBranch: string;
  authorEmail: string;
  runPostUpdate: boolean;
}

export const forkSyncDefaults: ForkSyncOptions = {
  mode: "rebase",
  upstreamRemote: "upstream",
  upstreamBranch: "main",
  targetBranch: "",
  authorEmail: "",
  runPostUpdate: false,
};

const REF = /^[^\s-][^\s]*$/;

export function forkSyncProblems(options: ForkSyncOptions): string[] {
  const problems: string[] = [];
  if (!REF.test(options.upstreamRemote.trim()))
    problems.push("Name the upstream remote.");
  if (!REF.test(options.upstreamBranch.trim()))
    problems.push("Name the upstream branch.");
  if (options.targetBranch.trim() && !REF.test(options.targetBranch.trim()))
    problems.push("The new branch name cannot contain spaces or start with a dash.");
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
  if (options.targetBranch.trim()) args.target_branch = options.targetBranch.trim();
  if (options.mode === "onto-author") args.author_email = options.authorEmail.trim();
  if (options.runPostUpdate) args.run_post_update = true;
  return args;
}

export function forkSyncPlan(
  name: string,
  git: GitState,
  options: ForkSyncOptions,
): PlanV1 {
  const upstream = `${options.upstreamRemote.trim()}/${options.upstreamBranch.trim()}`;
  const target =
    options.targetBranch.trim() || `${git.branch ?? "the current branch"}-synced-<date>`;
  const steps: PlanStep[] = [
    { op: "note", path: git.path, detail: `Fetch ${options.upstreamRemote.trim()}` },
    {
      op: "create",
      path: git.path,
      detail: `Create the branch ${target} and switch the checkout to it`,
    },
    {
      op: "update",
      path: git.path,
      detail:
        options.mode === "rebase"
          ? `Rebase ${target} onto ${upstream}`
          : `Start ${target} at ${upstream} and replay the commits of ${options.authorEmail.trim()}`,
    },
  ];
  for (const line of git.summaries.slice(0, 5)) {
    steps.push({ op: "note", path: git.path, detail: `Upstream: ${line}` });
  }
  if (options.runPostUpdate) {
    steps.push({
      op: "exec",
      path: git.path,
      detail: "Runs the stored update command (post_update) when the sync worked",
    });
  }
  return {
    summary: `Sync ${name} with ${upstream}`,
    steps,
    effects: {},
    warnings: [
      "A conflict stops the sync half way; the checkout stays on the new branch for you to resolve.",
    ],
    undo: git.branch ? `Switch the checkout back to ${git.branch}` : "",
  };
}

export function forkSyncResultPlan(result: Data, name: string): PlanV1 {
  const branch = text(result.branch) ?? "the new branch";
  if (result.synced !== true) {
    const paths = strings(result.conflictedPaths);
    return {
      summary: `The sync of ${name} stopped on a conflict`,
      steps: [
        ...paths.map<PlanStep>((path) => ({
          op: "note",
          path,
          detail: "Conflicted file",
        })),
        {
          op: "note",
          detail: text(result.next) ?? "Resolve the conflicts in the checkout.",
        },
      ],
      effects: {},
      warnings: [`The checkout is on ${branch} with the sync unfinished.`],
      undo: "",
    };
  }
  const picked = count(result.picked);
  return {
    summary: `${name} synced on ${branch}`,
    steps: [
      {
        op: "note",
        detail: `Mode ${text(result.mode) ?? "rebase"}${picked !== undefined ? `, ${plural(picked, "commit")} kept` : ""}`,
      },
      ...(text(result.previousBranch)
        ? [
            {
              op: "note" as const,
              detail: `Before the sync the checkout was on ${text(result.previousBranch)}`,
            },
          ]
        : []),
    ],
    effects: {},
    warnings: [],
    undo: "",
  };
}
