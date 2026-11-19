import type { CtlReply } from "./ctlWorld";
import type {
  HookEntry,
  KnobRow,
  McpServerRow,
  OptionRow,
  PluginRow,
  PluginShowData,
  PluginsLsData,
} from "../library/pluginsData";

export const PLUGIN_FOLDER = "/Users/example/work/project-a";
const HOME = "/Users/example";
const TAP_SOURCE = "acme-skills/kit";

export interface PluginsWorldOptions {
  /** `claude` is missing: `--refresh` names why nothing was fetched and the costs stay empty. */
  noClaude?: boolean;
  /** No plugin is installed. */
  empty?: boolean;
  /** An administrator switches `kit@acme` off. */
  managed?: boolean;
}

interface Seed {
  id: string;
  version: string;
  available?: string;
  skills: string[];
  agents?: string[];
  commands?: string[];
  hooks?: HookEntry[];
  servers?: Array<{ name: string; command: string[] | null; url?: string }>;
  options?: OptionRow[];
  knobs?: KnobRow[];
  description: string;
  source: string;
  cost?: number;
  select?: boolean;
  followsFolder?: boolean;
}

const hook = (
  plugin: string,
  event: string,
  matcher: string,
  command: string,
  tools: string[],
  runtimeId: string | null = null,
): HookEntry => ({
  owner: { kind: "plugin", name: plugin },
  event,
  matcher,
  type: "command",
  command,
  runtimeId,
  timeoutSec: null,
  async: false,
  source: `~/.claude/plugins/cache/${plugin}/hooks/hooks.json`,
  marker: null,
  switch: {
    method: runtimeId ? "plugin-setting" : "disable-plugin",
    detail: runtimeId
      ? `plugins config ${plugin} --set disabled_hooks=${runtimeId} --cwd <dir>; the hook still starts a process and exits at once`
      : `turn the plugin off: plugins off ${plugin} --cwd <dir>`,
  },
  tools,
});

const profileKnob = (value: string, from: string): KnobRow => ({
  key: "profile",
  label: "Profile",
  kind: "enum",
  env: "LINT_PROFILE",
  option: "profile",
  choices: ["relaxed", "standard", "strict"],
  default: "standard",
  folderOnly: false,
  list: false,
  current: { value, from },
});

const guardKnob = (value: string, from: string): KnobRow => ({
  key: "guard",
  label: "Guard",
  kind: "bool-off",
  env: "LINT_GUARD",
  option: null,
  choices: null,
  default: "on",
  folderOnly: true,
  list: false,
  current: { value, from },
});

const SEEDS: Seed[] = [
  {
    id: "kit@acme",
    version: "1.8.2",
    available: "1.9.0",
    description: "Everyday skills for the acme team.",
    source: "github:acme/skills",
    skills: ["commit-notes", "review-pr", "write-tests"],
    agents: ["code-scout", "planner"],
    commands: ["ship", "triage", "standup", "release"],
    hooks: [
      hook("kit@acme", "SessionStart", "", "node scripts/start.js", []),
      hook("kit@acme", "PreToolUse", "Bash", "node scripts/guard.js", ["Bash"]),
    ],
    servers: [{ name: "docs", command: ["npx", "-y", "docs-mcp"] }],
    options: [
      {
        key: "mode",
        title: "Mode",
        description: "How the kit works",
        type: "string",
        default: "standard",
        sensitive: false,
        current: "standard",
        configured: true,
        choices: ["standard", "quiet"],
      },
      {
        key: "api_token",
        title: "API token",
        description: "Used for the docs server",
        type: "string",
        default: null,
        sensitive: true,
        current: null,
        configured: null,
        choices: null,
      },
    ],
    cost: 640,
    select: true,
    followsFolder: true,
  },
  {
    id: "kit-labs@acme",
    version: "0.4.1",
    description: "Experiments that are not ready for the kit.",
    source: "github:acme/skills",
    skills: ["spike-notes"],
    cost: 120,
  },
  {
    id: "toolkit-pro@community",
    version: "2.1.0",
    description: "A large collection of skills, agents and commands.",
    source: "github:community/toolkit",
    skills: Array.from({ length: 290 }, (_, i) => `skill-${i + 1}`),
    agents: Array.from({ length: 14 }, (_, i) => `agent-${i + 1}`),
    commands: Array.from({ length: 31 }, (_, i) => `command-${i + 1}`),
    cost: 40639,
  },
  {
    id: "notes-helper@community",
    version: "1.0.3",
    description: "Meeting notes.",
    source: "github:community/toolkit",
    skills: ["meeting-notes", "minutes"],
    cost: 210,
  },
  {
    id: "lint-rules@community",
    version: "0.9.0",
    description: "Lint rules for the code Claude writes.",
    source: "github:community/toolkit",
    skills: ["lint-fix"],
    hooks: [
      hook(
        "lint-rules@community",
        "PreToolUse",
        "Edit|Write",
        "node run.js pre:edit",
        ["Edit", "Write"],
        "pre:edit",
      ),
      hook(
        "lint-rules@community",
        "PostToolUse",
        "Edit|Write",
        "node run.js post:edit",
        ["Edit", "Write"],
        "post:edit",
      ),
      hook("lint-rules@community", "Stop", "", "node run.js stop", [], "stop"),
    ],
    knobs: [profileKnob("standard", "default"), guardKnob("on", "default")],
    cost: 380,
  },
];

