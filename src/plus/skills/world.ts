/** A stateful version of the Skills fixtures: the same argv rows, but an applied write changes
 * what the next read answers (a sync writes the outputs and clears the drift, a clean removes
 * them and the lockfile, an uninstall removes the skill, an install adds one, a resolve ends
 * the collision). The e2e tests and the dev browser fixture walk the screen through it; a
 * preview (`--dry-run`) never changes anything. Browser safe: no node imports. */
import {
  ALL_CLIENTS,
  CLIENTS,
  HOME,
  REPO,
  auditData,
  collision,
  libraryRows,
  lintData,
  sourceLs,
  sourcesFixture,
  statusData,
} from "./fixtures";
import {
  TAPS_ROOT,
  bundleData,
  emptySearch,
  initData,
  installData,
  searchData,
  tapAddData,
  tapLs,
  tapRemoveData,
  tapUpdateData,
  unbundleData,
} from "./fixturesTaps";
import type { SkillRow } from "./model";

/** A reply that is a failed envelope; `data` is what a command that exits 1 still prints. */
export class Failure {
  constructor(
    readonly code: string,
    readonly message: string,
    readonly data?: unknown,
  ) {}
}

export const PROJECT = "/fixture/proj";
export const NEW_REPO = "/fixture/new";
const DATA = "/fixture/data";
const STAMP = "2026-10-04T10:00:00Z";
const ADDABLE = ["reviewer"];
const INSTALLABLE = ["code-review", "risky"];
const TAP_NAMES = ["acme-skills", "local-notes", "tools"];

interface Item {
  name: string;
  type: string;
}

export interface WorldOptions {
  /** False: a fresh Mac with no skills repository until `skills init` is applied. */
  repo?: boolean;
  /** The dev browser has no exit codes: a check that exits 1 is shown as its data. */
  browser?: boolean;
}

const subsets = (list: string[]): string[][] =>
  list
    .reduce<string[][]>((all, one) => [...all, ...all.map((s) => [...s, one])], [[]])
    .slice(1);

const outputFile = (name: string, client: string, root = HOME) =>
  `${root}/.${client}/skills/${name}/SKILL.md`;

