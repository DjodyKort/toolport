/** A stateful version of the Context fixtures: every command of the screen answers from one
 * small home, and an applied write changes what the next read returns (a sync writes the
 * shims, a profile add or remove edits the list, an init scaffolds the personal layer, a
 * disable removes the shims, a folders flag turns routing on). A preview (`--dry-run`) never
 * changes anything. The e2e tests and the dev browser fixture walk the screen through it.
 * Browser safe: no node imports. */
import type { LoadItem, LoadsData } from "../bridge/data";
import {
  HOME,
  SHIMS,
  bareProfile,
  clientLayer,
  personalLayer,
  zshrcPlan,
} from "./fixtures";
import type { Check, LayerRow, ProfileRow, Selection, ZshrcLine } from "./model";

/** A reply that is a failed envelope; `data` is what a command that exits 1 still prints. */
export class Failure {
  constructor(
    readonly code: string,
    readonly message: string,
    readonly data?: unknown,
  ) {}
}

export interface WorldOptions {
  /** A home where nothing is set up: no layers, no profiles, no shims file. */
  fresh?: boolean;
}

const BASE = `${HOME}/.config/toolport`;
const CONFIG = `${BASE}/context.json`;
const NAME = /^[A-Za-z0-9][A-Za-z0-9_-]*$/;
const ZSHRC = `${HOME}/.zshrc`;
const WINDOW = 200000;
const OPTIONS = [[], ["--rules"], ["--rewrite-zshrc"], ["--rules", "--rewrite-zshrc"]];
const DEPLOY = new Set(["context plan", "context apply", "context sync"]);
const VALUED = new Set([
  "--rules",
  "--servers",
  "--org-mode",
  "--glob",
  "--profile",
  "--cwd",
  "--window",
  "--checkpoint-at",
  "--home",
]);

const legacyLine: ZshrcLine = {
  line: 3,
  file: "context-shims.zsh",
  text: "source ~/.config/mcpm/context-shims.zsh",
};
const deadAlias = {
  name: "toolup",
  file: `${HOME}/.config/mcpm/local-aliases.zsh`,
  line: 1,
  command: "cd ~/toolport && ./update.sh",
};

interface Parsed {
  path: string;
  words: string[];
  flags: Map<string, string | true>;
}

function parse(argv: string[]): Parsed {
  const group = argv[1] === "profile" || argv[1] === "client";
  const path = argv.slice(0, group ? 3 : 2).join(" ");
  const words: string[] = [];
  const flags = new Map<string, string | true>();
  const tokens = argv.slice(group ? 3 : 2);
  for (let i = 0; i < tokens.length; i += 1) {
    const token = tokens[i];
    if (!token.startsWith("--")) {
      words.push(token);
    } else if (VALUED.has(token) && !(token === "--rules" && DEPLOY.has(path))) {
      flags.set(token, tokens[(i += 1)] ?? "");
    } else {
      flags.set(token, true);
    }
  }
  return { path, words, flags };
}

const selection = (value: string | true | undefined): Selection =>
  value === undefined || value === true || value === "inherit"
    ? "inherit"
    : value === "none"
      ? "none"
      : value.split(",").filter(Boolean);

const serverCount = (value: Selection) => (Array.isArray(value) ? value.length : 0);

interface State {
  config: boolean;
  layers: LayerRow[];
  profiles: ProfileRow[];
  shims: boolean;
  legacy: ZshrcLine[];
  orderBad: boolean;
  folders: boolean;
}

const seed = (fresh: boolean): State =>
  fresh
    ? {
        config: false,
        layers: [],
        profiles: [],
        shims: false,
        legacy: [],
        orderBad: false,
        folders: false,
      }
    : {
        config: true,
        layers: [personalLayer, clientLayer],
        profiles: [bareProfile as ProfileRow],
        shims: true,
        legacy: [legacyLine],
        orderBad: true,
        folders: false,
      };

const org = { kind: "org", name: "corp-tools" } as const;
const user = { kind: "user", name: "~/.claude" } as const;