const row = (
  seed: Seed,
  folderOff: boolean,
  managedOff: boolean,
  denied: Set<string>,
): PluginRow => {
  const [name, marketplace] = seed.id.split("@");
  return {
    id: seed.id,
    name,
    marketplace,
    version: seed.version,
    scope: "user",
    installPath: `~/.claude/plugins/cache/${marketplace}/${name}/${seed.version}`,
    source: seed.source,
    enabled: {
      user: true,
      project: null,
      local: folderOff ? false : null,
      managed: managedOff ? false : null,
      effective: !folderOff && !managedOff,
    },
    update: seed.available
      ? { state: "update", available: seed.available }
      : { state: "current", available: null },
    brings: {
      skills: seed.skills.length,
      agents: seed.agents?.length ?? 0,
      commands: seed.commands?.length ?? 0,
      hooks: seed.hooks?.length ?? 0,
      mcpServers: seed.servers?.length ?? 0,
      lspServers: 0,
    },
    cost: {
      projected: seed.cost ? { value: seed.cost, basis: "projected" } : null,
      measured: null,
      measuredAt: null,
    },
    adapter: seed.knobs ? seed.id : null,
    selectSource: seed.select ? TAP_SOURCE : null,
    mcpOutsideGateway: (seed.servers ?? []).map((s) => ({
      key: `plugin:${name}:${s.name}`,
      name: s.name,
      denied: denied.has(`${seed.id}|${s.name}`),
    })),
    errors: [],
    from: "claude-cli",
  };
};

const flag = (argv: string[], name: string) => argv.includes(name);
const value = (argv: string[], name: string) => {
  const at = argv.indexOf(name);
  return at >= 0 ? (argv[at + 1] ?? null) : null;
};
const all = (argv: string[], name: string) =>
  argv.flatMap((arg, i) => (arg === name && argv[i + 1] ? [argv[i + 1]] : []));

const fail = (code: string, message: string): CtlReply => ({
  ok: false,
  error: { code, message },
});

/** A small in-memory toolportctl for `plugins ls|show|config|mcp` and `hooks ls`: a folder can
 * deny a server or set a knob and the next read shows it. The plugin switch itself
 * (`plugins off|on`) lives in the Context world; `off` reports the folder's switched-off ids. */