export function createSkillsWorld(
  options: WorldOptions = {},
): Map<string, () => unknown> {
  const fresh = options.repo === false;
  const seed: Item[] = fresh
    ? []
    : libraryRows.map((r) => ({ name: r.name, type: r.type }));
  const s = {
    repo: !fresh,
    items: seed,
    lock: fresh ? null : (CLIENTS as string[] | null),
    known: new Set(seed.map((i) => i.name).filter((n) => n !== "feature-spec")),
    present: new Set<string>(),
    drifted: new Set(fresh ? [] : ["deploy-helper"]),
    collision: !fresh,
    taps: [...tapLs.taps],
  };
  for (const name of s.known)
    for (const client of CLIENTS)
      if (!(name === "api-review" && client === "cursor"))
        s.present.add(`${name}/${client}`);

  const rows = new Map<string, () => unknown>();
  const on = (argv: string, reply: () => unknown) => rows.set(argv, reply);
  const both = (argv: string, preview: () => unknown, apply: () => unknown) => {
    on(`${argv} --dry-run`, preview);
    on(argv, apply);
  };
  const noRepo = () => new Failure("skills", `no skills repository at ${NEW_REPO}`);
  const guarded =
    <T>(reply: () => T) =>
    () =>
      s.repo ? reply() : noRepo();
  const exits = <T extends { clean?: boolean }>(data: T, code = "unhealthy") =>
    options.browser || data.clean !== false
      ? data
      : new Failure(code, "one or more checks failed", data);

  const has = (name: string) => s.items.some((i) => i.name === name);
  const typeOf = (name: string) => s.items.find((i) => i.name === name)?.type ?? "skill";
  const rowOf = (item: Item): SkillRow => {
    const known = libraryRows.find((r) => r.name === item.name);
    if (known) return known;
    return {
      activation: item.type === "rule" ? "always" : "auto",
      description: `Use when you need to ${item.name.replace(/-/g, " ")}`,
      invisibleReason: null,
      name: item.name,
      origin: libraryRows[0].origin,
      path: `${REPO}/${item.type === "rule" ? "rules" : "skills"}/${item.name}/SKILL.md`,
      type: item.type,
      visible: true,
      writable: true,
    };
  };
  const counts = () => ({
    rules: s.items.filter((i) => i.type === "rule").length,
    skills: s.items.filter((i) => i.type !== "rule").length,
  });
  const warningsOf = (name: string, clients: string[]) =>
    name === "deploy-helper" && clients.includes("cursor")
      ? ["cursor: 'allowed-tools' field not supported, dropped"]
      : [];
  const lockClients = () => s.lock ?? ["claude-code"];

  const syncOf = (clients: string[], dryRun: boolean, scope = "") => ({
    backupRoot: `${HOME}/.mcpm-backups`,
    cleaned: [],
    clientCount: clients.length,
    clientSource: s.lock ? "lock" : "default",
    collisions: s.collision ? [collision] : [],
    dryRun,
    entries: s.items.map(({ name, type }) => ({
      clientsSynced: clients,
      name,
      type,
      warnings: warningsOf(name, clients),
    })),
    globalMode: scope === "",
    kept: s.collision ? 1 : 0,
    outputRoot: scope === "" ? HOME : PROJECT,
    replaced: 0,
    repo: REPO,
    ruleCount: counts().rules,
    skillCount: counts().skills,
    syncedAt: STAMP,
    targetedClients: clients,
  });
  const applySync = (clients: string[]) => {
    s.lock = clients;
    s.known = new Set(s.items.map((i) => i.name));
    s.drifted.clear();
    s.present.clear();
    for (const { name } of s.items)
      for (const client of clients) s.present.add(`${name}/${client}`);
  };

  const outputsOf = (name?: string) =>
    [...s.present]
      .map((key) => key.split("/"))
      .filter(([n]) => !name || n === name)
      .map(([n, client]) => outputFile(n, client));
  const cleanOf = (dryRun: boolean, scope = "") => ({
    cleanRoot: scope === "" ? HOME : PROJECT,
    dryRun,
    ignored: [],
    lockDir: DATA,
    lockfilePresent: s.lock !== null,
    lockfileRemoved: s.lock !== null && !dryRun,
    managed: [...s.known],
    removed:
      scope === "" ? outputsOf() : [outputFile("api-review", "claude-code", PROJECT)],
    scope: scope === "" ? "global" : "project",
    skipped: [],
  });
  const uninstallOf = (name: string, dryRun: boolean, scope = "") => ({
    dryRun,
    lockDir: DATA,
    lockUpdated: s.known.has(name),
    name,
    outputRoot: scope === "" ? HOME : PROJECT,
    outputs: scope === "" ? outputsOf(name) : [outputFile(name, "claude-code", PROJECT)],
    repo: REPO,
    scope: scope === "" ? "global" : "project",
    sourcePath: `${REPO}/${typeOf(name) === "rule" ? "rules" : "skills"}/${name}`,
  });
  const resolveOf = (migrate: boolean, dryRun: boolean, scope = "") => {
    const found = s.collision;
    return {
      backupRoot: `${HOME}/.mcpm-backups`,
      collisions: found
        ? [
            migrate
              ? {
                  ...collision,
                  action: "replaced",
                  backupPath: `${HOME}/.mcpm-backups/.claude/commands/release-notes.md.1`,
                }
              : collision,
          ]
        : [],
      dryRun,
      kept: found && !migrate ? 1 : 0,
      migrate: migrate ? true : null,
      outputRoot: scope === "" ? HOME : PROJECT,
      replaced: found && migrate ? 1 : 0,
      repo: REPO,
      scope: scope === "" ? "global" : "project",
      skillCount: counts().skills,
    };
  };
  const addOf = (name: string, type: string, progressive: boolean, dryRun: boolean) => {
    const folder = type === "rule" ? "rules" : "skills";
    return {
      dryRun,
      files: [
        `${REPO}/${folder}/${name}/SKILL.md`,
        ...(progressive ? [`${REPO}/${folder}/${name}/reference.md`] : []),
      ],
      name,
      path: `${REPO}/${folder}/${name}/SKILL.md`,
      progressive,
      repo: REPO,
      type,
    };
  };

  on("skills ls", () =>
    s.repo
      ? {
          repo: REPO,
          skills: s.items.map(rowOf).sort((a, b) => a.name.localeCompare(b.name)),
        }
      : noRepo(),
  );
  on("sources ls", () => sourcesFixture);
  for (const source of sourcesFixture.sources) {
    const ls = sourceLs(source.id);
    if (source.id !== "library" && source.counts.skill > 0)
      on(`skills ls --source ${source.id}`, () => ls);
  }
  on(
    "skills status",
    guarded(() => {
      const lock = s.lock;
      const known = s.items.filter((i) => s.known.has(i.name));
      const outputs = lock
        ? known.flatMap(({ name }) =>
            lock.map((client) => ({
              client,
              name,
              present: s.present.has(`${name}/${client}`),
            })),
          )
        : [];
      return {
        ...statusData,
        drift: lock !== null && (s.drifted.size > 0 || outputs.some((o) => !o.present)),
        entries: s.items.map(({ name, type }) => ({
          clientsSynced: lock && s.known.has(name) ? lock : [],
          currentHash: `sha256:${name}`,
          drifted: s.drifted.has(name),
          knownToLockfile: lock !== null && s.known.has(name),
          lockfileHash:
            lock && s.known.has(name)
              ? s.drifted.has(name)
                ? "sha256:old"
                : `sha256:${name}`
              : null,
          name,
          type,
        })),
        lockedCount: lock ? known.length : 0,
        lockfilePresent: lock !== null,
        lockfileSyncedAt: lock ? "2026-10-03T09:00:00Z" : null,
        outputRoot: lock ? HOME : null,
        outputs,
        targetedClients: lock ?? ALL_CLIENTS,
      };
    }),
  );
  on(
    "skills lint",
    guarded(() => ({
      ...lintData,
      messages: lintData.messages.filter((m) => has(m.name)),
    })),
  );
  for (const name of [...s.items.map((i) => i.name), ...ADDABLE, ...INSTALLABLE])
    on(
      `skills lint --name ${name}`,
      guarded(() => ({
        ...lintData,
        messages: lintData.messages.filter((m) => m.name === name),
      })),
    );
  on(
    "skills audit",
    guarded(() => ({
      ...auditData,
      skillCount: counts().skills,
      findings: (auditData.findings as Array<{ skill: string }>).filter((f) =>
        has(f.skill),
      ),
    })),
  );
  on(
    "skills diff",
    guarded(() => {
      const fresh = s.lock ? s.items.filter((i) => !s.known.has(i.name)) : s.items;
      const modified = s.lock ? [...s.drifted].filter((n) => has(n)) : [];
      return exits({
        clean: fresh.length === 0 && modified.length === 0,
        modified,
        new: fresh.map((i) => i.name),
        noLockfile: s.lock === null,
        removed: [],
        repo: REPO,
        unchanged: s.lock ? s.known.size - modified.length : 0,
      });
    }),
  );
  on(
    "skills sync --dry-run",
    guarded(() => syncOf(lockClients(), true)),
  );
  on(
    "skills resolve --dry-run",
    guarded(() => resolveOf(false, true)),
  );

  for (const clients of subsets(CLIENTS)) {
    const flags = clients.map((c) => `--client ${c}`).join(" ");
    both(
      `skills sync ${flags}`,
      () => syncOf(clients, true),
      () => {
        applySync(clients);
        return syncOf(clients, false);
      },
    );
    for (const scope of ["--project", `--project --repo ${PROJECT}`])
      both(
        `skills sync ${scope} ${flags}`,
        () => syncOf(clients, true, scope),
        () => syncOf(clients, false, scope),
      );
  }
  both(
    "skills clean",
    () => cleanOf(true),
    () => {
      const done = cleanOf(false);
      s.lock = null;
      s.known.clear();
      s.present.clear();
      s.drifted.clear();
      return done;
    },
  );
  both(
    "skills resolve --migrate",
    () => resolveOf(true, true),
    () => {
      const done = resolveOf(true, false);
      s.collision = false;
      return done;
    },
  );
  for (const scope of ["--project", `--project --repo ${PROJECT}`]) {
    both(
      `skills clean ${scope}`,
      () => cleanOf(true, scope),
      () => cleanOf(false, scope),
    );
    both(
      `skills resolve --migrate ${scope}`,
      () => resolveOf(true, true, scope),
      () => resolveOf(true, false, scope),
    );
  }
  for (const name of [...s.items.map((i) => i.name), ...ADDABLE, ...INSTALLABLE]) {
    both(
      `skills uninstall ${name}`,
      () => uninstallOf(name, true),
      () => {
        const done = uninstallOf(name, false);
        s.items = s.items.filter((i) => i.name !== name);
        s.known.delete(name);
        s.drifted.delete(name);
        for (const client of ALL_CLIENTS) s.present.delete(`${name}/${client}`);
        return done;
      },
    );
    for (const scope of ["--project", `--project --repo ${PROJECT}`])
      both(
        `skills uninstall ${name} ${scope}`,
        () => uninstallOf(name, true, scope),
        () => uninstallOf(name, false, scope),
      );
  }
  for (const name of ADDABLE)
    for (const type of ["skill", "rule"])
      for (const progressive of [false, true])
        both(
          `skills add ${name} --type ${type}${progressive ? " --with-progressive" : ""}`,
          () => addOf(name, type, progressive, true),
          () => {
            if (!has(name)) s.items = [...s.items, { name, type }];
            return addOf(name, type, progressive, false);
          },
        );
  both(
    `skills init --path ${NEW_REPO} --name team`,
    () => initData(true),
    () => {
      s.repo = true;
      return initData(false);
    },
  );

  on("skills tap ls", () => ({ taps: s.taps, tapsRoot: TAPS_ROOT }));
  on("skills search review", () => ({
    ...searchData,
    tapCount: s.taps.length,
    results: searchData.results.filter((r) => s.taps.some((t) => t.name === r.tap)),
  }));
  on("skills search nothing", () => emptySearch("nothing"));
  both(
    "skills tap add acme/tools --name tools",
    () => tapAddData(true),
    () => {
      if (!s.taps.some((t) => t.name === "tools"))
        s.taps = [
          ...s.taps,
          {
            cloned: true,
            name: "tools",
            path: `${TAPS_ROOT}/tools`,
            repo: "acme/tools",
            url: "https://github.com/acme/tools.git",
          },
        ];
      return tapAddData(false);
    },
  );
  both(
    "skills tap update",
    () => tapUpdateData(true),
    () => tapUpdateData(false),
  );
  for (const name of TAP_NAMES) {
    const one = (dryRun: boolean) => ({
      ...tapUpdateData(dryRun),
      results: tapUpdateData(dryRun).results.filter((r) => r.name === name),
    });
    both(
      `skills tap update ${name}`,
      () => one(true),
      () => one(false),
    );
    const removal = (dryRun: boolean) => ({
      ...tapRemoveData(dryRun),
      name,
      path: `${TAPS_ROOT}/${name}`,
    });
    both(
      `skills tap remove ${name}`,
      () => removal(true),
      () => {
        s.taps = s.taps.filter((t) => t.name !== name);
        return removal(false);
      },
    );
  }

  const spec = (name: string) => `@acme/skills/${name}`;
  both(
    `skills install ${spec("code-review")}`,
    () => installData(spec("code-review"), { dryRun: true }),
    () => {
      if (!has("code-review"))
        s.items = [...s.items, { name: "code-review", type: "skill" }];
      return installData(spec("code-review"), { dryRun: false });
    },
  );
  both(
    "skills install @acme/risky",
    () => installData("@acme/risky", { dryRun: true, blocked: true }),
    () => installData("@acme/risky", { dryRun: false, blocked: true }),
  );
  both(
    "skills install @acme/risky --no-audit",
    () => installData("@acme/risky", { dryRun: true, audit: false }),
    () => {
      if (!has("risky")) s.items = [...s.items, { name: "risky", type: "skill" }];
      return installData("@acme/risky", { dryRun: false, audit: false });
    },
  );
  both(
    "skills bundle --skills api-review,deploy-helper --output /fixture/out/team.zip",
    () => bundleData(true),
    () => bundleData(false),
  );
  both(
    "skills unbundle /fixture/in/team.zip --path /fixture/fresh",
    () => unbundleData(true),
    () => unbundleData(false),
  );
  return rows;
}
