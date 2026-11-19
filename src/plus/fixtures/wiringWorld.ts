import type { CtlReply } from "./ctlWorld";
import type {
  CheckpointData,
  LayerDeployPart,
  LayerRow,
  LayersData,
  Plan,
  PlanWriteData,
  PresyncData,
  RulesDeployPart,
  ShimsPart,
  OrgPullStatus,
  WiringCheck,
  WiringState,
  WiringStatusData,
} from "../context/data";

const HOME = "/Users/example";
const SHIMS = `${HOME}/Library/Application Support/Toolport/context-shims.zsh`;
const RC = `${HOME}/.zshrc`;
const LINE =
  '[ -r "$HOME/Library/Application Support/Toolport/context-shims.zsh" ] && source "$HOME/Library/Application Support/Toolport/context-shims.zsh"  # toolport:context-shims';
const INTERVAL = 14_400;

export interface WiringWorldOptions {
  /** Where the tab starts. `look` is the source order finding, `failed` a failed presync. */
  state?: WiringState;
  /** The org tool is installed (default) or not. */
  org?: boolean;
  /** The org pull command set in context.json; absent means the built-in pull. */
  orgPullCommand?: string[];
  /** The orgPull setting in context.json cannot be used; this is why. */
  orgPullError?: string;
  /** The layers of the skills repository; the default is the four of the design. */
  layers?: LayerRow[];
  /** No Claude session has run yet: the gauge has nothing to show. */
  noSession?: boolean;
  /** The checkpoint already set, as tokens before compaction. */
  checkpointAt?: number | null;
  /** `context presync` ends in a failure. */
  presyncFails?: boolean;
  /** The sync preview finds a hand-edited layer block that a confirmed run replaces. */
  edited?: boolean;
}

