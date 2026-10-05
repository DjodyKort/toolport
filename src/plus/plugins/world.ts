import { CtlReplyFailure } from "../fixtures/ctlReply";
import type { CcUpdateData } from "../types/cc";
import type {
  HookEntry,
  HooksLsData,
  PluginRow,
  PluginsConfigData,
  PluginsFolderSwitchData,
  PluginsLsData,
  PluginsMcpData,
  PluginsShowData,
  PluginsUserSwitchData,
} from "../types/plugins";

/** A stateful plugins and hooks world shaped like the goldens of `plugins ls|show|config|mcp|
 * off|on|disable|enable`, `hooks ls` and `cc list|update`: an applied `plugins config`,
 * `plugins mcp`, `plugins off|on`, `plugins disable|enable` or `cc update` changes what the
 * next `plugins ls`, `plugins show`, `hooks ls` and `cc list` answer, and a preview never does. Browser safe (the goldens come in through `import.meta.glob`); the
 * folder is whatever `--cwd` says. */
interface Golden {
  envelope: { data: unknown };
}

const files = import.meta.glob<Golden>(
  "../../../src-tauri/tests/fixtures/ctl-envelopes/{plugins-ls.measured,plugins-show.cli,hooks-ls.full,plugins-config.folder.plan,plugins-mcp.deny.plan,plugins-mcp.deny.apply,plugins-mcp.allow.plan,plugins-mcp.allow.apply,plugins-off.plan,plugins-off.apply,plugins-off.again,plugins-on.plan,plugins-on.apply,plugins-on.nothing,plugins-disable.plan,plugins-disable.apply,plugins-enable.plan,plugins-enable.apply}.json",
  { eager: true, import: "default" },
);

const GOLDEN_FOLDER = "/home/demo/work/acme-erp";
const SETTINGS = ".claude/settings.local.json";
const WARNING =
  "A switched-off hook still starts a process and exits at once; only turning the plugin off removes it";

function golden<T>(stem: string): T {
  const key = Object.keys(files).find((path) => path.endsWith(`/${stem}.json`));
  if (!key) throw new Error(`no golden envelope ${stem}`);
  return JSON.parse(
    JSON.stringify(files[key].envelope.data)
      .split("<WORLD>/home")
      .join("/home/demo")
      .split("<WORLD>/data")
      .join("/home/demo/.local/share/toolport")
      .split("<WORLD>")
      .join("/home/demo"),
  ) as T;
}

const fail = (message: string) => new CtlReplyFailure("usage", message);
const notInstalled = (id: string) =>
  new CtlReplyFailure("not_found", `plugin '${id}' is not installed`);

/** The counts of `hooks ls` without the hooks a switched-off plugin no longer brings. */
function withoutHooks(
  counts: HooksLsData["counts"],
  gone: HookEntry[],
): HooksLsData["counts"] {
  const next = structuredClone(counts);
  for (const hook of gone) {
    const owner = `${hook.owner.kind}:${hook.owner.name}`;
    delete next.byOwner[owner];
    delete next.otherEventsByOwner[owner];
    const tracked = hook.event === "PreToolUse" || hook.event === "PostToolUse";
    for (const tool of hook.tools) {
      const row = next.perTool[tool as keyof typeof next.perTool];
      if (!row || !tracked) continue;
      if (hook.event === "PreToolUse") row.pre -= 1;
      else row.post -= 1;
      row.total -= 1;
    }
  }
  next.otherEvents = {};
  for (const events of Object.values(next.otherEventsByOwner))
    for (const [event, n] of Object.entries(events))
      next.otherEvents[event] = (next.otherEvents[event] ?? 0) + n;
  return next;
}

interface FolderState {
  env: Record<string, string>;
  denied: string[];
  /** Plugins `plugins off` turned off in this folder (`enabledPlugins.<id> = false`). */
  off: string[];
}

interface PluginState {
  id: string;
  installed: string;
  available: string | null;
}

