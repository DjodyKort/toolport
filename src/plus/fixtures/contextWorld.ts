import type { CtlReply } from "./ctlWorld";
import type {
  BundleRow,
  BundleShowData,
  LoadItem,
  MeasureData,
  Plan,
  PlanStep,
  SyncData,
  WriteData,
} from "../context/data";

export const FIXTURE_HOME = "/Users/example";
export const FIXTURE_FOLDER = `${FIXTURE_HOME}/work/clients/alpha/web-app`;
const VERSION = "2.1.295";
const SETTINGS = ".claude/settings.local.json";
const PROFILES = `${FIXTURE_HOME}/.config/mcpm/skills_repo/profiles`;

export interface ContextWorldOptions {
  /** `fresh`: the folder was measured and nothing changed since. `stale`: a Claude Code update. */
  measured?: "none" | "fresh" | "stale";
  /** How `context measure --yes` ends: `slow` takes a while, `fail` answers with an error. */
  measure?: "ok" | "slow" | "fail";
  bundles?: boolean;
  /** Launch profile files and direct entries `context sync` finds out of date. */
  regenerate?: boolean;
}

interface FileRow {
  name: string;
  kind: string;
  source: string;
  path: (cwd: string) => string;
  tokens: number;
  lazy?: boolean;
  reason?: string;
}

const dirname = (path: string) => path.slice(0, path.lastIndexOf("/"));

const FILES: FileRow[] = [
  {
    name: "CLAUDE.md",
    kind: "memory",
    source: "policy",
    path: () => "/Library/Application Support/ClaudeCode/CLAUDE.md",
    tokens: 10070,
  },
  {
    name: "personal",
    kind: "rule",
    source: "user",
    path: () => `${FIXTURE_HOME}/.claude/rules/personal.md`,
    tokens: 1860,
  },
  {
    name: "review-habits",
    kind: "rule",
    source: "user",
    path: () => `${FIXTURE_HOME}/.claude/rules/review-habits.md`,
    tokens: 940,
  },
  {
    name: "CLAUDE.md",
    kind: "memory",
    source: "project",
    path: (cwd) => `${dirname(cwd)}/CLAUDE.md`,
    tokens: 640,
  },
  {
    name: "CLAUDE.md",
    kind: "memory",
    source: "project",
    path: (cwd) => `${cwd}/CLAUDE.md`,
    tokens: 3410,
  },
  {
    name: "CLAUDE.local.md",
    kind: "memory",
    source: "local",
    path: (cwd) => `${cwd}/CLAUDE.local.md`,
    tokens: 1120,
  },
  {
    name: "MEMORY.md",
    kind: "memory-index",
    source: "user",
    path: () => `${FIXTURE_HOME}/.claude/projects/web-app/memory/MEMORY.md`,
    tokens: 1380,
  },
  {
    name: "api",
    kind: "rule",
    source: "project",
    path: (cwd) => `${cwd}/.claude/rules/api.md`,
    tokens: 780,
    lazy: true,
    reason: "rule scoped to **/api/**",
  },
  {
    name: "CLAUDE.md",
    kind: "memory",
    source: "project",
    path: (cwd) => `${cwd}/services/billing/CLAUDE.md`,
    tokens: 1240,
    lazy: true,
    reason: "loads when Claude reads files in services/billing",
  },
];

interface Measurement {
  total: number;
  measuredAt: string;
  version: string;
  estimated: number;
  dirty: boolean;
}

interface FolderState {
  leftOut: string[];
  memoryOff: boolean;
  pluginsOff: string[];
  bundle: string | null;
  measured: Measurement | null;
}

const PLUGINS = ["suite-a@market", "team-tools@internal", "loop-runner@market"];

interface BundleParts {
  layers?: string[];
  plugins?: string[];
  skills?: string[];
  agents?: string[];
  bind?: string[];
  memoryOff?: boolean;
  servers?: string;
}

type WorldBundle = BundleShowData & { appliedTo: string[] };

function makeBundle(name: string, description: string, parts: BundleParts): WorldBundle {
  return {
    name,
    path: `${PROFILES}/${name}.yaml`,
    description,
    servers: parts.servers ?? null,
    skills: { off: parts.skills ?? [], nameOnly: [], allow: [] },
    plugins: { off: parts.plugins ?? [] },
    mcp: { deny: [] },
    layers: { add: [], exclude: parts.layers ?? [] },
    dirs: { add: [] },
    agents: { off: parts.agents ?? [] },
    bind: parts.bind ?? [],
    yaml: `format: 1\nname: ${name}\n`,
    issues: [],
    appliedTo: [],
    ...(parts.memoryOff ? { memory: { off: true } } : {}),
  };
}