function loadItem(
  kind: LoadItem["kind"],
  name: string,
  source: string,
  tokens: number,
  reason: string,
  options: { lazy?: boolean; origin?: LoadItem["origin"]; path?: string } = {},
): LoadItem {
  const lazy = options.lazy ?? false;
  return {
    basis: "estimate",
    kind,
    lazy,
    loaded: !lazy,
    name,
    origin: options.origin ?? { kind: "managed", name: "toolportctl" },
    path: options.path ?? null,
    reason,
    scope: lazy ? "paths" : "always",
    source,
    tokens,
    via: [],
    visible: null,
    writable: false,
  } as LoadItem;
}

const PERSONAL_SKILLS = ["skill-01", "skill-02", "skill-03", "skill-04"];

export interface ContextWorld {
  /** Runs one `toolportctl` argv; a `Failure` is a command that exits non-zero. */
  run(argv: string[], stdin?: string | null): unknown;
  /** The argv rows the dev browser fixture answers, each reading the live world. */
  rows(): Array<[string, () => unknown]>;
  snapshot(): State;
}

export function createContextWorld(options: WorldOptions = {}): ContextWorld {
  const s = seed(options.fresh === true);
  const dir = (name: string) => `${BASE}/claude-profiles/${name}`;
  const hasPersonal = () => s.layers.some((layer) => layer.name === "personal");

  const checks = (): Check[] =>
    [
      ["ok", "no legacy MCP duplicates"],
      ...(hasPersonal()
        ? []
        : [["warn", "no personal layer scaffolded — run `toolportctl context init`"]]),
      ...(s.shims
        ? []
        : [["warn", "shims file missing — run `toolportctl context sync`"]]),
      ...(s.shims && s.orderBad
        ? [
            [
              "warn",
              "context-shims sourced BEFORE shell-wrapper.sh in ~/.zshrc — move our source line below it",
            ],
          ]
        : []),
      ["ok", "corp-tools not installed — no coexistence constraints"],
    ] as Check[];

  const status = () => ({
    config: { exists: s.config, path: CONFIG },
    layers: s.layers,
    legacyDupes: [],
    profiles: s.profiles,
    shims: {
      exists: s.shims,
      legacyExists: false,
      legacyPath: `${HOME}/.config/mcpm/context-shims.zsh`,
      path: SHIMS,
    },
    zshrc: {
      deadAliases: s.config ? [deadAlias] : [],
      exists: true,
      legacyLines: s.legacy,
      path: ZSHRC,
    },
  });

  const deploy = (dry: boolean, flags: Parsed["flags"], persistFlag: boolean) => {
    const rewrite = flags.has("--rewrite-zshrc");
    const lines = s.legacy.length;
    const actions = [
      ...s.profiles.map(
        (p) =>
          `${dry ? "would generate" : "generated"} launch profile ${p.name} (${serverCount(p.servers)} server(s)) in ${p.dir}`,
      ),
      `${dry ? "would write" : "wrote"} shims: ${SHIMS}`,
      ...(dry || !persistFlag ? [] : [`saved config (${s.profiles.length} profile(s))`]),
      ...(rewrite && lines
        ? [`${dry ? "would rewrite" : "rewrote"} ${lines} line(s) of ${ZSHRC}`]
        : []),
    ];
    const zshrc =
      rewrite && lines
        ? {
            ...zshrcPlan(dry),
            changes: s.legacy.map((l) => ({
              line: l.line,
              before: l.text,
              after: `source ${SHIMS}`,
            })),
            actions: [`would rewrite ${lines} line(s) of ${ZSHRC}`],
            order: s.orderBad
              ? {
                  ok: false,
                  problems: ["context-shims is sourced before shell-wrapper.sh"],
                }
              : { ok: true, problems: [] },
          }
        : undefined;
    return {
      actions,
      checks: checks(),
      dryRun: dry,
      warnings: [],
      ...(zshrc ? { zshrc } : {}),
    };
  };

  const settle = (flags: Parsed["flags"], persist: boolean) => {
    s.shims = true;
    s.profiles = s.profiles.map((p) => ({ ...p, generated: true }));
    if (persist) s.config = true;
    if (flags.has("--rewrite-zshrc")) s.legacy = [];
  };

  const loads = (profile: string | null, cwd: string): LoadsData | Failure => {
    const chosen = profile ? s.profiles.find((p) => p.name === profile) : undefined;
    if (profile && !chosen)
      return new Failure("not_found", `no launch profile named ${profile}`);
    const withOrg = chosen ? chosen.org : true;
    const withRules = chosen ? chosen.rules !== "none" : true;
    const withServers = chosen ? chosen.servers !== "none" : true;
    const items: LoadItem[] = [
      loadItem(
        "settings",
        "settings.json",
        "user",
        0,
        "configuration only, no prompt tokens",
        {
          origin: user,
          path: `${HOME}/.claude/settings.json`,
        },
      ),
      ...(withOrg
        ? [
            loadItem("memory", "CLAUDE.md", "org", 10070, "user memory", {
              origin: org,
              path: `${HOME}/.claude/CLAUDE.md`,
            }),
          ]
        : []),
      ...(withRules
        ? s.layers
            .filter((layer) => layer.globs.length > 0)
            .map((layer) =>
              loadItem(
                "rule",
                layer.name,
                "client-layer",
                56,
                "path-scoped: loads when a matching file is read",
                {
                  lazy: true,
                  path: layer.path,
                },
              ),
            )
        : []),
      ...(withRules && hasPersonal()
        ? [
            loadItem("rule", "personal", "personal", 47, "always on", {
              path: personalLayer.path,
            }),
            ...PERSONAL_SKILLS.map((name) =>
              loadItem(
                "skill",
                name,
                "personal",
                111,
                "name and description load at start; body on demand",
              ),
            ),
          ]
        : []),
      ...(withServers
        ? [
            loadItem(
              "mcp",
              "docs-search",
              "user",
              40,
              "server definition; tool schemas are not included in the estimate",
              { origin: user },
            ),
          ]
        : []),
      loadItem(
        "memory",
        "work/app/CLAUDE.md",
        "project",
        5610,
        "loads when Claude reads a file in work/app",
        {
          lazy: true,
        },
      ),
    ];
    const byKind: Record<string, number> = {};
    for (const item of items) {
      if (item.loaded) byKind[item.kind] = (byKind[item.kind] ?? 0) + item.tokens;
    }
    const skillTokens = byKind.skill ?? 0;
    return {
      basis: "estimate",
      clobbers: [],
      cwd,
      measured: null,
      measured_info: null,
      notes: [],
      partial: false,
      profile,
      skill_budget: {
        capped: [],
        context_window: WINDOW,
        fraction: 0.01,
        limit_tokens: 2000,
        used_tokens: skillTokens,
      },
      tokens_by_kind: byKind,
      total_tokens: Object.values(byKind).reduce((a, b) => a + b, 0),
      tokens_lazy: items.filter((i) => i.lazy).reduce((a, i) => a + i.tokens, 0),
      items,
    } as LoadsData;
  };

  const foldersData = () => {
    const total = loads(null, HOME);
    return {
      enabled: s.folders,
      folders: [
        {
          applies: false,
          launchProfile: null,
          profile: null,
          reason: "no mapping matches this folder",
          root: HOME,
          rule: null,
          tokens: total instanceof Failure ? 0 : total.total_tokens,
          wouldApply: null,
        },
      ],
      mappings: [],
    };
  };

  const checkpoint = (parsed: Parsed, stdin: string | null) => {
    let input: {
      context_window?: { context_window_size?: number; used_percentage?: number };
    };
    try {
      input = JSON.parse(stdin ?? "");
    } catch {
      return new Failure(
        "bad_input",
        `statusline JSON: ${(stdin ?? "").trim() === "" ? "EOF while parsing a value at line 1 column 0" : "expected value at line 1 column 1"}`,
      );
    }
    const given = Number(parsed.flags.get("--window"));
    const fromJson = input.context_window?.context_window_size;
    const window = given || fromJson || WINDOW;
    const at = Number(parsed.flags.get("--checkpoint-at")) || 50000;
    const used = Math.round(
      (window * (input.context_window?.used_percentage ?? 0)) / 100,
    );
    const point = window - at;
    return {
      at_checkpoint: used >= point,
      checkpoint_at: at,
      checkpoint_point: point,
      remaining_to_checkpoint: Math.max(0, point - used),
      remaining_to_compact: Math.max(0, window - used),
      used_tokens: used,
      window,
      window_source: given ? "flag" : fromJson ? "model" : "default",
    };
  };

  const profileData = (
    parsed: Parsed,
    name: string,
    dry: boolean,
  ): { data: unknown; row: ProfileRow } => {
    const existing = s.profiles.find((p) => p.name === name);
    const row: ProfileRow = {
      name,
      shim: `claude-${name}`,
      org: !parsed.flags.has("--no-org"),
      orgMode: String(parsed.flags.get("--org-mode") ?? "import"),
      rules: selection(parsed.flags.get("--rules")),
      servers: selection(parsed.flags.get("--servers")),
      generated: !dry,
      dir: dir(name),
    };
    return {
      row,
      data: {
        actions: [
          `${dry ? "would generate" : "generated"} launch profile ${name} (${serverCount(row.servers)} server(s)) in ${row.dir}`,
          `${dry ? "would write" : "wrote"} shims: ${SHIMS}`,
          ...(dry
            ? []
            : [`saved config (${s.profiles.length + (existing ? 0 : 1)} profile(s))`]),
        ],
        dryRun: dry,
        profile: { ...row, created: !existing },
        warnings: [],
      },
    };
  };

  const usage = (what: string, line: string) =>
    new Failure("usage", `missing ${what}\nusage: ${line}`);

  function run(argv: string[], stdin: string | null = null): unknown {
    if (argv[0] !== "context") return new Failure("usage", `unknown command ${argv[0]}`);
    const parsed = parse(argv);
    const dry = parsed.flags.has("--dry-run");
    const [name] = parsed.words;
    switch (parsed.path) {
      case "context status":
        return status();
      case "context profile list":
        return { profiles: s.profiles };
      case "context client list":
        return { layers: s.layers };
      case "context plan":
        return deploy(true, parsed.flags, false);
      case "context apply": {
        const persist = !parsed.flags.has("--no-persist");
        const data = deploy(dry, parsed.flags, persist);
        if (!dry) settle(parsed.flags, persist);
        return data;
      }
      case "context sync": {
        const plan = deploy(true, parsed.flags, false);
        if (dry) return { apply: null, dryRun: true, plan };
        const apply = deploy(false, parsed.flags, true);
        settle(parsed.flags, true);
        return { apply, dryRun: false, plan };
      }
      case "context init": {
        const created = !hasPersonal();
        const data = {
          config: { keptUnreadable: null, path: CONFIG, saved: !dry },
          dryRun: dry,
          migration: null,
          nextSteps: [
            {
              command: "toolportctl skills sync",
              label: "Edit your personal layer, then:",
            },
            {
              command: "toolportctl context client add <name>",
              label: "Per-client layer:",
            },
            { command: "toolportctl context sync", label: "Apply everything:" },
          ],
          personal: { created, path: personalLayer.path },
        };
        if (!dry) {
          s.config = true;
          if (created) s.layers = [personalLayer, ...s.layers];
        }
        return data;
      }
      case "context client add": {
        if (!name)
          return usage(
            "client name",
            "context client add <name> [--glob <pattern>] [--home <dir>] [--dry-run]",
          );
        const rule = `client-${name}`;
        const glob = String(parsed.flags.get("--glob") ?? `**/clients/${name}/**`);
        const path = `${BASE}/skills_repo/rules/${rule}/SKILL.md`;
        const created = !s.layers.some((layer) => layer.name === rule);
        if (!dry && created)
          s.layers = [
            ...s.layers,
            {
              name: rule,
              path,
              globs: [glob],
              description: `Rules for the ${name} client`,
            },
          ];
        return { created, dryRun: dry, glob, name, path, rule };
      }
      case "context profile add": {
        if (!name)
          return usage(
            "profile name",
            "context profile add <name> [--no-org] [--org-mode import|copy] [--rules inherit|none|<a,b>] [--servers inherit|none|<a,b>] [--no-commands] [--no-skills] [--home <dir>] [--dry-run]",
          );
        if (!NAME.test(name))
          return new Failure("bad_input", `invalid profile name ${name}`);
        const made = profileData(parsed, name, dry);
        if (!dry) {
          s.profiles = [...s.profiles.filter((p) => p.name !== name), made.row];
          s.shims = true;
          s.config = true;
        }
        return made.data;
      }
      case "context profile remove": {
        if (!name)
          return usage(
            "profile name",
            "context profile remove <name> [--purge] [--home <dir>] [--dry-run]",
          );
        const found = s.profiles.find((p) => p.name === name);
        if (!found) return new Failure("not_found", `no launch profile named ${name}`);
        const purge = parsed.flags.has("--purge");
        const data = {
          actions: [
            ...(purge
              ? [`${dry ? "would remove" : "removed"} profile dir ${found.dir}`]
              : []),
            `${dry ? "would write" : "wrote"} shims: ${SHIMS}`,
            ...(dry ? [] : [`saved config (${s.profiles.length - 1} profile(s))`]),
          ],
          dryRun: dry,
          inConfig: true,
          name,
          purged: purge,
          warnings: [],
        };
        if (!dry) s.profiles = s.profiles.filter((p) => p.name !== name);
        return data;
      }
      case "context disable": {
        const purge = parsed.flags.has("--purge-profiles");
        const dirs = purge ? s.profiles.filter((p) => p.generated).map((p) => p.dir) : [];
        const data = {
          actions: [
            ...(s.shims ? [`${dry ? "would remove" : "removed"} shims file`] : []),
            ...dirs.map((d) => `${dry ? "would remove" : "removed"} profile dir ${d}`),
          ],
          dryRun: dry,
          purgedProfiles: dirs,
          shims: { path: SHIMS, removed: s.shims },
          warnings: [],
        };
        if (!dry) {
          s.shims = false;
          if (purge) s.profiles = s.profiles.map((p) => ({ ...p, generated: false }));
        }
        return data;
      }
      case "context loads": {
        const profile = parsed.flags.get("--profile");
        const cwd = parsed.flags.get("--cwd");
        return loads(
          typeof profile === "string" ? profile : null,
          typeof cwd === "string" ? cwd : HOME,
        );
      }
      case "context folders": {
        if (parsed.flags.has("--enable")) s.folders = true;
        if (parsed.flags.has("--disable")) s.folders = false;
        return foldersData();
      }
      case "context checkpoint-status":
        return checkpoint(parsed, stdin);
      default:
        return new Failure("usage", `unknown command ${argv.join(" ")}`);
    }
  }

  function rows(): Array<[string, () => unknown]> {
    const out: Array<[string, () => unknown]> = [];
    const row = (argv: string, stdin: string | null = null) => {
      out.push([
        argv,
        () => {
          const data = run(argv.split(" "), stdin);
          if (data instanceof Failure) throw new Error(data.message);
          return data;
        },
      ]);
    };
    for (const read of ["status", "profile list", "client list", "folders"]) {
      row(`context ${read}`);
    }
    for (const flag of ["--enable", "--disable"]) row(`context folders ${flag}`);
    for (const profile of ["", " --profile bare", " --profile work"]) {
      row(`context loads${profile}`);
    }
    row(
      "context checkpoint-status --checkpoint-at 50000",
      JSON.stringify({
        context_window: { context_window_size: WINDOW, used_percentage: 0.75 },
      }),
    );
    for (const flags of OPTIONS) {
      const base = flags.length ? ` ${flags.join(" ")}` : "";
      row(`context plan${base}`);
      for (const verb of ["apply", "sync"]) {
        row(`context ${verb}${base}`);
        row(`context ${verb}${base} --dry-run`);
      }
      row(`context apply${base} --no-persist`);
      row(`context apply${base} --no-persist --dry-run`);
    }
    for (const preview of ["", " --dry-run"]) {
      for (const purge of ["", " --purge"]) {
        for (const profile of ["bare", "work"]) {
          row(`context profile remove ${profile}${purge}${preview}`);
        }
      }
      row(`context disable${preview}`);
      row(`context disable --purge-profiles${preview}`);
      row(`context init --yes${preview}`);
      row(`context client add partner${preview}`);
      for (const form of [
        "",
        " --no-org --rules none --servers none",
        " --rules none --servers none",
      ]) {
        row(`context profile add work${form}${preview}`);
      }
    }
    return out;
  }

  return { run, rows, snapshot: () => ({ ...s }) };
}
