import type { RepoDoneData, RepoPlan, RepoRowData, RepoState } from "../servers/data";
import type { CtlReply } from "./ctlWorld";

export interface RepoSeed {
  root?: string;
  branch?: string | null;
  upstream?: string | null;
  head?: string;
  upstreamHead?: string;
  behind?: number;
  ahead?: number;
  dirty?: number;
  commits?: Array<{ id: string; subject: string }>;
  files?: Array<{ status: string; path: string }>;
  /** The build command the person agreed to for this server earlier. */
  remembered?: string;
  /** Calls running in the gateways; `null` means no gateway answers. */
  inFlight?: number | null;
  checkedAt?: string | null;
  sharedWith?: string[];
  buildFails?: boolean;
  /** The next fetch fails with this reason. */
  fetchFails?: string;
}

interface Applied {
  from: string;
  to: string;
  build: string | null;
  behind: number;
  commits: Array<{ id: string; subject: string }>;
}

interface Repo extends Required<Omit<RepoSeed, "remembered" | "fetchFails">> {
  remembered: string | null;
  fetchFails: string | null;
  applied: Applied | null;
}

const NOW = "2026-11-18T10:02:00+01:00";

function stateOf(repo: Repo): RepoState {
  if (repo.branch === null) return "detached";
  if (repo.upstream === null) return "no_upstream";
  if (repo.behind === 0) return "up_to_date";
  if (repo.ahead > 0) return "diverged";
  return repo.dirty > 0 ? "dirty" : "behind";
}

function fail(code: string, message: string, data?: unknown): CtlReply {
  return { ok: false, error: { code, message }, ...(data ? { data } : {}) };
}

function refusalOf(repo: Repo): RepoPlan["refusal"] {
  const state = stateOf(repo);
  const text: Partial<Record<RepoState, string>> = {
    up_to_date: `${repo.branch} is up to date with ${repo.upstream}`,
    detached:
      "HEAD is detached; check out the branch first, Toolport only updates a branch",
    no_upstream: `${repo.branch} tracks no remote branch, so there is nothing to update from`,
    diverged: `your branch diverged: ${repo.ahead} local commit(s) and ${repo.behind} new commit(s) on ${repo.upstream}; Toolport only fast-forwards, so rebase or merge it yourself`,
    dirty: `local changes in ${repo.dirty} tracked file(s) block the update; commit or stash them yourself`,
    busy: "a merge is open in this checkout; finish or abort it first",
  };
  const message = text[state];
  return message
    ? { code: state === "up_to_date" ? "up_to_date" : state, message }
    : null;
}

function flagValue(argv: string[], flag: string): string | null {
  const at = argv.indexOf(flag);
  return at >= 0 && argv[at + 1] ? argv[at + 1] : null;
}

/** `server repo ...` of the in-memory toolportctl: checkouts by server id that move when an update
 * is applied and move back on undo. */
