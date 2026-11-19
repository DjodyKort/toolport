import type { CtlReply } from "./ctlWorld";

const HELD = [
  {
    file: "~/.claude/CLAUDE.md",
    kind: "claude-user",
    reason: "a GitHub token",
  },
];

const PARTS = [
  {
    id: "repos",
    label: "Repositories",
    count: 3,
    fileSize: 1400,
    notes: ["1 without a remote cannot be cloned again"],
  },
  { id: "servers", label: "Servers", count: 2, fileSize: 900, notes: [] },
  { id: "library", label: "Skills library", count: 4, fileSize: 300, notes: [] },
  { id: "settings", label: "Settings", count: 1, fileSize: 500, notes: [] },
  {
    id: "claudeUser",
    label: "Claude Code user files",
    count: 3,
    fileSize: 1500,
    notes: [],
    heldBack: HELD,
  },
  { id: "memory", label: "Memory", count: 2, fileSize: 800, notes: [] },
  {
    id: "history",
    label: "Sessions",
    count: 3,
    fileSize: 4096,
    notes: [],
    sealed: true,
    info: {
      repos: [
        { name: "web", path: "~/code/web", sessions: 2, fileSize: 3000 },
        { name: "api", path: "~/code/api", sessions: 1, fileSize: 1096 },
      ],
    },
  },
  { id: "secrets", label: "Secrets", count: 2, fileSize: 0, notes: [], sealed: true },
];

const REPOS = [
  {
    name: "web",
    path: "~/code/web",
    remote: "https://example.invalid/web.git",
    branch: "main",
    upstream: "origin/main",
    dirty: 1,
    untracked: 0,
    stashes: 0,
    unpushed: 1,
    neverPushed: [],
    noRemote: false,
    worktrees: 1,
    error: null,
  },
  {
    name: "scratch",
    path: "~/code/scratch",
    remote: null,
    branch: "main",
    upstream: null,
    dirty: 0,
    untracked: 0,
    stashes: 0,
    unpushed: 0,
    neverPushed: [],
    noRemote: true,
    worktrees: 0,
    error: null,
  },
];

const rowsFor = (restored: boolean, take: string[]) => [
  {
    id: "repo:~/code/web",
    label: "web",
    part: "repos",
    state: restored ? "here" : "add",
    note: "",
    takeable: false,
  },
  {
    id: "claude:CLAUDE.md",
    label: "~/.claude/CLAUDE.md",
    part: "claudeUser",
    state: restored || take.includes("claude:CLAUDE.md") ? "here" : "conflict",
    note: "differs: yours is kept",
    takeable: true,
  },
  {
    id: "server:github",
    label: "github",
    part: "servers",
    state: "needs",
    note: "needs a secret",
    takeable: false,
  },
];

const INSPECT = {
  file: "/tmp/setup.zip",
  format: "toolport-setup",
  version: 1,
  madeAt: "2026-11-08T09:14:00Z",
  machine: "Studio Mac",
  toolport: "2.3.7",
  parts: [
    { id: "repos", label: "Repositories", count: 3, fileSize: 1400, sealed: false },
    { id: "servers", label: "Servers", count: 2, fileSize: 900, sealed: false },
    {
      id: "claudeUser",
      label: "Claude Code user files",
      count: 3,
      fileSize: 1500,
      sealed: false,
    },
    { id: "history", label: "Sessions", count: 3, fileSize: 4096, sealed: true },
    { id: "secrets", label: "Secrets", count: 2, fileSize: 800, sealed: true },
  ].map((p) => ({ ...p, standing: "ok" })),
  problems: [],
  intact: true,
};

/** Switches for a test to reach the states a screen has to show. */
export interface SetupWorldOptions {
  /** The first apply leaves one step failed, and the next undo cannot finish one step. */
  failures?: boolean;
  /** The file has no encrypted part. */
  plain?: boolean;
  /** Nothing to find: no repositories, no sessions, nothing in the file, no export yet. */
  empty?: boolean;
  /** Commands that fail with a message: parts, export, inspect, restore, apply, undo, check. */
  broken?: string[];
}

