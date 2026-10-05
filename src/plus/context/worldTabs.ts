/** The part of the stateful Context world that belongs to the tabs This folder and Profiles:
 * profiles (bundles) with their ledger, `context use`, the real measurement and the composed
 * text. An applied write changes the next read: a bundle applied in a folder lowers what
 * `context loads --cwd` returns there, `bundle status|ls` list it, a measurement is kept and
 * goes stale when the estimate moves. Browser safe: no node imports. */
import type { LoadItem, LoadsData, MeasureData, MeasureRun } from "../bridge/data";
import { Failure } from "./failure";
import type { LayerRow } from "./model";

export interface TabsHost {
  home: string;
  base: string;
  layers(): LayerRow[];
  layerMeta(name: string): { scope: string; folders: string[]; imports: string[] };
  foldersEnabled(): boolean;
}

interface Call {
  words: string[];
  flags: Map<string, string | true>;
  dry: boolean;
}

interface Bundle {
  name: string;
  description: string;
  servers: string | null;
  skillsOff: string[];
  skillsNameOnly: string[];
  skillsAllow: string[];
  pluginsOff: string[];
  layersAdd: string[];
  layersExclude: string[];
  agentsOff: string[];
  bind: string[];
  legacy: boolean;
}

interface Applied {
  bundle: string;
  folder: string;
  appliedAt: string;
  drift: boolean;
}

interface Kept {
  run: MeasureRun;
  data: MeasureData;
  estimate: number;
}

const AT = "2026-10-05T09:00:00Z";
const VERSION = "2.1.289";
const MODEL = "claude-haiku-4-5-20251001";
const OVERHEAD = 41000;
const PLUGIN_COST = 8651;
const NAME = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
const PLUGIN = "kit@market";
const LISTS: Array<[string, keyof Bundle]> = [
  ["--skills-off", "skillsOff"],
  ["--skills-name-only", "skillsNameOnly"],
  ["--skills-allow", "skillsAllow"],
  ["--plugins-off", "pluginsOff"],
  ["--layers-add", "layersAdd"],
  ["--layers-exclude", "layersExclude"],
  ["--agents-off", "agentsOff"],
  ["--bind", "bind"],
];

export function globMatch(pattern: string, text: string): boolean {
  const source = pattern
    .split("**")
    .map((part) =>
      part
        .split("*")
        .map((piece) => piece.replace(/[.+^${}()|[\]\\?]/g, "\\$&"))
        .join("[^/]*"),
    )
    .join(".*");
  return new RegExp(`^${source}$`).test(text);
}

const csv = (value: string | true | undefined) =>
  typeof value === "string"
    ? value
        .split(",")
        .map((part) => part.trim())
        .filter(Boolean)
    : [];

const seedBundles = (): Bundle[] => [
  {
    name: "acme-dev",
    description: "ERP work: fewer skills, no marketplace plugin, one agent off",
    servers: "acme-dev",
    skillsOff: ["skill-03", "skill-04"],
    skillsNameOnly: ["skill-02"],
    skillsAllow: [],
    pluginsOff: [PLUGIN],
    layersAdd: ["client-acme"],
    layersExclude: ["**/work/app/CLAUDE.md"],
    agentsOff: ["reviewer"],
    bind: ["~/work/erp/clients/*"],
    legacy: false,
  },
  {
    name: "default",
    description: "",
    servers: null,
    skillsOff: [],
    skillsNameOnly: [],
    skillsAllow: ["skill-01", "skill-02"],
    pluginsOff: [],
    layersAdd: [],
    layersExclude: [],
    agentsOff: [],
    bind: [],
    legacy: true,
  },
];