export function createRepoWorld(
  seeds: Record<string, RepoSeed>,
  find: (key: string) => { id: string; name: string } | undefined,
) {
  const repos = new Map<string, Repo>(
    Object.entries(seeds).map(([id, seed]) => [
      id,
      {
        root: "/Users/example/work/alpha-api",
        branch: "main",
        upstream: "origin/main",
        head: "4f2a1c3aa11",
        upstreamHead: "9be07d2bb22",
        behind: 0,
        ahead: 0,
        dirty: 0,
        commits: [],
        files: [],
        inFlight: null,
        checkedAt: NOW,
        sharedWith: [],
        buildFails: false,
        ...seed,
        remembered: seed.remembered ?? null,
        fetchFails: seed.fetchFails ?? null,
        applied: null,
      },
    ]),
  );
  const short = (id: string | null) => (id ?? "").slice(0, 8);

  function row(id: string, name: string, repo: Repo): RepoRowData {
    return {
      server: name,
      id,
      root: repo.root,
      branch: repo.branch,
      head: repo.head,
      remote: "origin",
      url: "https://git.example.invalid/alpha-api.git",
      upstream: repo.upstream,
      upstreamHead: repo.upstreamHead,
      ahead: repo.ahead,
      behind: repo.behind,
      dirty: repo.dirty,
      unpushed: repo.ahead,
      state: stateOf(repo),
      checkedAt: repo.checkedAt,
      error: repo.fetchFails,
    };
  }

  function target(key: string | undefined) {
    if (!key) return fail("usage", "usage: server repo <verb> <id|name>");
    const server = find(key);
    if (!server) return fail("not_found", `no server '${key}'`);
    const repo = repos.get(server.id);
    if (!repo)
      return fail(
        "not_found",
        `${server.name} does not run from a git checkout on this machine`,
      );
    return { server, repo };
  }

  function buildOf(repo: Repo, argv: string[]) {
    const given = flagValue(argv, "--build");
    const command = argv.includes("--no-build") ? null : (given ?? repo.remembered);
    return { command, remembered: command !== null && command === repo.remembered };
  }

  function plan(id: string, name: string, repo: Repo, argv: string[]): RepoPlan {
    const build = buildOf(repo, argv);
    return {
      server: name,
      id,
      root: repo.root,
      branch: repo.branch,
      upstream: repo.upstream,
      from: repo.head,
      to: repo.upstreamHead,
      commits: stateOf(repo) === "behind" ? repo.commits : [],
      commitsTotal: repo.behind,
      files: stateOf(repo) === "behind" ? repo.files : [],
      filesTotal: stateOf(repo) === "behind" ? repo.files.length : 0,
      sharedWith: repo.sharedWith,
      refusal: refusalOf(repo),
      build: { ...build, timeoutSeconds: 120 },
      inFlight: repo.inFlight,
      planHash: `sha256:${short(repo.head)}${short(repo.upstreamHead)}${build.command ?? ""}`,
      undo: `toolportctl server repo undo ${name}`,
    };
  }

  function done(
    id: string,
    name: string,
    repo: Repo,
    applied: Applied,
    argv: string[],
    back: boolean,
  ) {
    const command = back ? applied.build : buildOf(repo, argv).command;
    const run =
      command && !(back && argv.includes("--no-build"))
        ? {
            command,
            ok: !repo.buildFails,
            exitCode: repo.buildFails ? 3 : 0,
            elapsedMs: 800,
            output: repo.buildFails ? "error: could not compile alpha-api" : "built",
          }
        : null;
    let restart: RepoDoneData["restart"] = null;
    if (run && !run.ok) {
      restart = {
        status: "skipped",
        detail: "the build failed, so the server was not restarted",
      };
    } else if (!argv.includes("--no-restart")) {
      restart =
        (repo.inFlight ?? 0) > 0 && !argv.includes("--restart-now")
          ? {
              status: "busy",
              detail: `a call is running; restart when it ends with: toolportctl server restart ${name}`,
            }
          : { status: "restarted", detail: `Restarted ${name} in 1 gateway process` };
    }
    if (!back && run?.ok && command) repo.remembered = command;
    return {
      ok: !(run && !run.ok),
      data: {
        applied: true,
        server: name,
        id,
        root: repo.root,
        branch: repo.branch ?? "",
        from: back ? applied.to : applied.from,
        to: back ? applied.from : applied.to,
        build: run,
        restart,
        warnings: [],
        undo: back
          ? `toolportctl server repo update ${name}`
          : `toolportctl server repo undo ${name}`,
      } satisfies RepoDoneData,
    } satisfies CtlReply;
  }

  return {
    run(argv: string[]): CtlReply {
      const [verb, key] = argv;
      if (verb === "status") {
        if (key) {
          const found = target(key);
          if ("ok" in found) return found;
          return {
            ok: true,
            data: { repos: [row(found.server.id, found.server.name, found.repo)] },
          };
        }
        return {
          ok: true,
          data: {
            repos: [...repos].map(([id, repo]) => row(id, find(id)?.name ?? id, repo)),
          },
        };
      }
      if (verb === "check") {
        const results = [...repos].map(([id, repo]) => {
          const name = find(id)?.name ?? id;
          if (repo.fetchFails) return { server: name, ok: false, error: repo.fetchFails };
          repo.checkedAt = NOW;
          return { server: name, ok: true };
        });
        const failed = results.some((r) => !r.ok);
        return { ok: !failed, data: { checkedAt: NOW, repos: results } };
      }
      const found = target(key);
      if ("ok" in found) return found;
      const { server, repo } = found;
      if (verb === "update") {
        if (argv.includes("--restart-now") && argv.includes("--no-restart"))
          return fail("usage", "usage: server repo update <id|name>");
        const current = plan(server.id, server.name, repo, argv);
        if (argv.includes("--dry-run")) return { ok: true, data: { plan: current } };
        if (current.refusal) {
          if (current.refusal.code === "up_to_date")
            return {
              ok: true,
              data: {
                applied: false,
                server: server.name,
                id: server.id,
                build: null,
                restart: null,
                warnings: [],
              },
            };
          return fail("update_refused", current.refusal.message, {
            reason: current.refusal.code,
          });
        }
        const hash = flagValue(argv, "--plan-hash");
        if (hash && hash !== current.planHash)
          return fail(
            "conflict",
            "the plan has changed since it was reviewed; review it again",
            {
              planHash: current.planHash,
            },
          );
        const applied: Applied = {
          from: repo.head,
          to: repo.upstreamHead,
          build: current.build.command,
          behind: repo.behind,
          commits: repo.commits,
        };
        repo.head = repo.upstreamHead;
        repo.behind = 0;
        repo.applied = applied;
        return done(server.id, server.name, repo, applied, argv, false);
      }
      if (verb === "undo") {
        const applied = repo.applied;
        if (!applied)
          return fail("not_found", `no update of ${server.name} to take back`);
        if (argv.includes("--dry-run"))
          return {
            ok: true,
            data: {
              plan: {
                server: server.name,
                id: server.id,
                from: applied.to,
                to: applied.from,
                build: applied.build,
                refusal: null,
              },
            },
          };
        repo.head = applied.from;
        repo.behind = applied.behind;
        repo.applied = null;
        return done(server.id, server.name, repo, applied, argv, true);
      }
      return fail("usage", `unsupported in the fixture: server repo ${verb}`);
    },
  };
}

export type RepoWorld = ReturnType<typeof createRepoWorld>;