/** The setup commands against a fixed file and one reset checklist. */
export function createSetupWorld(options: SetupWorldOptions = {}) {
  const carry = ["browser", "keys", "licenses", "outside"].map((id) => ({
    id,
    label: id,
    ticked: false,
  }));
  const acked = new Set<string>();
  const finding = (id: string, kind: string, note: string, commands: string[] = []) => ({
    id,
    kind,
    repo: id.split(":")[1],
    note,
    commands,
    acked: acked.has(id),
  });
  let restored = false;
  let undoFailing = options.failures === true;
  let lastExport: unknown = null;

  return {
    run(key: string[], stdin?: string): CtlReply {
      const [, verb] = key;
      const step = verb === "restore" && key.includes("--apply") ? "apply" : verb;
      if (options.broken?.includes(step)) {
        return {
          ok: false,
          error: { code: "failed", message: `${step} failed: the disk is read-only` },
        };
      }
      const flag = (name: string) =>
        key.includes(name) ? key[key.indexOf(name) + 1] : undefined;
      const all = (name: string) =>
        key.flatMap((arg, i) => (arg === name ? [key[i + 1]] : []));
      if (verb === "parts") {
        return {
          ok: true,
          data: {
            encryption: true,
            minPassphrase: 12,
            lastExport,
            roots: options.empty ? [] : ["~/code"],
            repos: options.empty ? [] : REPOS,
            parts: PARTS.map((p) => ({
              info: {},
              ...p,
              ...(options.empty
                ? { count: 0, fileSize: 0, notes: [], heldBack: [], info: {} }
                : {}),
              default: !p.sealed,
              sealed: p.sealed === true,
              error: null,
            })),
          },
        };
      }
      if (verb === "export") {
        const dryRun = key.includes("--dry-run");
        const ids = (flag("--parts") ?? "").split(",");
        const sealed = ids.some((id) => PARTS.some((p) => p.id === id && p.sealed));
        if (key.includes("--peek")) {
          return {
            ok: true,
            data: {
              dryRun: true,
              peek: {
                part: flag("--peek"),
                label: "Repositories",
                files: [{ name: "repos.json", json: { version: 1, repos: [] } }],
              },
            },
          };
        }
        if ((key.includes("--secrets") || sealed) && !dryRun && !stdin) {
          return {
            ok: false,
            error: { code: "usage", message: "the passphrase comes on stdin" },
          };
        }
        const secretsOn = key.includes("--secrets");
        const picked = PARTS.filter((p) => ids.includes(p.id));
        const data = {
          dryRun,
          file: `${flag("--out")}/toolport-setup.zip`,
          name: "toolport-setup.zip",
          fileSize: dryRun ? 0 : 3674,
          totalSize: 4096,
          verified: !dryRun,
          heldBackSealed: secretsOn,
          parts: picked.map((p) => ({
            id: p.id,
            label: p.label,
            count: p.count,
            heldBack: p.heldBack ?? [],
          })),
          tree: [
            { name: "manifest.json", fileSize: 0 },
            ...picked.map((p) => ({
              name: p.id,
              fileSize: p.fileSize,
              sealed: p.sealed === true,
            })),
          ],
          notIncluded: ["your code and anything uncommitted, stashed or unpushed"],
        };
        if (!dryRun) {
          lastExport = {
            file: data.file,
            madeAt: "2026-11-17T10:00:00Z",
            bytes: 3674,
            parts: Object.fromEntries(picked.map((p) => [p.id, { count: p.count }])),
          };
        }
        return { ok: true, data };
      }
      if (verb === "inspect") {
        return {
          ok: true,
          data: {
            ...INSPECT,
            parts: INSPECT.parts.filter((p) => !(options.plain && p.sealed)),
          },
        };
      }
      if (verb === "restore") {
        const apply = key.includes("--apply");
        const take = all("--take");
        const sealedGiven = key.includes("--passphrase-stdin");
        if (sealedGiven && stdin !== "correct passphrase") {
          return {
            ok: false,
            error: { code: "passphrase", message: "that passphrase does not open it" },
          };
        }
        const planHash = `fixture-plan${take.length}`;
        if (apply && flag("--plan") !== planHash) {
          return {
            ok: false,
            error: { code: "stale_plan", message: "the file changed since the plan" },
          };
        }
        const wanted = flag("--parts")?.split(",");
        const rows = (options.empty ? [] : rowsFor(restored, take)).filter(
          (r) => !wanted || wanted.includes(r.part),
        );
        if (apply) {
          restored = true;
          const steps = rows
            .filter((r) => r.state === "add" || r.id === "claude:CLAUDE.md")
            .map((r) => ({
              id: r.id,
              part: r.part,
              label: r.label,
              ok: !(options.failures && r.part === "repos"),
              note:
                options.failures && r.part === "repos"
                  ? "authentication required"
                  : "done",
            }));
          return {
            ok: true,
            data: {
              applied: true,
              dryRun: false,
              plan: planHash,
              rows,
              summary: { add: 0, here: 0, conflict: 0, needs: 1, skip: 0 },
              run: "run-1",
              steps,
              failed: steps.filter((s) => !s.ok).length,
              needsYou: [
                { id: "server:github", label: "github", note: "needs a secret" },
              ],
            },
          };
        }
        const count = (state: string) => rows.filter((r) => r.state === state).length;
        return {
          ok: true,
          data: {
            applied: false,
            dryRun: true,
            plan: planHash,
            rows,
            summary: {
              add: count("add"),
              here: count("here"),
              conflict: count("conflict"),
              needs: count("needs"),
              skip: count("skip"),
            },
          },
        };
      }
      if (verb === "undo") {
        if (!restored)
          return { ok: false, error: { code: "nothing", message: "nothing to undo" } };
        const dryRun = key.includes("--dry-run");
        const failing = undoFailing;
        const steps = [
          { what: "~/code/web", ok: true, result: "done", note: "clone removed" },
          {
            what: "~/.claude/CLAUDE.md",
            ok: false,
            result: "kept",
            note: "kept: changed since the restore",
          },
          ...(failing
            ? [
                {
                  what: "~/.claude/rules/a.md",
                  ok: false,
                  result: "failed",
                  note: "it could not be removed: read-only",
                },
              ]
            : []),
        ];
        const undone = !dryRun && !failing;
        if (!dryRun) {
          if (undone) restored = false;
          undoFailing = false;
        }
        return {
          ok: true,
          data: {
            run: "run-1",
            dryRun,
            undone,
            failed: failing && !dryRun ? ["~/.claude/rules/a.md"] : [],
            steps: dryRun ? steps.filter((s) => s.result !== "failed") : steps,
            left: {
              servers: [],
              secretValues: 0,
              note: "servers and stored values are removed in Servers",
            },
          },
        };
      }
      if (verb === "check") {
        for (const [flagName, on] of [
          ["--tick", true],
          ["--untick", false],
        ] as const) {
          const id = flag(flagName);
          const item = carry.find((c) => c.id === id);
          if (key.includes(flagName) && item) item.ticked = on;
        }
        if (key.includes("--ack")) acked.add(flag("--ack")!);
        const blockers = options.empty
          ? []
          : [
              finding("unpushed:~/code/web", "unpushed", "1 commits are on no remote", [
                "git -C ~/code/web push origin main",
              ]),
            ];
        const amber = options.empty
          ? []
          : [
              finding("stash:~/code/web", "stash", "1 stashes", [
                "git -C ~/code/web stash list",
              ]),
            ];
        const open = blockers.filter((b) => !b.acked).length;
        return {
          ok: true,
          data: {
            repos: options.empty ? 0 : 2,
            blockers,
            amber,
            carry,
            lastExport,
            since: key.includes("--since")
              ? {
                  parts: [
                    {
                      id: "repos",
                      label: "Repositories",
                      added: 0,
                      removed: 0,
                      changed: 1,
                      countWas: 3,
                      countNow: 3,
                      branchChanges: 1,
                    },
                  ],
                }
              : null,
            sessions: options.empty
              ? { count: 0, fileSize: 0, repos: [] }
              : {
                  count: 3,
                  fileSize: 4096,
                  repos: PARTS.find((p) => p.id === "history")!.info!.repos,
                },
            open: { blockers: open, carry: carry.filter((c) => !c.ticked).length },
            ready: open === 0 && carry.every((c) => c.ticked) && lastExport !== null,
          },
        };
      }
      return {
        ok: false,
        error: { code: "usage", message: `unknown setup verb ${verb}` },
      };
    },
  };
}