export function createPluginsWorld(
  options: PluginsWorldOptions = {},
  off: (cwd: string) => string[] = () => [],
) {
  const seeds = options.empty ? [] : SEEDS;
  const written = new Map<string, Map<string, string>>();
  const global = new Map<string, string>();
  const denied = new Map<string, Set<string>>();
  let refreshed = false;

  const folderKey = (cwd: string, id: string) => `${cwd}|${id}`;
  const deniedIn = (cwd: string | null) => denied.get(cwd ?? "") ?? new Set<string>();

  function rowFor(seed: Seed, cwd: string | null): PluginRow {
    const folderOff = cwd ? off(cwd).includes(seed.id) : false;
    return row(
      seed,
      folderOff,
      !!options.managed && seed.id === "kit@acme",
      deniedIn(cwd),
    );
  }

  function find(id: string): Seed | null {
    return seeds.find((s) => s.id === id || s.id.split("@")[0] === id) ?? null;
  }

  function knobsOf(seed: Seed, cwd: string | null): KnobRow[] {
    return (seed.knobs ?? []).map((knob) => {
      const mine = cwd ? written.get(folderKey(cwd, seed.id))?.get(knob.key) : undefined;
      if (mine !== undefined)
        return { ...knob, current: { value: mine, from: "folder-env" } };
      const set = global.get(knob.key);
      if (set !== undefined) return { ...knob, current: { value: set, from: "option" } };
      return knob;
    });
  }

  function servers(seed: Seed, cwd: string | null): McpServerRow[] {
    const name = seed.id.split("@")[0];
    return (seed.servers ?? []).map((s) => {
      const key = `plugin:${name}:${s.name}`;
      const here = deniedIn(cwd).has(`${seed.id}|${s.name}`);
      return {
        key,
        name: s.name,
        type: null,
        command: s.command,
        url: s.url ?? null,
        toolPrefix: `mcp__plugin_${name}_${s.name}__*`,
        denied: { user: false, project: false, local: here, managed: false },
        deniedByToolport: here,
      };
    });
  }

  function ls(argv: string[]): CtlReply {
    const cwd = value(argv, "--cwd");
    if (flag(argv, "--refresh")) refreshed = !options.noClaude;
    const plugins = seeds.map((s) => rowFor(s, cwd));
    const markets = [...new Set(plugins.map((p) => p.marketplace as string))];
    const data: PluginsLsData = {
      cwd,
      plugins,
      marketplaces: markets.map((name) => ({
        name,
        source: plugins.find((p) => p.marketplace === name)?.source ?? null,
        followsFolder: name === "acme",
        installed: plugins.filter((p) => p.marketplace === name).length,
      })),
      updatesAvailable: plugins.filter((p) => p.update.state === "update").length,
      managedSettings: {
        path: options.managed
          ? "/Library/Application Support/ClaudeCode/managed-settings.json"
          : null,
        present: !!options.managed,
        problem: null,
      },
      refreshed: flag(argv, "--refresh") && !options.noClaude,
      refreshError:
        flag(argv, "--refresh") && options.noClaude ? "claude not found on PATH" : null,
      partial: !!options.noClaude && plugins.length > 0,
    };
    void refreshed;
    return { ok: true, data };
  }

  function show(argv: string[]): CtlReply {
    const seed = find(argv[2] ?? "");
    if (!seed) return fail("not_found", `plugin not found: ${argv[2] ?? ""}`);
    const cwd = value(argv, "--cwd");
    const data: PluginShowData = {
      ...rowFor(seed, cwd),
      description: seed.description,
      components: {
        skills: seed.skills,
        agents: seed.agents ?? [],
        commands: seed.commands ?? [],
        lspServers: [],
      },
      hooks: seed.hooks ?? [],
      mcpServers: servers(seed, cwd),
      options: seed.options ?? [],
      optionsFrom: "claude-cli",
      knobs: knobsOf(seed, cwd),
      adapterProblems: [],
    };
    return { ok: true, data };
  }

  function planOf(summary: string, undo: string, cwd: string | null, detail: string) {
    return {
      summary,
      steps: [
        {
          op: cwd ? "merge" : "note",
          path: cwd ? `${cwd}/.claude/settings.local.json` : undefined,
          detail,
          ...(cwd
            ? {
                keys: ["env"],
                diff: { before: "env: (absent)\n", after: `${detail}\n` },
              }
            : {}),
        },
      ],
      effects: {},
      warnings: [
        "Claude Code reads plugin settings when a session starts: sessions that are already running keep the old state until they restart",
      ],
      undo,
    };
  }

  function config(argv: string[]): CtlReply {
    const seed = find(argv[2] ?? "");
    if (!seed)
      return fail("not_found", `${argv[2] ?? ""} is not among the installed plugins`);
    const cwd = value(argv, "--cwd");
    const sets = all(argv, "--set");
    const unsets = all(argv, "--unset");
    const knobs = knobsOf(seed, cwd);
    if (sets.length === 0 && unsets.length === 0) {
      return {
        ok: true,
        data: {
          id: seed.id,
          scope: cwd ? "folder" : "global",
          cwd,
          adapter: seed.knobs ? seed.id : null,
          options: seed.options ?? [],
          optionsFrom: "claude-cli",
          knobs,
          writtenByToolport: cwd
            ? [...(written.get(folderKey(cwd, seed.id)) ?? new Map())].map(
                ([knob, v]) => ({
                  knob,
                  env: (seed.knobs ?? []).find((k) => k.key === knob)?.env ?? knob,
                  value: v,
                }),
              )
            : [],
          adapterProblems: [],
        },
      };
    }
    if (!seed.knobs) {
      return fail(
        "usage",
        `${seed.id} has no adapter, so Toolport does not know its switches`,
      );
    }
    const changes: Array<Record<string, unknown>> = [];
    for (const pair of sets) {
      const [key, ...rest] = pair.split("=");
      const knob = seed.knobs.find((k) => k.key === key);
      if (!knob) return fail("usage", `${key} is not a knob of ${seed.id}`);
      if (!cwd && knob.folderOnly) {
        return fail(
          "usage",
          `${key} is a switch of one folder, not a plugin option: pass --cwd <dir>`,
        );
      }
      changes.push({
        knob: key,
        env: cwd ? knob.env : null,
        option: cwd ? null : knob.option,
        action: "set",
        value: rest.join("="),
      });
    }
    for (const key of unsets) {
      const knob = seed.knobs.find((k) => k.key === key);
      if (!knob) return fail("usage", `${key} is not a knob of ${seed.id}`);
      changes.push({
        knob: key,
        env: cwd ? knob.env : null,
        option: cwd ? null : knob.option,
        action: "unset",
        value: null,
      });
    }
    const dryRun = flag(argv, "--dry-run");
    const touched = changes.map((c) => c.knob as string);
    const undo = `toolportctl plugins config ${seed.id}${cwd ? ` --cwd ${cwd}` : ""} ${touched.map((k) => `--unset ${k}`).join(" ")}`;
    if (!dryRun) {
      for (const change of changes) {
        const key = change.knob as string;
        if (cwd) {
          const mine = written.get(folderKey(cwd, seed.id)) ?? new Map<string, string>();
          if (change.action === "set") mine.set(key, change.value as string);
          else mine.delete(key);
          written.set(folderKey(cwd, seed.id), mine);
        } else if (change.action === "set") global.set(key, change.value as string);
        else global.delete(key);
      }
    }
    return {
      ok: true,
      data: {
        id: seed.id,
        adapter: seed.id,
        scope: cwd ? "folder" : "global",
        cwd,
        dryRun,
        plan: planOf(
          `Set ${touched.join(", ")} for ${seed.id}${cwd ? ` in ${cwd}` : ""}`,
          undo,
          cwd,
          cwd
            ? `writes env keys of ${seed.id}; every other key stays as it is`
            : `run claude plugin configure ${seed.id} --values-stdin`,
        ),
        result: dryRun
          ? null
          : {
              applied: true,
              changed: [
                cwd
                  ? `${cwd}/.claude/settings.local.json`
                  : `plugin options of ${seed.id}`,
              ],
              undo,
              backups: [],
            },
        changes,
        conflicts: [],
        ledger: cwd
          ? "/Users/example/Library/Application Support/Toolport/plus/profile-ledger.json"
          : null,
      },
    };
  }

  function mcp(argv: string[]): CtlReply {
    const action = argv[2];
    const seed = find(argv[3] ?? "");
    if (!seed)
      return fail("not_found", `${argv[3] ?? ""} is not among the installed plugins`);
    const cwd = value(argv, "--cwd");
    if (action === "ls") {
      return {
        ok: true,
        data: {
          id: seed.id,
          cwd,
          servers: servers(seed, cwd),
          outsideGateway:
            "these servers are started by Claude Code, not by the Toolport gateway",
        },
      };
    }
    const serverName = argv[4] ?? "";
    const server = (seed.servers ?? []).find((s) => s.name === serverName);
    if (!server) {
      return fail("not_found", `${seed.id} has no MCP server ${serverName}`);
    }
    if (!cwd) return fail("usage", "--cwd is required: a server is denied per folder");
    const deny = action === "deny";
    const dryRun = flag(argv, "--dry-run");
    const key = `plugin:${seed.id.split("@")[0]}:${server.name}`;
    const undo = `toolportctl plugins mcp ${deny ? "allow" : "deny"} ${seed.id} ${server.name} --cwd ${cwd}`;
    if (!dryRun) {
      const here = new Set(denied.get(cwd) ?? []);
      if (deny) here.add(`${seed.id}|${server.name}`);
      else here.delete(`${seed.id}|${server.name}`);
      denied.set(cwd, here);
    }
    return {
      ok: true,
      data: {
        id: seed.id,
        server: server.name,
        serverName: key,
        toolPrefix: `mcp__plugin_${seed.id.split("@")[0]}_${server.name}__*`,
        scope: "folder",
        cwd,
        dryRun,
        plan: planOf(
          `${deny ? "Deny" : "Allow"} MCP server ${server.name} of ${seed.id} in ${cwd}`,
          undo,
          cwd,
          `${deny ? "add" : "remove"} ${key} in deniedMcpServers`,
        ),
        result: dryRun
          ? null
          : {
              applied: true,
              changed: [`${cwd}/.claude/settings.local.json`],
              undo,
              backups: [],
            },
        changes: [
          { key: "deniedMcpServers", action: deny ? "add" : "remove", value: key },
        ],
        conflicts: [],
        ledger:
          "/Users/example/Library/Application Support/Toolport/plus/profile-ledger.json",
      },
    };
  }

  function hooks(argv: string[]): CtlReply {
    const cwd = value(argv, "--cwd");
    const own: HookEntry = {
      owner: { kind: "user", name: "settings.json" },
      event: "PreToolUse",
      matcher: "Bash",
      type: "command",
      command: "~/bin/own.sh",
      runtimeId: null,
      timeoutSec: null,
      async: false,
      source: "~/.claude/settings.json",
      marker: null,
      switch: {
        method: "edit-settings",
        detail: "edit the hooks key of ~/.claude/settings.json",
      },
      tools: ["Bash"],
    };
    const fromPlugins = seeds
      .filter((s) => !(cwd && off(cwd).includes(s.id)))
      .flatMap((s) => s.hooks ?? []);
    const entries = [own, ...fromPlugins];
    const byOwner: Record<string, number> = {};
    const perTool: Record<string, { pre: number; post: number; total: number }> = {};
    const other: Record<string, number> = {};
    const preOwners: Record<string, string[]> = {};
    for (const entry of entries) {
      byOwner[entry.owner.kind] = (byOwner[entry.owner.kind] ?? 0) + 1;
      if (entry.event === "PreToolUse" || entry.event === "PostToolUse") {
        for (const tool of entry.tools) {
          const slot = (perTool[tool] ??= { pre: 0, post: 0, total: 0 });
          if (entry.event === "PreToolUse") {
            slot.pre += 1;
            (preOwners[tool] ??= []).push(entry.owner.name);
          } else slot.post += 1;
          slot.total += 1;
        }
      } else other[entry.event] = (other[entry.event] ?? 0) + 1;
    }
    return {
      ok: true,
      data: {
        cwd,
        disabledAll: false,
        managedOnly: false,
        managedSettings: { path: null, present: false, problem: null },
        hooks: entries,
        counts: { byOwner, perTool, otherEvents: other },
        conflicts: Object.entries(preOwners)
          .filter(([, owners]) => new Set(owners).size > 1)
          .map(([tool, owners]) => ({
            event: "PreToolUse",
            tools: [tool],
            hooks: [...new Set(owners)].sort(),
            note: "the strictest answer wins: deny, then defer, then ask, then allow",
          })),
      },
    };
  }

  return {
    run(argv: string[]): CtlReply {
      const [first, second] = argv;
      if (first === "hooks") return hooks(argv);
      if (second === "ls") return ls(argv);
      if (second === "show") return show(argv);
      if (second === "config") return config(argv);
      if (second === "mcp") return mcp(argv);
      throw new Error(`Unimplemented fixture command: plus_ctl ${argv.join(" ")}`);
    },
    /** Where a folder is denying servers, for a test that checks what the screen wrote. */
    deniedIn: (cwd: string) => [...(denied.get(cwd) ?? [])],
    written: (cwd: string, id: string) =>
      Object.fromEntries(written.get(folderKey(cwd, id)) ?? []),
  };
}

export type PluginsWorld = ReturnType<typeof createPluginsWorld>;
export const PLUGIN_HOME = HOME;