function seedBundles(): WorldBundle[] {
  return [
    makeBundle("Stage", "No global files, no heavy plugins, no auto memory", {
      layers: [`${FIXTURE_HOME}/.claude/CLAUDE.md`],
      plugins: ["suite-a@market", "loop-runner@market"],
      memoryOff: true,
      servers: "Default",
    }),
    makeBundle("default", "Only two skills, everything else as is", {
      skills: ["scratch-*"],
    }),
    makeBundle("erp-dev", "ERP work: fewer plugins, one agent off", {
      plugins: ["team-tools@internal"],
      agents: ["reviewer-bot"],
      bind: ["~/work/clients/erp/*"],
      servers: "Default",
    }),
  ];
}

function plan(
  summary: string,
  steps: PlanStep[],
  undo: string,
  warnings: string[] = [],
  effects: Plan["effects"] = {},
): Plan {
  return { summary, steps, effects, warnings, undo };
}

const NEXT_SESSION =
  "Open sessions keep what they loaded; the change applies from the next session";

const flag = (argv: string[], name: string) => {
  const at = argv.indexOf(name);
  return at >= 0 ? (argv[at + 1] ?? null) : null;
};

const folderOf = (argv: string[]) => flag(argv, "--cwd") ?? FIXTURE_FOLDER;

const operand = (argv: string[], from: number) =>
  argv.slice(from).find((a, i, all) => !a.startsWith("--") && all[i - 1] !== "--cwd") ??
  null;

/** A small in-memory toolportctl for the `context` and `plugins` commands: enough state for a
 * leave-out, a plugin switch, a bundle and a measurement to show on the next read. */
