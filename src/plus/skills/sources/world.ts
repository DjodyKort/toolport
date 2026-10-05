/** A stateful version of the Sources fixtures, shaped like the goldens of `sources ls`,
 * `sources root` and `library`: an applied `sources root add|rm` changes what the next
 * `sources root ls` answers, an applied `library pull` leaves the clone level with its remote
 * and an applied `library push` leaves nothing to push; a preview never changes anything.
 * Browser safe: no node imports. The folders are fixed because a fixture row is keyed by its
 * argv. */
import type { SourcesLsData, SourcesRootLsData } from "../../bridge/data";
import type {
  LibraryChecks,
  LibraryPullData,
  LibraryPushData,
  LibraryStatusData,
} from "../../types/library";
import { Failure } from "../failure";
import {
  plusSourcesFixture,
  plusSourcesItemsFixture,
  plusSourcesRootFixture,
} from "../../fixtures/sources";

export const ADD_ROOT = "/home/demo/work/client-repo";
export const REMOVE_ROOT = "/home/demo/dups";
const CONFIG = "/home/demo/.config/toolport/context.json";
const BACKUP = "/home/demo/.cache/toolport/context/backups/20261004-100000/context.json";

type Roots = SourcesRootLsData["roots"];

/** Where the library clone starts. The world does not model a diverged clone: a pull with
 * commits waiting to be pushed still succeeds. */
export interface LibraryWorld {
  /** Commits on the remote that the clone lacks (default 2). */
  behind?: number;
  /** Commits here that are not pushed (default 0). */
  ahead?: number;
  /** Changed files that are not committed (default 0); a pull refuses while there are any. */
  uncommitted?: number;
  /** One commit to push holds a secret, so the scan blocks the push. */
  leak?: boolean;
  /** False: the clone has no remote. */
  remote?: boolean;
}

const LIBRARY = "/home/demo/lib/ai-skills";
const REMOTE = "https://git.example.com/demo/ai-skills.git";
const BASE = "789e5c01712e1a018cbf8f9c24ea2b964678bda9";
const FETCHED = "2026-10-04T09:30:00Z";
const FETCHED_NOW = "2026-10-05T10:00:00Z";
const INCOMING = [
  { sha: "628cdc8", subject: "Add notes-two", name: "notes-two" },
  { sha: "e3bf666", subject: "Add notes-one", name: "notes-one" },
];
const LEAKY = { sha: "13e1696", subject: "Add leaky" };
const FINDING = {
  commit: "13e1696",
  file: "skills/leaky/SKILL.md",
  line: 5,
  rule: "github-token",
};
const CLONE = {
  path: "/home/demo/dups/ai-skills-copy",
  skills: 2,
  ahead: 0,
  behind: 0,
  sameRemote: true,
};

const shorthand = (roots: Roots) =>
  JSON.stringify(
    roots
      .filter((root) => root.origin === "config")
      .map((root) => root.path.replace("/home/demo", "~")),
  );