export interface PluginsState {
  plugins: PluginState[];
  folders: Record<string, FolderState>;
  /** Plugins `plugins disable` turned off in the user settings. */
  disabledUser: string[];
  restartRequired: boolean;
}

const ECC = "ecc@ecc";
const DEMO = "demo-plugin@fake-market";

function initial(): PluginsState {
  return {
    plugins: [
      { id: ECC, installed: "2.1.0", available: "2.2.0" },
      { id: DEMO, installed: "1.0.0", available: null },
    ],
    folders: {},
    disabledUser: [],
    restartRequired: false,
  };
}

const flags = (argv: string[], name: string) =>
  argv.flatMap((word, i) => (word === name && argv[i + 1] ? [argv[i + 1]] : []));
const cwdOf = (argv: string[]) => flags(argv, "--cwd")[0] ?? "";
const positional = (argv: string[], skip: number) =>
  argv.slice(skip).filter((word, i, all) => {
    if (word.startsWith("--")) return false;
    const before = all[i - 1];
    return !(before && ["--cwd", "--set", "--unset"].includes(before));
  });

export function createPluginsWorld(seed: Partial<PluginsState> = {}) {
  const state: PluginsState = { ...initial(), ...structuredClone(seed) };
  const folder = (cwd: string): FolderState =>
    (state.folders[cwd] ??= { env: {}, denied: [], off: [] });
  const plugin = (id: string) => state.plugins.find((p) => p.id === id);

  const update = (p: PluginState) => ({
    state: p.available ? ("update" as const) : ("current" as const),
    available: p.available,
  });

  function enabledOf(base: PluginRow["enabled"], id: string, cwd: string) {
    const user = state.disabledUser.includes(id) ? false : base.user;
    const local = cwd !== "" && folder(cwd).off.includes(id) ? false : base.local;
    return {
      user,
      project: base.project,
      local,
      effective: local ?? base.project ?? user ?? true,
    };
  }

  function rowOf<T extends PluginRow>(base: T, p: PluginState, cwd: string): T {
    const denied = folder(cwd).denied;
    return {
      ...base,
      version: p.installed,
      update: update(p),
      enabled: enabledOf(base.enabled, p.id, cwd),
      mcpOutsideGateway: base.mcpOutsideGateway.map((s) => ({
        ...s,
        denied: cwd !== "" && denied.includes(s.key),
      })),
    };
  }

  const demo = <T extends PluginRow>(base: T): T => ({
    ...base,
    id: DEMO,
    name: "demo-plugin",
    marketplace: "fake-market",
    source: "github:acme/demo-plugin",
    adapter: null,
    mcpOutsideGateway: [],
    brings: { ...base.brings, hooks: 0, mcpServers: 0 },
    cost: { projected: { basis: "projected", value: 12 }, measured: null },
  });

  const plain = (shown: PluginsShowData): PluginsShowData => ({
    ...demo(shown),
    knobs: [],
    mcpServers: [],
    hooks: [],
    options: [],
  });

  function ls(argv: string[]): unknown {
    const cwd = cwdOf(argv);
    const base = golden<PluginsLsData>("plugins-ls.measured");
    const eccRow = base.plugins[0];
    const rows = [
      rowOf(eccRow, plugin(ECC)!, cwd),
      rowOf(demo(eccRow), plugin(DEMO)!, cwd),
    ];
    return { ...base, plugins: rows, restartRequired: state.restartRequired };
  }

  function show(argv: string[]): unknown {
    const [id] = positional(argv, 2);
    const cwd = cwdOf(argv);
    const p = plugin(id);
    if (!p) return notInstalled(id);
    const data = golden<PluginsShowData>("plugins-show.cli");
    const shown = id === DEMO ? plain(data) : data;
    const f = folder(cwd);
    return {
      ...shown,
      ...rowOf(shown, p, cwd),
      knobs: shown.knobs.map((knob) =>
        cwd !== "" && knob.env in f.env
          ? { ...knob, current: { value: f.env[knob.env], from: "folder-env" as const } }
          : knob,
      ),
      mcpServers: shown.mcpServers.map((s) => ({
        ...s,
        denied: { ...s.denied, local: cwd !== "" && f.denied.includes(s.key) },
      })),
    };
  }

  function hooks(argv: string[]): unknown {
    const cwd = cwdOf(argv);
    const data = golden<HooksLsData>("hooks-ls.full");
    const env = folder(cwd).env;
    const ids = (env.ECC_DISABLED_HOOKS ?? "")
      .split(",")
      .map((id) => id.trim())
      .filter(Boolean);
    const off = (command: string) =>
      env.ECC_HOOKS_ENABLED === "false" ||
      (env.ECC_GATEGUARD === "off" && command.includes("gateguard")) ||
      ids.some((id) => command.includes(id));
    const listed = enabledOf(
      { user: true, project: null, local: null, effective: true },
      ECC,
      cwd,
    ).effective;
    const gone = listed ? [] : data.hooks.filter((hook) => hook.owner.name === ECC);
    const kept = data.hooks.filter((hook) => !gone.includes(hook));
    return {
      ...data,
      cwd: cwd || null,
      hooks: kept.map((hook) =>
        hook.owner.name === ECC && off(hook.command) ? { ...hook, active: false } : hook,
      ),
      counts: gone.length > 0 ? withoutHooks(data.counts, gone) : data.counts,
      conflicts:
        gone.length > 0
          ? data.conflicts
              .map((c) => ({
                ...c,
                hooks: c.hooks.filter((h) => !h.startsWith(`${ECC}:`)),
              }))
              .filter((c) => new Set(c.hooks.map((h) => h.split(":")[0])).size > 1)
          : data.conflicts,
    };
  }

  const pair = (path: string, before: string, after: string) => ({
    path,
    diff: { before, after },
  });

  function config(argv: string[]): unknown {
    const [id] = positional(argv, 2);
    const cwd = cwdOf(argv);
    const dry = argv.includes("--dry-run");
    if (id !== ECC) return fail(`${id} has no adapter, so it has no settings to change`);
    if (!cwd) return fail("a folder is required: pass --cwd <dir>");
    const knobs = golden<PluginsShowData>("plugins-show.cli").knobs;
    const changes: PluginsConfigData["changes"] = [];
    const next = { ...folder(cwd).env };
    const note = (key: string, set: boolean, value: string | null) => {
      const knob = knobs.find((k) => k.key === key);
      if (!knob) return fail(`${key} is not a knob of ${id}`);
      if (knob.choices && value && !knob.choices.includes(value))
        return fail(`${key} must be one of ${knob.choices.join(", ")}`);
      changes.push({
        knob: key,
        action: set ? "set" : "unset",
        env: knob.env,
        value,
      });
      if (set && value !== null) next[knob.env] = value;
      else delete next[knob.env];
    };
    for (const assignment of flags(argv, "--set")) {
      const [key, ...rest] = assignment.split("=");
      const failed = note(key, true, rest.join("="));
      if (failed) return failed;
    }
    for (const key of flags(argv, "--unset")) {
      const failed = note(key, false, null);
      if (failed) return failed;
    }
    if (changes.length === 0)
      return fail("nothing to do: give --set <knob>=<value> or --unset <knob>");
    const before = folder(cwd).env;
    const lines = (env: Record<string, string>) =>
      changes
        .map(
          (c) =>
            `env.${c.env}: ${c.env! in env ? JSON.stringify(env[c.env!]) : "(absent)"}`,
        )
        .sort()
        .join("\n") + "\n";
    const template = golden<PluginsConfigData>("plugins-config.folder.plan");
    const path = `${cwd}/${SETTINGS}`;
    const undo = changes
      .map((c) =>
        c.action === "set"
          ? c.env! in before
            ? `--set ${c.knob}=${before[c.env!]}`
            : `--unset ${c.knob}`
          : `--set ${c.knob}=${before[c.env!] ?? ""}`,
      )
      .join(" ");
    const plan = {
      summary: `Set ${changes.length} knob(s) of ${id} in ${cwd}`,
      steps: [
        {
          op: Object.keys(before).length === 0 ? ("create" as const) : ("merge" as const),
          detail: `${Object.keys(next).length} key(s) owned by config:${id}`,
          keys: ["env"],
          ...pair(path, lines(before), lines(next)),
        },
        ...template.plan.steps.slice(1).map((step) => ({
          ...step,
          path: step.path?.replace(GOLDEN_FOLDER, cwd),
        })),
      ],
      effects: {},
      warnings: [WARNING],
      undo: `toolportctl plugins config ${id} --cwd ${cwd} ${undo}`,
    };
    if (!dry) folder(cwd).env = next;
    return {
      ...template,
      changes,
      cwd,
      dryRun: dry,
      plan,
      result: dry
        ? null
        : {
            applied: true,
            backups: [],
            changed: [path, `${cwd}/.git/info/exclude`],
            undo: plan.undo,
          },
    };
  }

  function mcp(argv: string[]): unknown {
    const [action, id, server] = positional(argv, 2);
    const cwd = cwdOf(argv);
    const dry = argv.includes("--dry-run");
    if (action !== "deny" && action !== "allow")
      return fail("usage: plugins mcp deny|allow <plugin> <server> --cwd <dir>");
    if (!cwd) return fail("a folder is required: pass --cwd <dir>");
    const shown = golden<PluginsShowData>("plugins-show.cli");
    const row = id === ECC ? shown.mcpServers.find((s) => s.name === server) : undefined;
    if (!row) return fail(`${id} has no MCP server ${server}`);
    const stem = `plugins-mcp.${action}.${dry ? "plan" : "apply"}`;
    const text = JSON.stringify(golden<PluginsMcpData>(stem))
      .split(GOLDEN_FOLDER)
      .join(cwd)
      .split("chrome-devtools")
      .join(row.name);
    const data = JSON.parse(text) as PluginsMcpData;
    const f = folder(cwd);
    if (!dry)
      f.denied =
        action === "deny"
          ? [...new Set([...f.denied, row.key])]
          : f.denied.filter((key) => key !== row.key);
    return data;
  }

  const bringsText = (id: string) => {
    const b =
      id === DEMO
        ? demo(golden<PluginsLsData>("plugins-ls.measured").plugins[0]).brings
        : null;
    return b
      ? [
          [b.skills, "skills"],
          [b.agents, "agents"],
          [b.commands, "commands"],
        ]
          .filter(([n]) => n)
          .map(([n, word]) => `${n} ${word}`)
          .join(", ")
      : null;
  };

  /** A golden envelope of a switch, for this plugin and folder. */
  function switched<T>(stem: string, id: string, cwd: string | null): T {
    const text = JSON.stringify(golden<T>(stem))
      .split(GOLDEN_FOLDER)
      .join(cwd ?? GOLDEN_FOLDER)
      .split(ECC)
      .join(id);
    const data = JSON.parse(text) as { plan: { steps: Array<{ detail: string }> } };
    const brings = bringsText(id);
    if (brings)
      for (const step of data.plan.steps)
        step.detail = step.detail.replace(
          /^(goes away[^:]*|comes back[^:]*): .*$/,
          `$1: ${brings}`,
        );
    return data as T;
  }

  function folderSwitch(argv: string[], action: "off" | "on"): unknown {
    const [id] = positional(argv, 2);
    const cwd = cwdOf(argv);
    const dry = argv.includes("--dry-run");
    if (!id) return fail(`usage: plugins ${action} <id> --cwd <dir> [--dry-run]`);
    if (!cwd)
      return fail(
        "--cwd is required: a plugin is turned off or on per folder (user scope: plugins disable|enable)",
      );
    if (!plugin(id)) return notInstalled(id);
    const f = folder(cwd);
    const has = f.off.includes(id);
    const nothing = action === "off" ? has : !has;
    const stem = nothing
      ? action === "off"
        ? "plugins-off.again"
        : "plugins-on.nothing"
      : `plugins-${action}.${dry ? "plan" : "apply"}`;
    const data = switched<PluginsFolderSwitchData>(stem, id, cwd);
    if (!nothing && !dry)
      f.off = action === "off" ? [...f.off, id] : f.off.filter((off) => off !== id);
    if (nothing && !dry) data.dryRun = false;
    return data;
  }

  function userSwitch(argv: string[], action: "disable" | "enable"): unknown {
    const [id] = positional(argv, 2);
    const dry = argv.includes("--dry-run");
    if (!id) return fail(`usage: plugins ${action} <id> [--dry-run]`);
    if (!plugin(id)) return notInstalled(id);
    const data = switched<PluginsUserSwitchData>(
      `plugins-${action}.${dry ? "plan" : "apply"}`,
      id,
      null,
    );
    const off = state.disabledUser.includes(id);
    if (action === "enable" && off)
      data.plan.warnings = data.plan.warnings.filter((w) => !w.startsWith("already on"));
    if (!dry) {
      state.disabledUser =
        action === "disable"
          ? [...new Set([...state.disabledUser, id])]
          : state.disabledUser.filter((disabled) => disabled !== id);
    }
    return data;
  }

  function cc(argv: string[]): unknown {
    const dry = argv.includes("--dry-run");
    const [name] = positional(argv, 2);
    const chosen = state.plugins.filter(
      (p) => !name || p.id.split("@")[0] === name || p.id === name,
    );
    if (name && chosen.length === 0) return notInstalled(name);
    const row = (p: PluginState, outcome: string | null) => ({
      available: p.available,
      blocked: false,
      enabled: true,
      error: null,
      id: p.id,
      installed: p.installed,
      marketplace: p.id.split("@")[1],
      name: p.id.split("@")[0],
      outcome,
      status: p.available ? "update" : "current",
    });
    if (argv[1] === "list")
      return {
        mode: "list",
        plugins: state.plugins.map((p) => row(p, null)),
        refreshError: null,
        restartRequired: state.restartRequired,
      } satisfies CcUpdateData;
    if (argv[1] !== "update") return undefined;
    const outcomes = chosen.map((p) => {
      const move = p.available !== null;
      const shown = row(p, move ? (dry ? "would update" : "updated") : "up to date");
      if (move && !dry) {
        p.installed = p.available!;
        p.available = null;
      }
      return shown;
    });
    if (!dry && outcomes.some((o) => o.outcome === "updated"))
      state.restartRequired = true;
    return {
      mode: dry ? "dry-run" : "update",
      plugins: outcomes,
      refreshError: null,
      restartRequired: !dry && state.restartRequired,
    } satisfies CcUpdateData;
  }

  /** The reply to an argv, or undefined when the world does not run it. */
  function reply(argv: string[]): unknown {
    const key = `${argv[0]} ${argv[1]}`;
    switch (key) {
      case "plugins ls":
        return ls(argv);
      case "plugins show":
        return show(argv);
      case "plugins config":
        return config(argv);
      case "plugins mcp":
        return mcp(argv);
      case "plugins off":
        return folderSwitch(argv, "off");
      case "plugins on":
        return folderSwitch(argv, "on");
      case "plugins disable":
        return userSwitch(argv, "disable");
      case "plugins enable":
        return userSwitch(argv, "enable");
      case "hooks ls":
        return hooks(argv);
      case "cc list":
      case "cc update":
        return cc(argv);
      default:
        return undefined;
    }
  }

  return { state, reply };
}

export type PluginsWorld = ReturnType<typeof createPluginsWorld>;