export function createContextWorld(
  options: ContextWorldOptions = {},
  syncParts?: (
    dryRun: boolean,
    rules: boolean,
  ) => Pick<SyncData, "layers" | "rules" | "shims">,
) {
  const folders = new Map<string, FolderState>();
  const bundles = options.bundles === false ? [] : seedBundles();
  let autoApply = false;

  const sum = (items: LoadItem[]) =>
    items.filter((i) => i.loaded && !i.lazy).reduce((total, i) => total + i.tokens, 0);

  function rowsOf(
    cwd: string,
    state: Pick<FolderState, "leftOut" | "memoryOff">,
    mark = true,
  ): LoadItem[] {
    return FILES.map((file) => {
      const path = file.path(cwd);
      const left =
        mark &&
        (file.kind === "memory-index" ? state.memoryOff : state.leftOut.includes(path));
      return {
        kind: file.kind,
        name: file.name,
        path,
        source: file.source,
        loaded: !left && !file.lazy,
        lazy: file.lazy === true,
        reason: left ? "excluded here" : (file.reason ?? ""),
        tokens: file.tokens,
        leftOutBy: left ? "toolport" : null,
      } satisfies LoadItem;
    });
  }

  function stateOf(cwd: string): FolderState {
    let state = folders.get(cwd);
    if (!state) {
      const mode = cwd === FIXTURE_FOLDER ? (options.measured ?? "none") : "none";
      state = {
        leftOut: [],
        memoryOff: false,
        pluginsOff: [],
        bundle: null,
        measured:
          mode === "none"
            ? null
            : {
                total: 37470,
                measuredAt: "2026-11-16T10:42:00.000Z",
                version: mode === "stale" ? "2.1.290" : VERSION,
                estimated: sum(rowsOf(cwd, { leftOut: [], memoryOff: false }, false)),
                dirty: false,
              },
      };
      folders.set(cwd, state);
    }
    return state;
  }

  const isStale = (m: Measurement) => m.dirty || m.version !== VERSION;

  function summary(state: FolderState) {
    const m = state.measured;
    if (!m) return null;
    return {
      total: m.total,
      windowPercent: Math.round((m.total / 200000) * 1000) / 10,
      model: "haiku",
      claudeCodeVersion: m.version,
      measuredAt: m.measuredAt,
      stale: isStale(m),
    };
  }

  function calibration(cwd: string, state: FolderState) {
    const m = state.measured;
    if (!m) return null;
    const factor = Math.round((m.total / m.estimated) * 1000) / 1000;
    return {
      factor,
      measured: m.total,
      estimated: m.estimated,
      measuredAt: m.measuredAt,
      stale: isStale(m),
      calibratedTotal: Math.round(sum(rowsOf(cwd, state)) * factor),
      meaning:
        "the last measured total divided by the estimate of the instruction rows at that time",
    };
  }

  function verified(cwd: string, state: FolderState) {
    const m = state.measured;
    if (!m) return null;
    const rows = rowsOf(cwd, state).filter((r) => !r.lazy);
    return {
      status: "ok",
      checked: rows.length,
      loaded: rows.filter((r) => r.loaded).length,
      absent: rows.filter((r) => !r.loaded).length,
      mismatches: [],
      unknown: [],
      measuredAt: m.measuredAt,
      claudeCodeVersion: m.version,
      stale: m.dirty,
    };
  }

  function write(
    cwd: string,
    dryRun: boolean,
    p: Plan,
    apply: () => void,
    extra: Partial<WriteData> = {},
  ): CtlReply {
    if (!dryRun) {
      apply();
      const m = stateOf(cwd).measured;
      if (m) m.dirty = true;
    }
    const data: WriteData = {
      cwd,
      dryRun,
      plan: p,
      result: dryRun
        ? null
        : { applied: true, changed: [`${cwd}/${SETTINGS}`], undo: p.undo, backups: [] },
      ...extra,
    };
    return { ok: true, data };
  }

  function leaveOut(argv: string[]): CtlReply {
    const cwd = folderOf(argv);
    const state = stateOf(cwd);
    const memory = argv.includes("--memory");
    const target = operand(argv, 3);
    if (!memory && !target) {
      return {
        ok: false,
        error: {
          code: "usage",
          message: "usage: context leave-out add|rm <path> --cwd <dir>",
        },
      };
    }
    const add = argv[2] === "add";
    return write(
      cwd,
      argv.includes("--dry-run"),
      plan(
        memory
          ? `Turn auto memory ${add ? "off" : "on"} in ${cwd}`
          : `${add ? "Leave out" : "Put back"} ${target} in ${cwd}`,
        [
          {
            op: "update",
            path: `${cwd}/${SETTINGS}`,
            detail: memory ? "autoMemoryEnabled" : "claudeMdExcludes",
          },
        ],
        `toolportctl context leave-out ${add ? "rm" : "add"} ${memory ? "--memory" : target} --cwd ${cwd}`,
        [NEXT_SESSION],
      ),
      () => {
        if (memory) state.memoryOff = add;
        else if (add) state.leftOut = [...new Set([...state.leftOut, target!])];
        else state.leftOut = state.leftOut.filter((p) => p !== target);
      },
      {
        id: "leave-out",
        scope: "folder",
        root: cwd,
        leftOut: state.leftOut,
        memoryOff: state.memoryOff,
      },
    );
  }

  function plugins(argv: string[]): CtlReply {
    const cwd = folderOf(argv);
    const state = stateOf(cwd);
    const id = argv[2];
    const off = argv[1] === "off";
    if (!id || !id.includes("@")) {
      return {
        ok: false,
        error: {
          code: "not_found",
          message: `no plugin called ${id ?? ""}: use name@marketplace`,
        },
      };
    }
    return write(
      cwd,
      argv.includes("--dry-run"),
      plan(
        `Turn ${off ? "off" : "on"} ${id} in ${cwd}`,
        [{ op: "update", path: `${cwd}/${SETTINGS}`, detail: `enabledPlugins.${id}` }],
        `toolportctl plugins ${off ? "on" : "off"} ${id} --cwd ${cwd}`,
        [NEXT_SESSION],
      ),
      () => {
        state.pluginsOff = off
          ? [...new Set([...state.pluginsOff, id])]
          : state.pluginsOff.filter((p) => p !== id);
      },
    );
  }

  function bundleRow(b: WorldBundle): BundleRow {
    return {
      name: b.name,
      description: b.description,
      servers: b.servers,
      skills: { off: b.skills.off.length, nameOnly: 0, allow: 0 },
      plugins: { off: b.plugins.off },
      layers: b.layers,
      agents: b.agents,
      bind: b.bind,
      appliedTo: b.appliedTo,
      path: b.path,
      issues: 0,
      ...(b.memory ? { memory: b.memory } : {}),
    };
  }

  function switchBundle(argv: string[], apply: boolean, name: string | null): CtlReply {
    const cwd = folderOf(argv);
    const state = stateOf(cwd);
    const picked = apply
      ? bundles.find((b) => b.name === name)
      : bundles.find((b) => b.name === state.bundle);
    if (!picked) {
      return apply
        ? {
            ok: false,
            error: {
              code: "not_found",
              message: `no bundle called ${name} in the skills repo's profiles/`,
            },
          }
        : {
            ok: false,
            error: { code: "not_applied", message: `no bundle is applied in ${cwd}` },
          };
    }
    const keys = [
      ...picked.layers.exclude.map(() => "claudeMdExcludes"),
      ...picked.plugins.off.map(() => "enabledPlugins"),
    ];
    const lines = [
      ...picked.layers.exclude.map((l) => `claudeMdExcludes: "${l}"`),
      ...picked.plugins.off.map((x) => `enabledPlugins.${x}: false`),
    ];
    const before = sum(rowsOf(cwd, state));
    const p = plan(
      apply ? `Use ${picked.name} in ${cwd}` : `Stop using ${picked.name} in ${cwd}`,
      [
        {
          op: apply ? "create" : "delete",
          path: `${cwd}/${SETTINGS}`,
          detail: `${keys.length} key(s) owned by bundle ${picked.name}`,
          keys: [...new Set(keys)],
          diff: { before: "", after: `${lines.join("\n")}\n` },
        },
      ],
      apply
        ? `toolportctl context use --none --cwd ${cwd}`
        : `toolportctl context bundle apply ${picked.name} --cwd ${cwd}`,
      [NEXT_SESSION],
      { tokens: { before, after: before - 10070, basis: "estimate" } },
    );
    return write(
      cwd,
      argv.includes("--dry-run"),
      p,
      () => {
        if (apply) {
          state.bundle = picked.name;
          picked.appliedTo = [...new Set([...picked.appliedTo, cwd])];
          state.leftOut = [...new Set([...state.leftOut, ...picked.layers.exclude])];
          state.pluginsOff = [...new Set([...state.pluginsOff, ...picked.plugins.off])];
          state.memoryOff = state.memoryOff || picked.memory?.off === true;
        } else {
          state.bundle = null;
          picked.appliedTo = picked.appliedTo.filter((f) => f !== cwd);
          state.leftOut = state.leftOut.filter((f) => !picked.layers.exclude.includes(f));
          state.pluginsOff = state.pluginsOff.filter(
            (x) => !picked.plugins.off.includes(x),
          );
        }
      },
      { bundle: picked.name },
    );
  }

  function measure(argv: string[]): CtlReply {
    const cwd = folderOf(argv);
    const state = stateOf(cwd);
    const start = argv.includes("--yes");
    const withouts = argv.flatMap((a, i) => (a === "--without" ? [argv[i + 1]] : []));
    const bundleName = flag(argv, "--bundle");
    const variants = [...withouts, ...(bundleName ? [`bundle:${bundleName}`] : [])];
    const m = state.measured;
    const fresh = m !== null && !isStale(m) && !argv.includes("--force");
    const requests = (fresh ? 0 : 1) + variants.length;
    if (!start && requests > 0) {
      const data: MeasureData = {
        cwd,
        ran: false,
        plan: {
          cost: "about as many input tokens as the folder's stack, on the chosen model, once per request",
          model: "haiku",
          requests,
          sends: "the instruction files go to the model like in any session",
          variants: ["as is", ...variants.map((v) => `without ${v}`)],
          writes: "nothing in the folder; the answers are cached",
        },
        start: "run again with --yes to start the requests",
      };
      return { ok: true, data };
    }
    if (start && options.measure === "fail") {
      return {
        ok: false,
        error: {
          code: "failed",
          message: "Claude Code timed out after 3:00. The last good number stays.",
        },
      };
    }
    const estimated = sum(rowsOf(cwd, state));
    if (start && requests > 0 && !fresh) {
      state.measured = {
        total: estimated + 18050,
        measuredAt: new Date().toISOString(),
        version: VERSION,
        estimated,
        dirty: false,
      };
    }
    const rec = state.measured!;
    const rest = Math.max(0, rec.total - estimated);
    const data: MeasureData = {
      cwd,
      ran: true,
      claudeCodeVersion: rec.version,
      model: "haiku",
      measuredAt: rec.measuredAt,
      cached: requests === 0,
      stale: isStale(rec),
      basis: "measurement",
      total: rec.total,
      contextWindow: 200000,
      windowPercent: Math.round((rec.total / 200000) * 1000) / 10,
      plugins: PLUGINS,
      mcpServers: ["toolport", "design-hub"],
      itemised: {
        tokens: estimated,
        basis: "estimate",
        rows: rowsOf(cwd, state).filter((r) => r.loaded).length,
      },
      notItemised: {
        tokens: rest,
        percent: Math.round((rest / rec.total) * 100),
        includes:
          "Claude Code's own prompt and built-in tools, tool definitions, the skill list, plugin parts and the error of the estimate",
      },
      variants: variants.map((v) => {
        const delta = v.startsWith("plugin:")
          ? -7450
          : v.startsWith("bundle:")
            ? -21820
            : -59;
        return {
          label: v.startsWith("bundle:") ? `with bundle ${v.slice(7)}` : `without ${v}`,
          total: rec.total + delta,
          deltaTokens: delta,
          deltaPercent: `${Math.round((delta / rec.total) * 100)}%`,
          cached: false,
        };
      }),
      verified: verified(cwd, state),
      notes: [],
    };
    return {
      ok: true,
      data,
      delayMs: start && options.measure === "slow" ? 6000 : undefined,
    };
  }

  function status(argv: string[]): CtlReply {
    const cwd = folderOf(argv);
    const state = stateOf(cwd);
    const b = bundles.find((x) => x.name === state.bundle);
    return {
      ok: true,
      data: {
        cwd,
        bundle: {
          folder: cwd,
          conflicts: [],
          applied: b
            ? {
                bundle: b.name,
                appliedAt: "2026-11-16T09:00:00.000Z",
                changedKeys: [],
                drift: false,
                ownedKeys: {
                  claudeMdExcludes: b.layers.exclude,
                  enabledPlugins: b.plugins.off,
                },
              }
            : null,
        },
        leftOut: { files: state.leftOut, memoryOff: state.memoryOff },
        pluginsOff: state.pluginsOff,
        estimate: {
          totalTokens: sum(rowsOf(cwd, state)),
          basis: "estimate",
          tokenizer: "cl100k_base",
        },
        measured: summary(state),
        calibration: calibration(cwd, state),
      },
    };
  }

  function loads(argv: string[], compose: boolean): CtlReply {
    const cwd = folderOf(argv);
    const state = stateOf(cwd);
    const items = rowsOf(cwd, state);
    if (compose) {
      return {
        ok: true,
        data: {
          cwd,
          basis: "estimate",
          totalTokens: sum(items),
          parts: items
            .filter((i) => i.loaded || i.lazy)
            .map((i) => ({ ...i, text: `# ${i.name}\n\nFixture text of ${i.path}.` })),
        },
      };
    }
    return {
      ok: true,
      data: {
        cwd,
        totalTokens: sum(items),
        basis: "estimate",
        tokenizer: "cl100k_base",
        calibration: calibration(cwd, state),
        notItemised:
          "skills, plugins, agents, tool definitions and Claude Code's own prompt are not rows; `context measure` gives the real total",
        items,
        ...(argv.includes("--measured")
          ? { measured: summary(state), verified: verified(cwd, state) }
          : {}),
      },
    };
  }

  function bundleCommand(argv: string[]): CtlReply {
    const verb = argv[2];
    const name = argv[3] && !argv[3].startsWith("--") ? argv[3] : null;
    const dry = argv.includes("--dry-run");
    const found = bundles.find((b) => b.name === name);
    const missing: CtlReply = {
      ok: false,
      error: { code: "not_found", message: `no bundle called ${name}` },
    };
    switch (verb) {
      case "ls":
        return {
          ok: true,
          data: { bundles: bundles.map(bundleRow), directory: PROFILES },
        };
      case "show":
        return found ? { ok: true, data: found } : missing;
      case "status": {
        const cwd = folderOf(argv);
        return { ok: true, data: { folder: cwd, applied: null, conflicts: [] } };
      }
      case "apply":
        return switchBundle(argv, true, name);
      case "undo":
        return switchBundle(argv, false, null);
      case "launch":
        return found
          ? {
              ok: true,
              data: {
                bundle: found.name,
                settingsFile: `${FIXTURE_HOME}/.config/toolport/profiles/${found.name}.settings.json`,
                command: `claude --settings ${FIXTURE_HOME}/.config/toolport/profiles/${found.name}.settings.json`,
                cwd: null,
                notes: [],
              },
            }
          : missing;
      case "config": {
        const at = argv.indexOf("--auto-apply");
        if (at >= 0) autoApply = argv[at + 1] === "on";
        return {
          ok: true,
          data: { autoApply, file: `${FIXTURE_HOME}/.config/mcpm/context.json` },
        };
      }
      case "add":
      case "edit": {
        const add = verb === "add";
        if (add && found) {
          return {
            ok: false,
            error: { code: "exists", message: `bundle ${name} exists` },
          };
        }
        if (!add && !found) return missing;
        const p = plan(
          `${add ? "Add" : "Change"} bundle ${name}`,
          [
            {
              op: add ? "create" : "update",
              path: `${PROFILES}/${name}.yaml`,
              detail: `${add ? "write the new" : "rewrite the"} bundle ${name}`,
            },
          ],
          add
            ? `toolportctl context bundle rm ${name}`
            : "toolportctl context bundle edit",
        );
        if (!dry) {
          const description = flag(argv, "--description");
          if (add) bundles.push(makeBundle(name!, description ?? "", {}));
          else if (found && description !== null) found.description = description;
        }
        return {
          ok: true,
          data: {
            name,
            dryRun: dry,
            created: add,
            issues: [],
            plan: p,
            result: dry
              ? null
              : { applied: true, changed: [], undo: p.undo, backups: [] },
          },
        };
      }
      case "rm": {
        if (!found) return missing;
        const p = plan(
          `Remove bundle ${name}`,
          [
            {
              op: "delete",
              path: found.path,
              detail: `delete the definition of bundle ${name}`,
            },
          ],
          `restore ${found.path} from git`,
        );
        if (!dry) bundles.splice(bundles.indexOf(found), 1);
        return {
          ok: true,
          data: {
            name,
            dryRun: dry,
            plan: p,
            result: dry
              ? null
              : { applied: true, changed: [found.path], undo: p.undo, backups: [] },
          },
        };
      }
      default:
        throw new Error(`Unimplemented fixture command: plus_ctl ${argv.join(" ")}`);
    }
  }

  function sync(argv: string[]): CtlReply {
    return {
      ok: true,
      data: {
        dryRun: argv.includes("--dry-run"),
        autoApply,
        bundles: autoApply
          ? [
              {
                bundle: "erp-dev",
                folder: `${FIXTURE_HOME}/work/clients/erp/portal`,
                applied: false,
              },
            ]
          : null,
        launchProfiles: options.regenerate
          ? {
              rows: [
                {
                  name: "bare",
                  state: "stale-file",
                  action: argv.includes("--dry-run") ? "would regenerate" : "regenerated",
                },
                {
                  name: "wide",
                  state: "set-missing",
                  action: "left",
                  reason:
                    "the access set wide no longer exists, so the launch sees no servers",
                  fix: "toolportctl context launch-profile add wide --set <access set>",
                },
              ],
              upToDate: 1,
            }
          : { rows: [], upToDate: 0 },
        directEntries: options.regenerate
          ? {
              rows: [
                {
                  client: "claude-code",
                  entry: "sierra-design",
                  state: "stale",
                  action: argv.includes("--dry-run") ? "would rewrite" : "rewritten",
                },
              ],
              upToDate: 0,
            }
          : { rows: [], upToDate: 0 },
        ...(syncParts?.(argv.includes("--dry-run"), argv.includes("--rules")) ?? {
          layers: {
            rows: [],
            upToDate: 0,
            files: [],
            changed: [],
            backups: [],
            undo: null,
          },
          rules: null,
          shims: {
            action: "skipped",
            file: "",
            reason: "the launch wiring is not set up",
          },
        }),
      },
    };
  }

  return {
    run(argv: string[]): CtlReply {
      const [group, sub] = argv;
      if (group === "plugins") return plugins(argv);
      switch (sub) {
        case "status":
          return status(argv);
        case "loads":
          return loads(argv, false);
        case "compose":
          return loads(argv, true);
        case "measure":
          return measure(argv);
        case "leave-out":
          return leaveOut(argv);
        case "use": {
          const none = argv.includes("--none");
          return switchBundle(argv, !none, operand(argv, 2));
        }
        case "sync":
          return sync(argv);
        case "bundle":
          return bundleCommand(argv);
        default:
          throw new Error(`Unimplemented fixture command: plus_ctl ${argv.join(" ")}`);
      }
    },
  };
}