export function createSourcesWorld(
  library: LibraryWorld = {},
): Array<[string, () => unknown]> {
  let roots: Roots = plusSourcesRootFixture.roots.map((root) => ({ ...root }));
  const rows = new Map<string, () => unknown>();
  const on = (argv: string, reply: () => unknown) => rows.set(argv, reply);

  const lib = {
    behind: library.behind ?? 2,
    ahead: Math.max(library.ahead ?? 0, library.leak ? 1 : 0),
    uncommitted: library.uncommitted ?? 0,
    remote: library.remote !== false,
    fetched: FETCHED,
  };
  const levelled = (data: SourcesLsData): SourcesLsData => ({
    ...data,
    sources: data.sources.map((row) =>
      row.id === "library" && row.freshness
        ? {
            ...row,
            freshness: { ...row.freshness, behind: lib.behind, ahead: lib.ahead },
          }
        : row,
    ),
  });
  on("sources ls", () => levelled(plusSourcesFixture));
  on("sources ls --refresh", () => levelled(plusSourcesFixture));
  for (const row of plusSourcesFixture.sources)
    on(`sources ls --source ${row.id} --items`, () => {
      const [shown] = levelled({ ...plusSourcesFixture, sources: [row] }).sources;
      return {
        ...plusSourcesItemsFixture,
        sources: [shown],
        items: (plusSourcesItemsFixture.items ?? []).filter((i) => i.sourceId === row.id),
      };
    });
  on("sources root ls", () => ({ roots }));

  const change = (path: string, add: boolean, dryRun: boolean) => {
    const after = add
      ? [...roots, { path, origin: "config" as const, exists: true, repo: true }]
      : roots.filter((root) => root.path !== path);
    const plan = {
      summary: `${add ? "add" : "remove"} source root ${path}`,
      steps: [
        {
          op: "merge" as const,
          path: CONFIG,
          detail: `${add ? "add" : "remove"} ${path} ${add ? "to" : "from"} sourceRoots`,
          keys: ["sourceRoots"],
          diff: { before: shorthand(roots), after: shorthand(after) },
        },
      ],
      effects: {},
      warnings: [] as string[],
      undo: `toolportctl sources root ${add ? "rm" : "add"} ${path}`,
    };
    if (!dryRun) roots = after;
    return {
      dryRun,
      plan,
      result: dryRun
        ? null
        : {
            applied: true as const,
            changed: [CONFIG],
            undo: plan.undo,
            backups: [BACKUP],
          },
    };
  };
  for (const [path, add] of [
    [ADD_ROOT, true],
    [REMOVE_ROOT, false],
  ] as const) {
    const verb = add ? "add" : "rm";
    on(`sources root ${verb} ${path} --dry-run`, () => change(path, add, true));
    on(`sources root ${verb} ${path}`, () => change(path, add, false));
  }

  const status = (fetch: boolean): LibraryStatusData => ({
    repo: LIBRARY,
    remote: lib.remote ? REMOTE : null,
    branch: "main",
    upstream: lib.remote ? "origin/main" : null,
    ahead: lib.ahead,
    behind: lib.behind,
    uncommitted: lib.uncommitted,
    lastFetch: lib.fetched,
    fetch: { requested: fetch, ok: fetch ? true : null, error: null },
    auth: { method: "git-credential", ok: lib.remote, checked: fetch },
    duplicateClones: [CLONE],
  });
  on("library status", () => status(false));
  on("library status --fetch", () => {
    lib.fetched = FETCHED_NOW;
    return status(true);
  });

  const undoPull = `git -C '${LIBRARY}' reset --hard ${BASE}`;
  const pullPlan = (): LibraryPullData => {
    const steps = [
      {
        op: "exec" as const,
        path: LIBRARY,
        detail: "git fetch origin (refs only, network)",
      },
      { op: "exec" as const, path: LIBRARY, detail: "git merge --ff-only origin/main" },
      ...INCOMING.slice(0, lib.behind).map((c) => ({
        op: "note" as const,
        detail: `${c.sha} ${c.subject}`,
      })),
    ];
    return {
      dryRun: true,
      repo: LIBRARY,
      plan: {
        summary:
          lib.behind === 0
            ? "Already up to date with origin/main"
            : `Fast-forward ${lib.behind} commit(s) from origin/main`,
        steps,
        effects: {},
        warnings: [],
        undo: undoPull,
      },
    };
  };
  const refusedPull = () =>
    lib.uncommitted > 0
      ? new Failure(
          "refused",
          `${lib.uncommitted} uncommitted change(s) in ${LIBRARY}; commit or stash them first`,
        )
      : null;
  on("library pull --dry-run", () => refusedPull() ?? pullPlan());
  on("library pull", () => {
    const refused = refusedPull();
    if (refused) return refused;
    const taken = INCOMING.slice(0, lib.behind);
    lib.behind = 0;
    lib.fetched = FETCHED_NOW;
    return {
      dryRun: false,
      repo: LIBRARY,
      pulled: taken.length > 0,
      commits: taken.length,
      result: {
        applied: true as const,
        backups: [],
        changed: taken.map((c) => `${LIBRARY}/skills/${c.name}/SKILL.md`),
        undo: undoPull,
      },
    } satisfies LibraryPullData;
  });

  const outgoing = () => [
    ...(library.leak ? [LEAKY] : []),
    ...Array.from(
      { length: Math.max(0, lib.ahead - (library.leak ? 1 : 0)) },
      (_, i) => ({
        sha: i === 0 ? "7caa837" : `7caa83${i}`,
        subject: i === 0 ? "Add new-local" : `Add new-local-${i + 1}`,
      }),
    ),
  ];
  const checks = (): LibraryChecks => ({
    audit: { ran: true, skills: 4, high: 0, findings: [] },
    gitleaks: {
      available: false,
      ran: false,
      error: "not installed",
      count: 0,
      findings: [],
    },
    builtinScan: library.leak
      ? { count: 1, findings: [FINDING] }
      : { count: 0, findings: [] },
    blocked: !!library.leak,
  });
  const nothing = () => lib.ahead === 0 && lib.uncommitted === 0;
  const undoPush = `git -C '${LIBRARY}' revert --no-edit ${BASE}..HEAD`;
  const pushPlan = (): LibraryPushData => {
    const commits = outgoing();
    const count = commits.length + (lib.uncommitted > 0 ? 1 : 0);
    return {
      dryRun: true,
      repo: LIBRARY,
      branch: "main",
      pushed: false,
      commits,
      checks: checks(),
      plan: {
        summary: nothing() ? "Nothing to push" : `Push ${count} commit(s) to origin/main`,
        steps: [
          { op: "note", detail: "audit: 4 skill(s), 0 finding(s), 0 high" },
          {
            op: "note",
            detail: `gitleaks not installed; built-in scan: ${library.leak ? 1 : 0} finding(s)`,
          },
          ...(lib.uncommitted > 0
            ? [
                {
                  op: "note" as const,
                  detail: `commit ${lib.uncommitted} changed file(s) as “Update skills library”`,
                },
              ]
            : []),
          ...commits.map((c) => ({
            op: "note" as const,
            detail: `commit ${c.sha} ${c.subject}`,
          })),
          { op: "exec", path: LIBRARY, detail: "git push (never forced)" },
        ],
        effects: {},
        warnings: library.leak
          ? [
              `the push would be refused: ${FINDING.rule} in ${FINDING.file}:${FINDING.line}`,
            ]
          : [],
        undo: undoPush,
      },
    };
  };
  on("library push --dry-run", pushPlan);
  on("library push", () => {
    if (library.leak)
      return new Failure(
        "refused",
        `secret scan blocked the push: ${FINDING.rule} in ${FINDING.file}:${FINDING.line} (nothing was pushed)`,
      );
    const none = nothing();
    lib.ahead = 0;
    lib.uncommitted = 0;
    return {
      dryRun: false,
      repo: LIBRARY,
      pushed: !none,
      checks: checks(),
      ...(none ? { message: "nothing to push" } : { commitSha: "9c1e4a7" }),
      result: { applied: true as const, backups: [], changed: [], undo: undoPush },
    } satisfies LibraryPushData;
  });
  return [...rows];
}
