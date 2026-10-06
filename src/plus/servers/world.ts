import addTagGolden from "../../../src-tauri/tests/fixtures/ctl-envelopes/mcp-call.servers_add_profile_tag.json";
import applyGolden from "../../../src-tauri/tests/fixtures/ctl-envelopes/mcp-call.servers_apply_update.json";
import checkGolden from "../../../src-tauri/tests/fixtures/ctl-envelopes/mcp-call.servers_check_updates.json";
import detectGolden from "../../../src-tauri/tests/fixtures/ctl-envelopes/mcp-call.servers_detect_source.json";
import forkGolden from "../../../src-tauri/tests/fixtures/ctl-envelopes/mcp-call.servers_fork_sync.json";
import gitGolden from "../../../src-tauri/tests/fixtures/ctl-envelopes/mcp-call.servers_git_status.json";
import removeGolden from "../../../src-tauri/tests/fixtures/ctl-envelopes/mcp-call.servers_remove_profile_tag.json";
import modeGolden from "../../../src-tauri/tests/fixtures/ctl-envelopes/mcp-call.servers_set_mode.json";
import type { ProfileLsData } from "../bridge/data";
import { CtlReplyFailure } from "../fixtures/ctlReply";
import { serversWorld } from "../fixtures/servers";

/** A small stateful world behind `toolportctl mcp call` for the servers tools: the replies
 * have the shape of the real goldens, and an applied write (an update, a sync, a tag) changes
 * what the next read returns. */

type Golden = { envelope: { data: { tier: number; result: Record<string, unknown> } } };
const tierOf = (golden: unknown) => (golden as Golden).envelope.data.tier;
const resultOf = (golden: unknown) => (golden as Golden).envelope.data.result;

export interface WorldServer {
  name: string;
  id: string;
  kind: "git" | "npx" | "remote" | "unknown";
  stored: boolean;
  update: "update-available" | "up-to-date" | "skipped";
  message: string;
  postUpdate?: string;
  git?: {
    path: string;
    branch: string;
    dirty: boolean;
    ahead: number;
    behind: number;
    summaries: string[];
    /** Defaults to "origin" when left out. */
    remote?: string;
    /** Defaults to `[remote]` when left out. */
    remotes?: string[];
    /** The branches known locally for each remote; a remote left out here reports none,
     * the way a remote nobody fetched yet does in the real registry. */
    branches?: Record<string, string[]>;
  };
}

export const WORLD_SERVERS: WorldServer[] = [
  {
    name: "docs-search",
    id: "srv-docs",
    kind: "git",
    stored: true,
    update: "update-available",
    message: "2 commits behind origin/main",
    postUpdate: "npm run build",
    git: {
      path: "/fixture/home/src/docs-search",
      branch: "main",
      dirty: false,
      ahead: 1,
      behind: 2,
      summaries: ["6a4a877 upstream change", "91be2c0 fix the indexer"],
      remote: "origin",
      remotes: ["origin", "upstream"],
      branches: { origin: ["main"], upstream: [] },
    },
  },
  {
    name: "wiki-reader",
    id: "srv-wiki",
    kind: "npx",
    stored: true,
    update: "up-to-date",
    message: "latest on each start",
  },
  {
    name: "corp-tools",
    id: "srv-corp",
    kind: "remote",
    stored: false,
    update: "skipped",
    message: "remote server, nothing to update",
  },
  {
    name: "mail-bridge",
    id: "srv-mail",
    kind: "unknown",
    stored: false,
    update: "skipped",
    message: "unknown source (synthetic fixture); run update --init",
  },
];

const body = (
  tool: string,
  golden: unknown,
  result: Record<string, unknown>,
): Record<string, unknown> => ({
  isError: false,
  result,
  tier: tierOf(golden),
  tool,
});

const refused = (tool: string, tier: number) =>
  new CtlReplyFailure(
    "refused",
    `Refused: ${tool} (tier ${tier}). Pass confirm=true to proceed.`,
  );