function orgPull(options: WiringWorldOptions): OrgPullStatus {
  const base = {
    configFile: `${HOME}/.config/mcpm/context.json`,
    key: "orgPull.command",
  };
  if (options.orgPullError) {
    return {
      ...base,
      source: "invalid",
      command: null,
      display: "not run until context.json is fixed",
      error: options.orgPullError,
    };
  }
  if (options.orgPullCommand) {
    return {
      ...base,
      source: "configured",
      command: options.orgPullCommand,
      display: options.orgPullCommand
        .map((a) => (/[\s"']/.test(a) ? JSON.stringify(a) : a))
        .join(" "),
      error: null,
    };
  }
  return {
    ...base,
    source: "default",
    command: null,
    display: "git pull, then the org tool's own sync (built in)",
    error: null,
  };
}

const tokensOf = (value: number) => ({ value, basis: "estimate" as const });

export function seedLayers(): LayerRow[] {
  return [
    {
      name: "personal",
      path: `${HOME}/.config/mcpm/skills_repo/rules/personal/SKILL.md`,
      scope: "global",
      delivery: "copy",
      globs: [],
      folderPatterns: [],
      imports: [],
      tokens: tokensOf(1860),
      problems: [],
      state: "deployed",
      detail: "",
      target: { kind: "rules", file: `${HOME}/.claude/rules/personal.md` },
    },
    {
      name: "review-habits",
      path: `${HOME}/.config/mcpm/skills_repo/rules/review-habits/SKILL.md`,
      scope: "global",
      delivery: "copy",
      globs: [],
      folderPatterns: [],
      imports: [],
      tokens: tokensOf(940),
      problems: [],
      state: "deployed",
      detail: "",
      target: { kind: "rules", file: `${HOME}/.claude/rules/review-habits.md` },
    },
    {
      name: "client-alpha",
      path: `${HOME}/.config/mcpm/skills_repo/rules/client-alpha/SKILL.md`,
      scope: "folder",
      delivery: "copy",
      globs: [],
      folderPatterns: ["**/clients/alpha/**"],
      imports: [],
      tokens: tokensOf(1120),
      problems: [],
      state: "deployed",
      detail: "",
      target: {
        kind: "folders",
        folders: [1, 2, 3, 4].map((n) => `${HOME}/work/clients/alpha/app-${n}`),
        deployed: 4,
        file: "CLAUDE.local.md",
      },
    },
    {
      name: "client-beta",
      path: `${HOME}/.config/mcpm/skills_repo/rules/client-beta/SKILL.md`,
      scope: "folder",
      delivery: "import",
      globs: [],
      folderPatterns: ["**/clients/beta/**"],
      imports: ["~/notes/beta.md"],
      tokens: tokensOf(860),
      problems: [],
      state: "edited",
      detail:
        "A block at the destination was changed by hand since the last deploy. A sync would replace it and keep a backup.",
      target: {
        kind: "folders",
        folders: [`${HOME}/work/clients/beta/api`, `${HOME}/work/clients/beta/web`],
        deployed: 0,
        file: "CLAUDE.local.md",
      },
    },
  ];
}

const plan = (
  summary: string,
  steps: Plan["steps"],
  undo: string,
  warnings: string[] = [],
): Plan => ({
  summary,
  steps,
  effects: {},
  warnings,
  undo,
});

/** `context wiring|layers|presync|checkpoint-*` of the in-memory toolportctl. */
export function createWiringWorld(options: WiringWorldOptions = {}) {
  let wired = (options.state ?? "healthy") !== "not_set_up";
  let wrongOrder = options.state === "look";
  let failed = options.state === "failed";
  let presync: { at: string; ok: boolean; lastOk: string | null; ms: number } | null =
    wired
      ? {
          at: "2026-11-19T07:54:00Z",
          ok: !failed,
          lastOk: failed ? "2026-11-18T20:02:00Z" : "2026-11-19T07:54:00Z",
          ms: 1800,
        }
      : null;
  let layers = options.layers ?? seedLayers();
  let checkpointAt = options.checkpointAt ?? null;
  let undoable = false;
  let pending: Array<Omit<LayerDeployPart["rows"][number], "applied">> = [
    {
      layer: "client-alpha",
      folder: `${HOME}/work/clients/alpha/app-5`,
      file: `${HOME}/work/clients/alpha/app-5/CLAUDE.local.md`,
      action: "add",
      diff: { before: "", after: "<!-- toolport:layer:begin client-alpha -->\n…" },
    },
    ...(options.edited === false
      ? []
      : [
          {
            layer: "client-beta",
            folder: `${HOME}/work/clients/beta/api`,
            file: `${HOME}/work/clients/beta/api/CLAUDE.local.md`,
            action: "replace" as const,
            reason: "changed by hand since the last deploy",
            diff: {
              before:
                "<!-- toolport:layer:begin client-beta -->\nUse the sandbox, mine.\n…",
              after: "<!-- toolport:layer:begin client-beta -->\nUse the sandbox.\n…",
            },
          },
        ]),
  ];
  const orgInstalled = options.org ?? true;

  function checks(): WiringCheck[] {
    const out: WiringCheck[] = [];
    if (wired) {
      out.push(
        wrongOrder
          ? {
              id: "source-order",
              level: "look",
              title: "context-shims sourced BEFORE shell-wrapper.sh in ~/.zshrc",
              detail:
                "The other wrapper wins, so layers are not refreshed before a launch. Move our source line below it; Toolport copies the file first.",
            }
          : {
              id: "source-order",
              level: "ok",
              title: "context-shims is sourced last",
              detail: "",
            },
        {
          id: "shims-file",
          level: "ok",
          title: "The shell file is written and current",
          detail: "",
        },
      );
    }
    out.push(
      orgInstalled
        ? {
            id: "org-tool",
            level: "ok",
            title: "The org tool is installed: no coexistence constraints are violated",
            detail: "",
          }
        : {
            id: "org-tool",
            level: "ok",
            title: "The org tool is not installed",
            detail:
              "No org file loads and no coexistence rules apply. Layers still deploy.",
          },
    );
    if (orgInstalled)
      out.push({
        id: "org-file",
        level: "ok",
        title: "The org file is read-only and unchanged by Toolport",
        detail: "",
      });
    if (failed)
      out.push({
        id: "presync",
        level: "look",
        title: "The last presync failed",
        detail:
          "the org tool could not be pulled; the files on disk are from its last good run",
      });
    return out;
  }

  function state(): WiringState {
    if (!wired) return "not_set_up";
    if (failed) return "failed";
    return checks().some((c) => c.level !== "ok") ? "look" : "healthy";
  }

  function status(): WiringStatusData {
    return {
      state: state(),
      home: HOME,
      setUp: wired,
      shims: {
        file: SHIMS,
        exists: wired,
        current: true,
        functions: [
          {
            name: "context_presync",
            summary:
              "Before a launch: pulls the org tool when it is due, then runs context sync. A failure only warns.",
          },
          {
            name: "claude",
            summary: "Presync, then starts Claude with the default settings folder.",
          },
          {
            name: "claude-bare",
            summary: "Presync, then starts Claude in launch profile bare.",
          },
          {
            name: "hrclaude",
            summary:
              "Only while a compression proxy is set up: presync, then Claude under that policy.",
          },
        ],
      },
      shell: {
        file: RC,
        exists: true,
        linkTarget: null,
        sourced: wired,
        addedByToolport: wired,
        line: wired ? (wrongOrder ? 3 : 9) : null,
        sourcedBefore: wrongOrder ? ["shell-wrapper.sh"] : [],
      },
      presync: presync
        ? {
            lastAttemptAt: presync.at,
            lastOkAt: presync.lastOk,
            durationMs: presync.ms,
            ok: presync.ok,
            warnings: presync.ok
              ? []
              : [
                  "the org tool could not be pulled; the files on disk are from its last good run",
                ],
            output: presync.ok
              ? ""
              : "fatal: unable to access 'https://example.test/corp-tools.git/'",
            nextAfter: presync.lastOk ? "2026-11-19T11:54:00Z" : null,
            due: false,
          }
        : null,
      intervalSecs: INTERVAL,
      org: {
        installed: orgInstalled,
        name: orgInstalled ? "corp-tools" : null,
        folder: `${HOME}/.local/share/corp-tools/claude`,
        file: `${HOME}/.claude/CLAUDE.md`,
        fileTokens: orgInstalled ? 10070 : null,
        tokenBasis: "estimate",
        intervalSecs: INTERVAL,
        pull: orgPull(options),
        lastSyncedAt: orgInstalled ? "2026-11-19T07:54:00Z" : null,
        matchesClone: orgInstalled ? true : null,
      },
      checks: checks(),
      shells: ["zsh"],
    };
  }

  function writeData(dryRun: boolean, p: Plan, changed: string[]): PlanWriteData {
    return {
      dryRun,
      plan: p,
      result: dryRun
        ? null
        : {
            applied: true,
            changed,
            undo: p.undo,
            backups: [
              `${HOME}/Library/Application Support/Toolport/plus/context-backups/shell/zshrc-3f2a9c1d7e04.bak`,
            ],
          },
    };
  }

  function init(dryRun: boolean): CtlReply {
    const steps: Plan["steps"] = [];
    if (!wired)
      steps.push({
        op: "create",
        path: SHIMS,
        detail:
          "The shell file Toolport owns: claude() runs the presync first, one function per launch profile",
        diff: {
          before: "",
          after: 'claude() { context_presync; command claude "$@"; }\n',
        },
      });
    if (!wired || wrongOrder) {
      steps.push({
        op: "update",
        path: RC,
        detail: wrongOrder
          ? "Moves our line to the end of the file, below shell-wrapper.sh; no other line changes"
          : "Adds one line at the end of the file, marked toolport:context-shims; no other line changes",
        diff: { before: wrongOrder ? LINE : "", after: LINE },
      });
      steps.push({
        op: "note",
        path: `${HOME}/Library/Application Support/Toolport/plus/context-backups/shell`,
        detail: "A copy of the file is saved here first",
      });
    }
    const p = plan(
      steps.length === 0
        ? "The launch wiring is already set up"
        : wrongOrder
          ? "Fix the source order of the launch wiring"
          : "Set up the launch wiring",
      steps,
      "toolportctl context wiring disable",
    );
    if (!dryRun && steps.length > 0) {
      wired = true;
      wrongOrder = false;
      undoable = true;
    }
    return { ok: true, data: writeData(dryRun, p, [SHIMS, RC]) };
  }

  function disable(dryRun: boolean): CtlReply {
    const steps: Plan["steps"] = wired
      ? [
          {
            op: "update",
            path: RC,
            detail:
              "Removes the line marked toolport:context-shims; no other line changes",
            diff: { before: LINE, after: "" },
          },
          {
            op: "delete",
            path: SHIMS,
            detail: "The shell file Toolport owns; it is generated again by init",
          },
        ]
      : [];
    const p = plan(
      steps.length === 0
        ? "The launch wiring is not set up"
        : "Turn off the launch wiring",
      steps,
      "toolportctl context wiring init",
    );
    if (!dryRun && steps.length > 0) wired = false;
    return { ok: true, data: writeData(dryRun, p, [RC, SHIMS]) };
  }

  function layerParts(
    dryRun: boolean,
    rules: boolean,
  ): {
    layers: LayerDeployPart;
    rules: RulesDeployPart | null;
    shims: ShimsPart;
  } {
    const rows: LayerDeployPart["rows"] = pending.map((row) => ({
      ...row,
      applied: !dryRun,
    }));
    if (!dryRun && rows.length > 0) {
      pending = [];
      layers = layers.map((l) => ({ ...l, state: "deployed" as const, detail: "" }));
      undoable = true;
    }
    return {
      layers: {
        rows,
        upToDate: 3,
        files: rows.map((r) => r.file),
        changed: dryRun ? [] : rows.map((r) => r.file),
        backups:
          dryRun || !rows.some((r) => r.action === "replace")
            ? []
            : [
                `${HOME}/Library/Application Support/Toolport/plus/context-backups/layers/1-0-client-beta.md`,
              ],
        undo: !dryRun && rows.length > 0 ? "toolportctl context layers undo" : null,
      },
      rules: rules ? { rows: [], held: [], upToDate: 2 } : null,
      shims: wired
        ? { action: "unchanged", file: SHIMS }
        : { action: "skipped", file: SHIMS, reason: "the launch wiring is not set up" },
    };
  }

  function presyncRun(): CtlReply {
    if (options.presyncFails) {
      failed = true;
      presync = {
        at: "2026-11-19T10:00:00Z",
        ok: false,
        lastOk: presync?.lastOk ?? null,
        ms: 900,
      };
      return {
        ok: false,
        error: {
          code: "presync_failed",
          message:
            "the org tool could not be pulled; the files on disk are from its last good run",
        },
      };
    }
    failed = false;
    presync = {
      at: "2026-11-19T10:00:00Z",
      ok: true,
      lastOk: "2026-11-19T10:00:00Z",
      ms: 1700,
    };
    const data: PresyncData = {
      ran: true,
      ok: true,
      orgPulled: false,
      durationMs: 1700,
      warnings: [],
      nextAfter: 1_795_000_000,
    };
    return { ok: true, data };
  }

  function checkpoint(): CtlReply {
    if (options.noSession) {
      const data: CheckpointData = {
        session: null,
        checkpointAt,
        note: "No Claude session with usage was found yet.",
      };
      return { ok: true, data };
    }
    const window = 200_000;
    const point = checkpointAt === null ? null : window - checkpointAt;
    const used = 142_000;
    const data: CheckpointData = {
      session: {
        id: "s1",
        cwd: `${HOME}/work/clients/alpha/app-1`,
        model: "claude-opus-4-8",
        lastActivityAt: "2026-11-19T09:40:00Z",
        stale: false,
      },
      usedTokens: used,
      usedBasis:
        "the input, cache read and cache creation tokens of the session's last request",
      window: { tokens: window, source: "model name" },
      compactionAt: window,
      checkpointAt,
      checkpoint:
        point === null
          ? null
          : { point, reached: used >= point, remaining: Math.max(0, point - used) },
      remainingToCompaction: window - used,
    };
    return { ok: true, data };
  }

  const fail = (code: string, message: string): CtlReply => ({
    ok: false,
    error: { code, message },
  });

  return {
    get wired() {
      return wired;
    },
    get checkpointAt() {
      return checkpointAt;
    },
    syncParts: layerParts,
    run(argv: string[]): CtlReply {
      const [, group, sub] = argv;
      const dryRun = argv.includes("--dry-run");
      if (group === "wiring") {
        if (sub === "status") return { ok: true, data: status() };
        if (sub === "init") return init(dryRun);
        if (sub === "disable") return disable(dryRun);
      }
      if (group === "layers") {
        if (sub === "ls") {
          const data: LayersData = {
            layers,
            library: `${HOME}/.config/mcpm/skills_repo/rules`,
          };
          return { ok: true, data };
        }
        if (sub === "undo") {
          if (!dryRun && undoable) undoable = false;
          return {
            ok: true,
            data: { dryRun, rows: [], changed: [] },
          };
        }
      }
      if (group === "presync") return presyncRun();
      if (group === "checkpoint-status") return checkpoint();
      if (group === "checkpoint-config") {
        const at = argv.indexOf("--at");
        const value = at >= 0 ? Number(argv[at + 1]) : null;
        if (value !== null && !(value >= 1 && value < 1_000_000))
          return fail(
            "usage",
            "--at must be between 1 and 999999 tokens before compaction",
          );
        const p = plan(
          value === null
            ? "Checkpoint: off"
            : `Checkpoint: ${value} tokens before compaction`,
          [
            {
              op: "update",
              path: `${HOME}/.config/mcpm/context.json`,
              detail: "Sets checkpointAt in context.json; every other key stays",
              keys: ["checkpointAt"],
              diff: {
                before:
                  checkpointAt === null
                    ? "off"
                    : `${checkpointAt} tokens before compaction`,
                after: value === null ? "off" : `${value} tokens before compaction`,
              },
            },
          ],
          checkpointAt === null
            ? "toolportctl context checkpoint-config --off"
            : `toolportctl context checkpoint-config --at ${checkpointAt}`,
        );
        if (!dryRun) checkpointAt = value;
        return {
          ok: true,
          data: writeData(dryRun, p, [`${HOME}/.config/mcpm/context.json`]),
        };
      }
      throw new Error(`Unimplemented fixture command: plus_ctl ${argv.join(" ")}`);
    },
  };
}

export type WiringWorld = ReturnType<typeof createWiringWorld>;