export function createTabsWorld(host: TabsHost, fresh: boolean) {
  let bundles: Bundle[] = fresh ? [] : seedBundles();
  let applied: Applied[] = [];
  let autoApply = false;
  const kept = new Map<string, Kept>();
  const dir = `${host.base}/skills_repo/profiles`;
  const pathOf = (name: string) => `${dir}/${name}.yaml`;
  const ledger = `${host.base}/data/plus/profile-ledger.json`;
  const settingsOf = (folder: string) => `${folder}/.claude/settings.local.json`;
  const unknown = (name: string) =>
    new Failure("not_found", `unknown bundle: ${name} (no ${pathOf(name)})`);

  const yamlOf = (b: Bundle) =>
    [
      "format: 1",
      `name: ${b.name}`,
      ...(b.description ? [`description: ${JSON.stringify(b.description)}`] : []),
      ...(b.servers ? [`servers: ${b.servers}`] : []),
      "skills:",
      `  off: [${b.skillsOff.join(", ")}]`,
      `  name_only: [${b.skillsNameOnly.join(", ")}]`,
      `  allow: [${b.skillsAllow.join(", ")}]`,
      "plugins:",
      `  off: [${b.pluginsOff.join(", ")}]`,
      "layers:",
      `  add: [${b.layersAdd.join(", ")}]`,
      `  exclude: [${b.layersExclude.join(", ")}]`,
      "agents:",
      `  off: [${b.agentsOff.join(", ")}]`,
      `bind: [${b.bind.join(", ")}]`,
      "",
    ].join("\n");

  const appliedTo = (name: string) =>
    applied
      .filter((one) => one.bundle === name)
      .map(({ appliedAt, drift, folder }) => ({ appliedAt, drift, folder }));
  const issuesOf = (b: Bundle) =>
    b.legacy
      ? [
          {
            key: "skills",
            level: "warning",
            message: "a plain skills list is read as skills.allow",
          },
        ]
      : [];

  const ownedKeys = (b: Bundle) => ({
    claudeMdExcludes: b.layersExclude,
    enabledPlugins: b.pluginsOff,
    permissionsDeny: b.agentsOff.map((agent) => `Agent(${agent})`),
    skillOverrides: [...b.skillsOff, ...b.skillsNameOnly],
  });

  const plan = (
    summary: string,
    steps: Array<Record<string, unknown>>,
    undo: string,
    tokens?: { before: number; after: number },
    warnings: string[] = [],
  ) => ({
    effects: tokens ? { tokens: { ...tokens, basis: "estimate" } } : {},
    steps,
    summary,
    undo,
    warnings,
  });

  const filter = (data: LoadsData, b: Bundle | undefined): LoadsData => {
    if (!b) return data;
    const items = data.items
      .filter((item) => {
        if (item.kind === "skill")
          return !(
            b.skillsOff.some((pattern) => globMatch(pattern, item.name)) ||
            (b.skillsAllow.length > 0 && !b.skillsAllow.includes(item.name))
          );
        if (item.kind === "agent") return !b.agentsOff.includes(item.name);
        return true;
      })
      .map((item): LoadItem => {
        if (item.kind === "plugin" && b.pluginsOff.includes(item.name))
          return { ...item, loaded: false };
        if (
          (item.kind === "memory" || item.kind === "rule") &&
          item.loaded &&
          b.layersExclude.some((pattern) => globMatch(pattern, item.path ?? item.name))
        )
          return { ...item, loaded: false };
        if (item.kind === "skill" && b.skillsNameOnly.includes(item.name))
          return { ...item, tokens: Math.min(item.tokens, 25) };
        return item;
      });
    return totals(data, items);
  };

  const totals = (data: LoadsData, items: LoadItem[]): LoadsData => {
    const byKind: Record<string, number> = {};
    for (const item of items) {
      if (item.loaded && !item.lazy)
        byKind[item.kind] = (byKind[item.kind] ?? 0) + item.tokens;
    }
    return {
      ...data,
      items,
      tokens_by_kind: byKind,
      total_tokens: Object.values(byKind).reduce((sum, n) => sum + n, 0),
      tokens_lazy: items
        .filter((item) => item.lazy)
        .reduce((sum, item) => sum + item.tokens, 0),
      skill_budget: { ...data.skill_budget, used_tokens: byKind.skill ?? 0 },
    };
  };

  const folderItems = (): LoadItem[] => [
    {
      basis: "estimate",
      kind: "plugin",
      lazy: false,
      loaded: true,
      name: PLUGIN,
      origin: { kind: "plugin", name: PLUGIN },
      path: null,
      reason: "2 skills, 1 agents, 2 commands",
      scope: "always",
      source: "plugin",
      tokens: 30,
      via: [],
      visible: null,
      writable: false,
    } as LoadItem,
    {
      basis: "estimate",
      kind: "plugin",
      lazy: false,
      loaded: false,
      name: "idle@market",
      origin: { kind: "plugin", name: "idle@market" },
      path: null,
      reason: "installed but not enabled in this folder",
      scope: "always",
      source: "plugin",
      tokens: 5,
      via: [],
      visible: null,
      writable: false,
    } as LoadItem,
    {
      basis: "estimate",
      kind: "agent",
      lazy: false,
      loaded: true,
      name: "reviewer",
      origin: { kind: "library", name: "skills_repo" },
      path: null,
      reason: "agent description",
      scope: "always",
      source: "library",
      tokens: 6,
      via: [],
      visible: null,
      writable: false,
    } as LoadItem,
  ];

  const current = (cwd: string) =>
    bundles.find((b) => b.name === applied.find((one) => one.folder === cwd)?.bundle);

  const measuredTotal = (data: LoadsData) =>
    data.total_tokens +
    OVERHEAD +
    (data.items.some((i) => i.kind === "plugin" && i.name === PLUGIN && i.loaded)
      ? PLUGIN_COST
      : 0);

  const runOf = (label: string, data: LoadsData): MeasureRun => {
    const total = measuredTotal(data);
    const skills = data.items.filter((i) => i.kind === "skill" && i.loaded);
    const agents = data.items.filter((i) => i.kind === "agent" && i.loaded);
    return {
      label,
      total,
      parts: { input: 10, cacheCreation: total - 13806, cacheRead: 13796 },
      skills: skills.length,
      agents: agents.length,
      slashCommands: skills.length + 2,
      plugins: data.items.some((i) => i.name === PLUGIN && i.loaded)
        ? [{ name: "kit", source: PLUGIN }]
        : [],
      mcpServers: [{ name: "docs-search", status: "connected" }],
      skillNames: skills.map((i) => i.name),
      agentNames: agents.map((i) => i.name),
      durationMs: 0,
    };
  };

  /** What `context loads --cwd` adds in a folder: the plugins and agents of the folder, the
   * applied profile, and the kept measurement when `--measured` is asked. */
  function shapeLoads(data: LoadsData, cwd: string, measured: boolean): LoadsData {
    const withFolder = totals(data, [...data.items, ...folderItems()]);
    const shaped = filter(withFolder, current(cwd));
    const keep = kept.get(`${cwd}|`);
    if (!measured || !keep) return shaped;
    return {
      ...shaped,
      measured: keep.run,
      measured_info: {
        claudeCodeVersion: VERSION,
        model: MODEL,
        measuredAt: AT,
        stale: keep.estimate !== shaped.total_tokens,
      },
    };
  }

  function measure(call: Call, raw: (cwd: string) => LoadsData): unknown {
    const cwd = String(call.flags.get("--cwd") ?? host.home);
    const without =
      typeof call.flags.get("--without") === "string"
        ? String(call.flags.get("--without"))
        : "";
    if (!call.flags.has("--yes"))
      return new Failure(
        "confirm_required",
        `this needs ${without ? 2 : 1} request(s) to Claude Code, which spend model tokens; pass --yes to go ahead`,
      );
    const asIs = shapeLoads(raw(cwd), cwd, false);
    if (without && !asIs.items.some((i) => `plugin:${i.name}` === without && i.loaded))
      return new Failure("not_found", `${without} is not loaded in ${cwd}`);
    const key = `${cwd}|${without}`;
    const old = kept.get(key);
    if (old && !call.flags.has("--force") && old.estimate === asIs.total_tokens)
      return { ...old.data, cached: true };
    const first = runOf("as is", asIs);
    const runs = [first];
    const deltas: MeasureData["deltas"] = [];
    if (without) {
      const name = without.slice("plugin:".length);
      const less = totals(
        asIs,
        asIs.items.map((i) => (i.name === name ? { ...i, loaded: false } : i)),
      );
      const second = runOf(`without ${without}`, less);
      runs.push(second);
      deltas.push({
        label: second.label,
        tokens: second.total - first.total,
        percent: Math.round(((second.total - first.total) / first.total) * 1000) / 10,
      });
    }
    const data: MeasureData = {
      cached: false,
      claudeCodeVersion: VERSION,
      cwd,
      deltas,
      invisibleSkills: [],
      measuredAt: AT,
      model: MODEL,
      notes: [],
      runs,
      stale: false,
      visibleSkills: first.skillNames,
    };
    kept.set(key, { run: first, data, estimate: asIs.total_tokens });
    if (!without) kept.set(`${cwd}|`, { run: first, data, estimate: asIs.total_tokens });
    return data;
  }

  function compose(cwd: string) {
    const b = current(cwd);
    const layers = host.layers().filter((layer) => {
      const meta = host.layerMeta(layer.name);
      if (b?.layersAdd.includes(layer.name)) return true;
      if (layer.name === "personal") return true;
      if (meta.scope === "folder")
        return meta.folders.some((f) => cwd === f || cwd.startsWith(`${f}/`));
      return layer.globs.some((glob) => globMatch(glob, `${cwd}/`));
    });
    const part = (
      name: string,
      origin: { kind: string; name: string },
      path: string,
      text: string,
      extra: { layers?: string[]; via?: string[] } = {},
    ) => ({
      kind: "memory",
      layers: extra.layers ?? [],
      lazy: false,
      name,
      origin,
      path,
      source: origin.kind,
      text,
      tokens: { basis: "estimate", value: Math.ceil(text.length / 4) },
      via: extra.via ?? [],
      writable: false,
    });
    const parts = [
      part(
        "CLAUDE.md",
        { kind: "org", name: "corp-tools" },
        `${host.home}/.claude/CLAUDE.md`,
        "# Corp rules\nBe brief.\n",
      ),
      ...(layers.length > 0
        ? [
            part(
              "CLAUDE.local.md",
              { kind: "managed", name: "toolportctl" },
              `${cwd}/CLAUDE.local.md`,
              layers
                .map((layer) => `## ${layer.name}\n${layer.description}\n`)
                .join("\n"),
              { layers: layers.map((layer) => layer.name) },
            ),
          ]
        : []),
      ...layers.flatMap((layer) =>
        host.layerMeta(layer.name).imports.map((path) =>
          part(
            path.split("/").at(-1) ?? path,
            { kind: "loose", name: path },
            path,
            `# Imported text of ${path}\n`,
            {
              via: [layer.name],
            },
          ),
        ),
      ),
    ].filter((one) => !b?.layersExclude.some((glob) => globMatch(glob, one.path)));
    return {
      cwd,
      notes: [],
      parts,
      skipped: [],
      total: {
        basis: "estimate",
        value: parts.reduce((sum, one) => sum + one.tokens.value, 0),
      },
    };
  }

  const listRow = (b: Bundle) => ({
    agents: { off: b.agentsOff },
    appliedTo: appliedTo(b.name),
    bind: b.bind,
    description: b.description,
    issues: issuesOf(b).length,
    layers: { add: b.layersAdd, exclude: b.layersExclude },
    name: b.name,
    path: pathOf(b.name),
    plugins: { off: b.pluginsOff },
    servers: b.servers,
    skills: {
      allow: b.skillsAllow.length,
      nameOnly: b.skillsNameOnly.length,
      off: b.skillsOff.length,
    },
  });
  const showRow = (b: Bundle) => ({
    agents: { off: b.agentsOff },
    appliedTo: appliedTo(b.name),
    bind: b.bind,
    description: b.description,
    issues: issuesOf(b),
    layers: { add: b.layersAdd, exclude: b.layersExclude },
    legacy: b.legacy,
    name: b.name,
    path: pathOf(b.name),
    plugins: { off: b.pluginsOff },
    servers: b.servers,
    skills: { allow: b.skillsAllow, nameOnly: b.skillsNameOnly, off: b.skillsOff },
    yaml: yamlOf(b),
  });

  const edited = (b: Bundle, call: Call): Bundle => {
    const next = { ...b };
    const description = call.flags.get("--description");
    if (typeof description === "string") next.description = description;
    const servers = call.flags.get("--servers");
    if (typeof servers === "string") next.servers = servers || null;
    for (const [flag, key] of LISTS) {
      const value = call.flags.get(flag);
      if (typeof value === "string") (next[key] as string[]) = csv(value);
    }
    if (next.legacy && next.skillsAllow.length === 0) next.legacy = false;
    return next;
  };

  const save = (
    call: Call,
    create: boolean,
    raw: (cwd: string) => LoadsData,
  ): unknown => {
    const [name] = call.words;
    if (!name)
      return new Failure(
        "usage",
        "missing bundle name\nusage: context bundle add|edit <name> [--from-folder <dir>] [--description <t>] [--skills-off <csv>] [--skills-name-only <csv>] [--skills-allow <csv>] [--plugins-off <csv>] [--layers-add <csv>] [--layers-exclude <csv>] [--agents-off <csv>] [--servers <profile>] [--bind <csv>] [--dry-run]",
      );
    const old = bundles.find((b) => b.name === name);
    if (create) {
      if (!NAME.test(name))
        return new Failure("usage", `bundle name ${name} is not valid`);
      if (old)
        return new Failure(
          "conflict",
          `bundle ${name} exists already (${pathOf(name)}); use \`context bundle edit\``,
        );
    } else if (!old) {
      return new Failure("not_found", `unknown bundle: ${name}`);
    }
    let base: Bundle = old ?? {
      name,
      description: "",
      servers: null,
      skillsOff: [],
      skillsNameOnly: [],
      skillsAllow: [],
      pluginsOff: [],
      layersAdd: [],
      layersExclude: [],
      agentsOff: [],
      bind: [],
      legacy: false,
    };
    const from = call.flags.get("--from-folder");
    if (create && typeof from === "string") {
      const source = current(from);
      base = source
        ? { ...source, name, legacy: false, bind: [], servers: null }
        : { ...base, skillsOff: [], description: `Read from ${from}` };
      void raw;
    }
    const next = edited(base, call);
    const path = pathOf(name);
    const step = create
      ? {
          op: "create",
          path,
          detail: `write the new bundle ${name}`,
          diff: { before: "", after: yamlOf(next) },
        }
      : {
          op: "update",
          path,
          detail: `change bundle ${name}`,
          diff: { before: yamlOf(old!), after: yamlOf(next) },
        };
    const undo = create
      ? `toolportctl context bundle rm ${name}`
      : `git checkout -- ${path} (the file is yours to commit)`;
    const result = {
      applied: true,
      backups: [] as string[],
      changed: [path],
      undo,
    };
    const data = {
      created: create,
      dryRun: call.dry,
      issues: issuesOf(next),
      name,
      path,
      plan: plan(`${create ? "Add" : "Edit"} bundle ${name}`, [step], undo),
      result: call.dry ? null : result,
    };
    if (!call.dry)
      bundles = create
        ? [...bundles, next]
        : bundles.map((b) => (b.name === name ? next : b));
    return data;
  };

  function put(
    call: Call,
    name: string,
    cwd: string,
    raw: (cwd: string) => LoadsData,
  ): { b: Bundle; data: Record<string, unknown> } | Failure {
    const b = bundles.find((one) => one.name === name);
    if (!b) return unknown(name);
    const before = shapeLoads(raw(cwd), cwd, false).total_tokens;
    const previous = applied.find((one) => one.folder === cwd);
    const withIt = applied.filter((one) => one.folder !== cwd);
    const stash = applied;
    applied = [...withIt, { bundle: name, folder: cwd, appliedAt: AT, drift: false }];
    const after = shapeLoads(raw(cwd), cwd, false).total_tokens;
    applied = stash;
    const files = [
      settingsOf(cwd),
      ...(b.layersAdd.length ? [`${cwd}/CLAUDE.local.md`] : []),
      `${cwd}/.git/info/exclude`,
    ];
    const steps = [
      {
        op: "merge",
        path: settingsOf(cwd),
        detail: `${ownedKeys(b).skillOverrides.length + b.pluginsOff.length + b.layersExclude.length + b.agentsOff.length} key(s) owned by bundle ${name}`,
        keys: [
          "skillOverrides",
          "enabledPlugins",
          "claudeMdExcludes",
          "permissions.deny",
        ],
        diff: {
          before: "skillOverrides: (absent)\n",
          after: `${[...b.skillsOff.map((s) => `skillOverrides.${s}: "off"`), ...b.pluginsOff.map((p) => `enabledPlugins.${p}: false`)].join("\n")}\n`,
        },
      },
      ...(b.layersAdd.length
        ? [
            {
              op: "merge",
              path: `${cwd}/CLAUDE.local.md`,
              detail: `one managed block with ${b.layersAdd.length} layer(s)`,
            },
          ]
        : []),
      {
        op: "update",
        path: `${cwd}/.git/info/exclude`,
        detail: "git-ignore CLAUDE.local.md and settings.local.json",
      },
    ];
    const warnings = previous
      ? [`bundle ${previous.bundle} is applied here: its keys are put back first`]
      : [];
    const undo = `toolportctl context bundle undo --cwd ${cwd}`;
    const data = {
      conflicts: [] as string[],
      cwd,
      dryRun: call.dry,
      plan: plan(
        `Apply bundle ${name} in ${cwd}`,
        steps,
        undo,
        { before, after },
        warnings,
      ),
      result: call.dry
        ? null
        : { applied: true, backups: [] as string[], changed: files, ledger, undo },
    };
    return { b, data };
  }

  function settle(name: string, cwd: string) {
    applied = [
      ...applied.filter((one) => one.folder !== cwd),
      { bundle: name, folder: cwd, appliedAt: AT, drift: false },
    ];
  }

  function run(path: string, call: Call, raw: (cwd: string) => LoadsData): unknown {
    const [name] = call.words;
    const cwdFlag = call.flags.get("--cwd");
    const cwd = typeof cwdFlag === "string" ? cwdFlag : null;
    switch (path) {
      case "context compose":
        return compose(cwd ?? host.home);
      case "context measure":
        return measure(call, raw);
      case "context bundle ls":
        return { bundles: bundles.map(listRow), directory: dir };
      case "context bundle show": {
        const b = bundles.find((one) => one.name === name);
        return b ? showRow(b) : unknown(name ?? "");
      }
      case "context bundle add":
        return save(call, true, raw);
      case "context bundle edit":
        return save(call, false, raw);
      case "context bundle rm": {
        const b = bundles.find((one) => one.name === name);
        if (!b) return new Failure("not_found", `unknown bundle: ${name ?? ""}`);
        const where = applied.find((one) => one.bundle === b.name);
        if (where && !call.flags.has("--force"))
          return new Failure(
            "conflict",
            `bundle ${b.name} is applied in ${where.folder}; undo it there or pass --force`,
          );
        const undo = `git checkout -- ${pathOf(b.name)} (the file is yours to commit)`;
        const data = {
          dryRun: call.dry,
          name: b.name,
          path: pathOf(b.name),
          plan: plan(
            `Remove bundle ${b.name}`,
            [
              {
                op: "delete",
                path: pathOf(b.name),
                detail: `delete the bundle ${b.name}`,
                diff: { before: yamlOf(b), after: "" },
              },
            ],
            undo,
          ),
          result: call.dry
            ? null
            : { applied: true, backups: [] as string[], changed: [pathOf(b.name)], undo },
        };
        if (!call.dry) bundles = bundles.filter((one) => one !== b);
        return data;
      }
      case "context bundle apply": {
        if (!cwd || !name)
          return new Failure(
            "usage",
            "usage: context bundle apply <name> --cwd <dir> [--dry-run]",
          );
        const made = put(call, name, cwd, raw);
        if (made instanceof Failure) return made;
        if (!call.dry) settle(name, cwd);
        return { bundle: name, ...made.data };
      }
      case "context use": {
        if (!cwd)
          return new Failure(
            "usage",
            "usage: context use <name>|--none --cwd <dir> [--dry-run]",
          );
        if (call.flags.has("--none")) {
          const was = applied.find((one) => one.folder === cwd);
          const undo = was ? `toolportctl context use ${was.bundle} --cwd ${cwd}` : "";
          const data = {
            bundle: was?.bundle ?? null,
            conflicts: [] as string[],
            cwd,
            dryRun: call.dry,
            plan: plan(
              was ? `Stop using ${was.bundle} in ${cwd}` : `Nothing is applied in ${cwd}`,
              was
                ? [
                    {
                      op: "update",
                      path: settingsOf(cwd),
                      detail: `put back the keys of ${was.bundle}`,
                    },
                  ]
                : [],
              undo,
            ),
            result:
              call.dry || !was
                ? null
                : {
                    applied: true,
                    backups: [] as string[],
                    changed: [settingsOf(cwd)],
                    ledger,
                    undo,
                  },
            server: { unrouted: false },
          };
          if (!call.dry) applied = applied.filter((one) => one.folder !== cwd);
          return data;
        }
        if (!name)
          return new Failure(
            "usage",
            "usage: context use <name>|--none --cwd <dir> [--dry-run]",
          );
        const exists = bundles.find((one) => one.name === name);
        if (!exists)
          return new Failure(
            "not_found",
            `nothing is called ${name}: no bundle in the skills repo's profiles/ and no server profile`,
          );
        const made = put(call, name, cwd, raw);
        if (made instanceof Failure) return made;
        if (!call.dry) settle(name, cwd);
        const warnings = host.foldersEnabled()
          ? []
          : ["the server set is not bound: folder profiles are switched off"];
        return {
          bundle: name,
          bundlePart: true,
          name,
          server: {
            bound: false,
            foldersEnabled: host.foldersEnabled(),
            profile: exists.servers,
          },
          ...made.data,
          plan: { ...(made.data.plan as object), warnings },
        };
      }
      case "context bundle undo": {
        if (!cwd)
          return new Failure(
            "usage",
            "usage: context bundle undo --cwd <dir> [--dry-run]",
          );
        const was = applied.find((one) => one.folder === cwd);
        if (!was) return new Failure("conflict", `no bundle is applied in ${cwd}`);
        const undo = `toolportctl context bundle apply ${was.bundle} --cwd ${cwd}`;
        const data = {
          bundle: was.bundle,
          conflicts: was.drift
            ? [
                `skillOverrides.${bundles.find((b) => b.name === was.bundle)?.skillsOff[0] ?? "x"}`,
              ]
            : [],
          cwd,
          dryRun: call.dry,
          plan: plan(
            `Undo bundle ${was.bundle} in ${cwd}`,
            [
              {
                op: "merge",
                path: settingsOf(cwd),
                detail: `put back the keys of ${was.bundle}`,
              },
            ],
            undo,
          ),
          result: call.dry
            ? null
            : {
                applied: true,
                backups: [] as string[],
                changed: [settingsOf(cwd), `${cwd}/CLAUDE.local.md`],
                ledger,
                undo,
              },
        };
        if (!call.dry) applied = applied.filter((one) => one.folder !== cwd);
        return data;
      }
      case "context bundle status": {
        if (!cwd) return new Failure("usage", "usage: context bundle status --cwd <dir>");
        const was = applied.find((one) => one.folder === cwd);
        const b = bundles.find((one) => one.name === was?.bundle);
        return {
          applied:
            was && b
              ? {
                  appliedAt: was.appliedAt,
                  bundle: was.bundle,
                  changedKeys: was.drift
                    ? [`skillOverrides.${b.skillsOff[0] ?? "x"}`]
                    : [],
                  drift: was.drift,
                  ownedKeys: ownedKeys(b),
                }
              : null,
          conflicts: was?.drift && b ? [`skillOverrides.${b.skillsOff[0] ?? "x"}`] : [],
          folder: cwd,
        };
      }
      case "context bundle launch": {
        const b = bundles.find((one) => one.name === name);
        if (!b) return unknown(name ?? "");
        const file = `${host.base}/profiles/${b.name}.settings.json`;
        return {
          bundle: b.name,
          command: `claude --settings ${file}`,
          cwd,
          notes: b.layersAdd.length
            ? [
                `layers.add (${b.layersAdd.join(", ")}) rides CLAUDE.local.md, which --settings cannot carry; \`context bundle apply\` delivers it`,
              ]
            : [],
          settingsFile: file,
        };
      }
      case "context bundle config": {
        const flag = call.flags.get("--auto-apply");
        if (flag !== undefined) {
          if (flag !== "on" && flag !== "off")
            return new Failure("usage", "--auto-apply must be on or off");
          autoApply = flag === "on";
        }
        return { autoApply, file: `${host.base}/context.json` };
      }
      default:
        return undefined;
    }
  }

  return {
    run,
    shapeLoads,
    markDrift: (folder: string) => {
      applied = applied.map((one) =>
        one.folder === folder ? { ...one, drift: true } : one,
      );
    },
    snapshot: () => ({ bundles, applied, autoApply, measured: [...kept.keys()] }),
  };
}

export type TabsWorld = ReturnType<typeof createTabsWorld>;