export function createToolsWorld(seed: WorldServer[] = WORLD_SERVERS) {
  const servers = structuredClone(seed);
  const profiles: ProfileLsData = structuredClone(serversWorld.profileLs);
  const calls: Array<{ tool: string; args: Record<string, unknown> }> = [];
  const state = { conflictNext: false };

  const find = (name: unknown) =>
    servers.find((server) => server.name === name || server.id === name);

  function gitRemotes(server: WorldServer) {
    const git = server.git!;
    const remote = git.remote ?? "origin";
    return {
      remote,
      remotes: git.remotes ?? [remote],
      branches: git.branches ?? { [remote]: [git.branch] },
    };
  }

  function sourceMeta(server: WorldServer) {
    if (server.kind === "git" && server.git)
      return {
        branch: server.git.branch,
        path: server.git.path,
        remote: gitRemotes(server).remote,
        type: "git",
      };
    if (server.kind === "unknown") return { type: "unknown", reason: "no source stored" };
    return { type: server.kind };
  }

  function updateRow(server: WorldServer, applied: boolean) {
    const behind = server.git?.behind ?? 0;
    return {
      detected: server.stored,
      id: server.id,
      kind: server.kind,
      message: applied ? "updated" : server.message,
      status: applied ? "updated" : server.update,
      ...(server.update === "update-available" && !applied
        ? {
            behind,
            plan: [
              `git pull --ff-only (${behind} commits)`,
              ...(server.postUpdate
                ? [`post_update: ${server.postUpdate} (not run without --allow-commands)`]
                : []),
            ],
          }
        : {}),
      ...(applied && server.postUpdate
        ? {
            steps: [{ name: "post_update", ok: true, detail: server.postUpdate }],
          }
        : {}),
    };
  }

  function profileOf(tag: string) {
    const key = tag.trim();
    return (
      profiles.profiles.find((profile) => profile.id === key) ??
      profiles.profiles.find(
        (profile) => profile.name.toLowerCase() === key.toLowerCase(),
      )
    );
  }

  const tagsOf = (id: string) =>
    profiles.profiles
      .filter((profile) => profile.servers.some((member) => member.id === id))
      .map((profile) => profile.name);

  function call(tool: string, args: Record<string, unknown>): unknown {
    calls.push({ tool, args });
    const server = find(args.name);
    const missing = () =>
      new CtlReplyFailure("not_found", `server not found: ${String(args.name ?? "")}`);
    switch (tool) {
      case "servers_detect_source": {
        if (!server) return missing();
        const known =
          server.kind === "git" && server.git
            ? {
                remotes: gitRemotes(server).remotes,
                branches: gitRemotes(server).branches,
              }
            : { remotes: [], branches: {} };
        return body(tool, detectGolden, {
          ...resultOf(detectGolden),
          name: server.name,
          stored: server.stored,
          detected: { kind: server.kind, meta: sourceMeta(server) },
          ...known,
        });
      }
      case "servers_set_source": {
        if (args.confirm !== true) return refused(tool, 3);
        if (!server) return missing();
        if (!server.git)
          return new CtlReplyFailure(
            "invalid_input",
            `server ${server.name} is not git-backed`,
          );
        const remotes = gitRemotes(server).remotes;
        const remote = String(args.remote ?? gitRemotes(server).remote);
        if (!remotes.includes(remote))
          return new CtlReplyFailure("invalid_arguments", `no such remote: ${remote}`);
        const branch = String(args.branch ?? server.git.branch);
        server.git.remote = remote;
        server.git.branch = branch;
        return {
          isError: false,
          result: {
            name: server.name,
            source: {
              kind: "git",
              meta: { branch, drift: false, path: server.git.path, remote, type: "git" },
            },
          },
          tier: 3,
          tool,
        };
      }
      case "servers_git_status": {
        if (!server) return missing();
        if (!server.git)
          return body(tool, gitGolden, {
            isGit: false,
            message: `server is tracked as ${server.kind}, not git`,
            name: server.name,
          });
        return body(tool, gitGolden, {
          ...resultOf(gitGolden),
          name: server.name,
          path: server.git.path,
          branch: server.git.branch,
          remoteRef: "origin/main",
          ahead: server.git.ahead,
          behind: server.git.behind,
          dirty: server.git.dirty,
          summaries: server.git.summaries,
        });
      }
      case "servers_check_updates": {
        const rows = (server ? [server] : servers).map((row) => updateRow(row, false));
        const counts: Record<string, number> = {};
        for (const row of rows) counts[row.status] = (counts[row.status] ?? 0) + 1;
        return body(tool, checkGolden, {
          ...resultOf(checkGolden),
          counts,
          servers: rows,
        });
      }
      case "servers_apply_update": {
        if (args.confirm !== true) return refused(tool, tierOf(applyGolden));
        if (!server) return missing();
        const row = updateRow(server, server.update === "update-available");
        if (server.update === "update-available") {
          server.update = "up-to-date";
          server.message = "up to date";
          if (server.git) server.git.behind = 0;
        }
        return body(tool, applyGolden, {
          ...resultOf(applyGolden),
          counts: { [row.status]: 1 },
          servers: [row],
        });
      }
      case "servers_add_profile_tag":
      case "servers_remove_profile_tag": {
        const add = tool === "servers_add_profile_tag";
        if (!server) return missing();
        const tag = String(args.profile_tag ?? "").trim();
        if (!tag) return new CtlReplyFailure("invalid_arguments", "profile_tag is empty");
        let profile = profileOf(tag);
        if (!profile && add) {
          profile = {
            id: tag.toLowerCase().replace(/\W+/g, "-"),
            name: tag,
            active: false,
            clients: [],
            servers: [],
          };
          profiles.profiles.push(profile);
        }
        if (!profile)
          return new CtlReplyFailure("not_found", `profile not found: ${tag}`);
        const has = profile.servers.some((member) => member.id === server.id);
        if (add && !has)
          profile.servers.push({
            id: server.id,
            name: server.name,
            target: "node server.js",
          });
        if (!add && has)
          profile.servers = profile.servers.filter((member) => member.id !== server.id);
        return body(tool, add ? addTagGolden : removeGolden, {
          name: server.name,
          profileTags: tagsOf(server.id),
        });
      }
      case "servers_set_mode": {
        if (args.confirm !== true) return refused(tool, tierOf(modeGolden));
        if (!server) return missing();
        return body(tool, modeGolden, {
          ...resultOf(modeGolden),
          name: server.name,
          mode: String(args.mode),
        });
      }
      case "servers_fork_sync": {
        if (args.confirm !== true) return refused(tool, tierOf(forkGolden));
        if (!server) return missing();
        if (!server.git)
          return new CtlReplyFailure(
            "invalid_input",
            `server ${server.name} is not git-backed`,
          );
        if (server.git.dirty)
          return new CtlReplyFailure("conflict", "working tree has uncommitted changes");
        const branch = server.git.branch;
        if (state.conflictNext) {
          state.conflictNext = false;
          return body(tool, forkGolden, {
            synced: false,
            conflict: true,
            branch,
            conflictedPaths: ["src/index.ts"],
            worktree: "/home/demo/docs-search/.git/toolport-sync/main-20261005",
            next: `${branch} and the live checkout are unchanged; resolve the conflicts in the worktree, git add the files, run git rebase --continue there, or drop it with git worktree remove --force`,
          });
        }
        server.git.behind = 0;
        return body(tool, forkGolden, {
          ...resultOf(forkGolden),
          branch,
          mode: String(args.mode ?? "rebase"),
          upstream: `${String(args.upstream_remote ?? "upstream")}/${String(args.upstream_branch ?? "main")}`,
          ...(args.push === true ? { push: { pushed: true, remote: "origin" } } : {}),
        });
      }
      default:
        return new CtlReplyFailure("usage", `unknown tool: ${tool}`);
    }
  }

  return {
    calls,
    servers,
    call,
    profileLs: (): ProfileLsData => structuredClone(profiles),
    /** The next fork sync stops on a conflict. */
    conflictNext: () => {
      state.conflictNext = true;
    },
    /** The reply to `mcp call <tool> --args-stdin`; an argv without stdin (the dev browser
     * fixture carries none) asks about the first server of the world. */
    reply(argv: string[], stdin: string | null | undefined): unknown {
      const args = stdin
        ? (JSON.parse(stdin) as Record<string, unknown>)
        : { name: servers[0]?.name, confirm: true, profile_tag: "Work", mode: "direct" };
      return call(argv[2], args);
    },
  };
}

export type ToolsWorld = ReturnType<typeof createToolsWorld>;

export const TOOL_NAMES = [
  "servers_detect_source",
  "servers_set_source",
  "servers_git_status",
  "servers_check_updates",
  "servers_apply_update",
  "servers_add_profile_tag",
  "servers_remove_profile_tag",
  "servers_set_mode",
  "servers_fork_sync",
] as const;
