/** A stateful System world: the argv the five tabs run, answered like `toolportctl` answers
 * them, but an applied write changes what the next read says (an init configures sync, a push
 * moves the bundle, an update moves a server to its latest version, an install changes the
 * council doctor). A preview (`--dry-run`) never changes anything. Browser safe: JSON imports
 * only, no node modules. All names are made up. */
import { ctlReplyFailure } from "../fixtures/ctlReply";
import councilTools from "../../../src-tauri/tests/fixtures/ctl-envelopes/council-tools.json";
import mcpTools from "../../../src-tauri/tests/fixtures/ctl-envelopes/mcp-tools.json";
import nameMap from "../../../src-tauri/tests/fixtures/ctl-envelopes/import-mcpm.name-map.json";

type Golden = { envelope: { data: unknown } };
type Loose = Record<string, unknown>;
const data = (golden: unknown) => (golden as Golden).envelope.data;

const STAMP = "2026-10-04T10:00:00Z";
const BINARY = "/fixture/bin/toolport-selfmcp";
export const COUNCIL_KEY = "OPENROUTER_API_KEY";
export const SELF_ID = "toolport-plus-self";

export interface SyncState {
  configured: boolean;
  repo: string | null;
  branch: string;
  machineId: string;
  lastSyncAt: string;
  lastDirection: string;
  /** This machine's files and their versions. */
  local: Record<string, string>;
  /** The files of the bundle on the remote; null while nothing was pushed to it yet. */
  remote: Record<string, string> | null;
  /** The version of each file at the last sync. */
  synced: Record<string, string>;
  projects: Record<string, { local_path: string; files: string[] }>;
}

export interface GitState {
  repo: string | null;
  branch: string | null;
  auto: boolean;
}

export interface UpdateServerState {
  id: string;
  kind: string;
  status: string;
  message: string;
  detected: boolean;
  current?: string;
  latest?: string;
  behind?: number;
  hook?: string;
}

export interface SystemState {
  sync: SyncState;
  git: GitState;
  updates: UpdateServerState[];
  council: { installed: boolean; keyStored: boolean };
  self: { state: "missing" | "enabled" | "opted-out"; profiles: string[] };
  mcpm: { imported: boolean; rewritten: boolean };
}

export const SERVER_IDS = {
  git: "srv-git",
  release: "srv-release",
  npx: "srv-npx",
  uvx: "srv-uvx",
  unknown: "srv-new",
};

const freshUpdates = (): UpdateServerState[] => [
  {
    id: SERVER_IDS.git,
    kind: "git",
    status: "update-available",
    message: "2 commit(s) behind origin/main",
    detected: true,
    current: "a1b2c3d4e5f6",
    latest: "f6e5d4c3b2a1",
    behind: 2,
    hook: "./build.sh",
  },
  {
    id: SERVER_IDS.release,
    kind: "github-release",
    status: "update-available",
    message: "v1.4.0 is available",
    detected: true,
    current: "v1.3.0",
    latest: "v1.4.0",
  },
  {
    id: SERVER_IDS.npx,
    kind: "npx",
    status: "auto",
    message: "follows the latest version on each start",
    detected: true,
  },
  {
    id: SERVER_IDS.uvx,
    kind: "uvx",
    status: "auto",
    message: "follows the latest version on each start",
    detected: true,
  },
  {
    id: SERVER_IDS.unknown,
    kind: "unknown",
    status: "skipped",
    message: "unknown source; run update --init",
    detected: false,
  },
];

/** A machine that has not set anything up: no sync, no council, no self-management server, an
 * mcpm folder that was not imported, and five servers that cover every update source. */
export function freshState(): SystemState {
  return {
    sync: {
      configured: false,
      repo: null,
      branch: "main",
      machineId: "",
      lastSyncAt: "",
      lastDirection: "",
      local: {
        "registry.json": "v1",
        "profiles.json": "v1",
        "skills/review/SKILL.md": "v1",
      },
      remote: null,
      synced: {},
      projects: {},
    },
    git: { repo: null, branch: null, auto: false },
    updates: freshUpdates(),
    council: { installed: false, keyStored: false },
    self: { state: "missing", profiles: [] },
    mcpm: { imported: false, rewritten: false },
  };
}

const flag = (argv: string[], name: string) => {
  const at = argv.indexOf(name);
  return at >= 0 ? argv[at + 1] : undefined;
};
const has = (argv: string[], name: string) => argv.includes(name);
const fail = (code: string, message: string, extra?: unknown) =>
  ctlReplyFailure(code, message, extra);
const needsSecret = (argv: string[], stdin: string | null | undefined) =>
  !has(argv, "--passphrase-stdin") || !stdin
    ? fail("sync", "a passphrase is required on stdin")
    : null;
const isProject = (path: string) => path.startsWith("projects/");
const unconfigured = () => fail("sync", "sync is not configured; run init first");

export function createSystemWorld(initial: Partial<SystemState> = {}) {
  const s: SystemState = { ...freshState(), ...initial };
  const sync = () => s.sync;

  const entriesFor = (argv: string[]) =>
    Object.keys(sync().local)
      .filter((path) => has(argv, "--include-projects") || !isProject(path))
      .sort();

  function changes() {
    const { local, remote, synced } = sync();
    const out = {
      new: [] as string[],
      modified: [] as string[],
      removed: [] as string[],
      conflicts: [] as string[],
      unchanged: [] as string[],
    };
    for (const path of Object.keys(remote ?? {}).sort()) {
      const theirs = remote![path];
      if (!(path in local)) out.new.push(path);
      else if (local[path] === theirs) out.unchanged.push(path);
      else if (local[path] === synced[path]) out.modified.push(path);
      else out.conflicts.push(path);
    }
    for (const path of Object.keys(synced).sort()) {
      if (remote && !(path in remote)) out.removed.push(path);
    }
    return out;
  }

  const statusData = () => ({
    backend: sync().configured ? "git" : null,
    branch: sync().configured ? sync().branch : null,
    configured: sync().configured,
    keyfilePresent: sync().configured,
    lastDirection: sync().lastDirection,
    lastSyncAt: sync().lastSyncAt,
    machineId: sync().configured ? sync().machineId : null,
    projects: sync().projects,
    repoUrl: sync().repo,
    tracked: Object.keys(sync().synced).length,
  });

  function push(argv: string[]) {
    if (!sync().configured) return unconfigured();
    const dry = has(argv, "--dry-run");
    const entries = entriesFor(argv);
    if (!dry) {
      const remote = { ...(sync().remote ?? {}) };
      for (const path of entries) remote[path] = sync().local[path];
      sync().remote = remote;
      for (const path of entries) sync().synced[path] = sync().local[path];
      sync().lastSyncAt = STAMP;
      sync().lastDirection = "push";
    }
    return {
      committed: !dry,
      dryRun: dry,
      entries,
      machineId: sync().machineId,
      pushed: !dry,
    };
  }

  function pull(argv: string[]) {
    if (!sync().configured) return unconfigured();
    const dry = has(argv, "--dry-run");
    const noRemote = sync().remote === null;
    const found = changes();
    const wanted = (path: string) => has(argv, "--include-projects") || !isProject(path);
    const force = has(argv, "--force");
    const applied = [...found.new, ...found.modified, ...(force ? found.conflicts : [])]
      .filter(wanted)
      .sort();
    const kept = force ? [] : found.conflicts.filter(wanted);
    const out = {
      applied: dry ? [] : applied,
      changes: dry ? found : null,
      conflicts: dry ? found.conflicts : kept,
      dryRun: dry,
      keptLocal: dry ? [] : kept,
      machineId: sync().machineId,
      noRemote,
      pushedAt: STAMP,
      resolved: [],
      skillsRepoChanged: 0,
      skipped: [],
    };
    if (!dry && !noRemote) {
      for (const path of applied) {
        sync().local[path] = sync().remote![path];
        sync().synced[path] = sync().remote![path];
      }
      for (const path of found.removed) {
        delete sync().synced[path];
        delete sync().local[path];
      }
      sync().lastSyncAt = STAMP;
      sync().lastDirection = "pull";
    }
    return out;
  }

  function init(argv: string[], stdin: string | null | undefined) {
    const secret = needsSecret(argv, stdin);
    if (secret) return secret;
    const repo = flag(argv, "--repo");
    if (!repo) return fail("usage", "sync init needs --repo");
    if (sync().configured && !has(argv, "--reconfigure")) {
      return fail("sync", "sync is already configured; pass --reconfigure to replace it");
    }
    Object.assign(sync(), {
      configured: true,
      repo,
      branch: flag(argv, "--branch") ?? "main",
      machineId: flag(argv, "--machine-id") ?? "this-machine",
    });
    return {
      branch: sync().branch,
      freshRemote: sync().remote === null,
      machineId: sync().machineId,
    };
  }

  function addProject(argv: string[]) {
    if (!sync().configured) return unconfigured();
    const path = argv[2];
    const name = flag(argv, "--name") ?? path.split("/").pop() ?? "project";
    const files = (flag(argv, "--files") ?? "CLAUDE.md").split(",").filter(Boolean);
    const replaced = name in sync().projects;
    sync().projects[name] = { local_path: path, files };
    for (const file of files) sync().local[`projects/${name}/${file}`] = "v1";
    return { name, replaced };
  }

  function removeProject(argv: string[]) {
    const name = argv[2];
    const found = name in sync().projects;
    for (const path of Object.keys(sync().local)) {
      if (path.startsWith(`projects/${name}/`)) delete sync().local[path];
    }
    delete sync().projects[name];
    return { name, removed: found };
  }

  const gitData = (extra: Loose = {}) => ({
    autoSync: s.git.auto,
    branch: s.git.branch,
    cleared: false,
    cloned: false,
    configured: s.git.repo !== null,
    head: null,
    localPath: s.git.repo ? "/fixture/data/skills_repo" : null,
    pulled: false,
    repo: s.git.repo,
    ...extra,
  });

  function gitSync(argv: string[]) {
    if (has(argv, "--status")) return gitData();
    if (has(argv, "--clear")) {
      s.git = { repo: null, branch: null, auto: false };
      return gitData({ cleared: true });
    }
    const repo = flag(argv, "--repo");
    if (!repo) return fail("usage", "sync git-sync needs --repo, --status or --clear");
    s.git = { repo, branch: flag(argv, "--branch") ?? null, auto: has(argv, "--auto") };
    return gitData({ cloned: true, pulled: true, head: "a1b2c3d" });
  }

  function syncReply(argv: string[], stdin: string | null | undefined) {
    switch (argv[1]) {
      case "status":
        return statusData();
      case "diff":
        if (!sync().configured) return unconfigured();
        return {
          changes: sync().remote ? changes() : changesEmpty(),
          machineId: sync().remote ? sync().machineId : "",
          noRemote: sync().remote === null,
        };
      case "init":
        return init(argv, stdin);
      case "push":
        return push(argv);
      case "pull":
        return pull(argv);
      case "reset": {
        const was = sync().configured;
        Object.assign(sync(), {
          configured: false,
          repo: null,
          machineId: "",
          lastSyncAt: "",
          lastDirection: "",
          synced: {},
          projects: {},
        });
        return {
          removed: was
            ? ["/fixture/data/sync/sync.json", "/fixture/data/sync/sync_keyfile"]
            : [],
        };
      }
      case "rotate-passphrase": {
        const secret = needsSecret(argv, stdin);
        if (secret) return secret;
        if (!sync().configured) return unconfigured();
        return { rotated: Object.keys(sync().remote ?? {}).length, skipped: [] };
      }
      case "add-project":
        return addProject(argv);
      case "remove-project":
        return removeProject(argv);
      case "git-sync":
        return gitSync(argv);
      case "migrate": {
        const secret = needsSecret(argv, stdin);
        if (secret) return secret;
        return fail("sync", `io error: ${argv[2]}/sync_manifest.json: No such file`);
      }
      default:
        return undefined;
    }
  }

  // ---- updates ------------------------------------------------------------------------

  const hookLine = (server: UpdateServerState, allowed: boolean) =>
    server.hook
      ? `post_update: ${server.hook}${allowed ? "" : " (not run without --allow-commands)"}`
      : null;

  const updatePlan = (server: UpdateServerState, allowed: boolean) => {
    if (server.status !== "update-available") return [];
    const hook = hookLine(server, allowed);
    return server.kind === "git"
      ? ["git merge --ff-only origin/main", ...(hook ? [hook] : [])]
      : [`download the ${server.latest} release asset`, "verify its checksum"];
  };

  const updateView = (server: UpdateServerState, allowed = false, extra: Loose = {}) => ({
    id: server.id,
    kind: server.kind,
    status: server.status,
    message: server.message,
    detected: server.detected,
    ...(server.current ? { current: server.current } : {}),
    ...(server.latest ? { latest: server.latest } : {}),
    ...(server.behind ? { behind: server.behind } : {}),
    ...(extra.plan === undefined && updatePlan(server, allowed).length
      ? { plan: updatePlan(server, allowed) }
      : {}),
    ...extra,
  });

  const report = (mode: string, servers: unknown[]) => {
    const counts: Record<string, number> = {};
    for (const server of servers as Array<{ status: string }>) {
      counts[server.status] = (counts[server.status] ?? 0) + 1;
    }
    return { counts, mode, servers };
  };

  function updateReply(argv: string[]) {
    const dry = has(argv, "--dry-run");
    const only = argv[1] && !argv[1].startsWith("--") ? argv[1] : null;
    const targets = s.updates.filter((server) => !only || server.id === only);
    if (only && targets.length === 0) return fail("update", `unknown server ${only}`);
    const allowed = has(argv, "--allow-commands");

    if (has(argv, "--init")) {
      const force = has(argv, "--force");
      const open = targets.filter((server) => force || !server.detected);
      const views = open.map((server) =>
        updateView(server, false, {
          status: "configured",
          message: `stored source github-release ${flag(argv, "--repo") ?? "acme/srv-new"}`,
          plan: [],
        }),
      );
      if (!dry) {
        for (const server of open) {
          Object.assign(server, {
            kind: "github-release",
            status: "up-to-date",
            message: "up to date with v2.0.0",
            detected: true,
            current: "v2.0.0",
            latest: "v2.0.0",
          });
        }
      }
      return report(dry ? "dry-run" : "init", views);
    }

    if (has(argv, "--apply")) {
      const views = targets.map((server) => {
        if (server.status !== "update-available") return updateView(server);
        if (dry) return updateView(server, allowed);
        const steps = [
          {
            name: server.kind === "git" ? "git merge" : "install release",
            ok: true,
            detail: `moved to ${server.latest}`,
          },
          ...(server.hook && allowed
            ? [{ name: "post_update", ok: true, detail: `${server.hook} exited 0` }]
            : []),
        ];
        return updateView(server, allowed, {
          status: "updated",
          message: `updated to ${server.latest}`,
          plan: server.hook && !allowed ? [hookLine(server, false)] : [],
          steps,
        });
      });
      if (!dry) {
        for (const server of targets) {
          if (server.status !== "update-available") continue;
          Object.assign(server, {
            status: "up-to-date",
            message: `up to date with ${server.latest}`,
            current: server.latest,
            behind: 0,
          });
        }
      }
      return report(dry ? "dry-run" : "apply", views);
    }

    return report(
      "check",
      targets.map((server) => updateView(server)),
    );
  }

  // ---- council and self-management ----------------------------------------------------

  const check = (name: string, ok: boolean, detail: string) => ({ detail, name, ok });
  const doctorReply = (command: string, checks: unknown[], extra: Loose = {}) => {
    const out = { ...extra, checks };
    const bad = (checks as Array<{ ok: boolean }>).filter((c) => !c.ok).length;
    return bad === 0 ? out : fail(command, `${bad} check(s) failed`, out);
  };

  function councilReply(argv: string[]) {
    const c = s.council;
    switch (argv[1]) {
      case "doctor":
        return doctorReply("council", [
          check(
            "registry_entry",
            c.installed,
            c.installed ? "council" : "run `toolportctl council install`",
          ),
          check("launcher_on_path", true, "uvx"),
          check("enabled_in_active_profile", c.installed, "default"),
          check("key_declared_secret", c.installed, COUNCIL_KEY),
          check("key_in_vault", c.keyStored, `council::${COUNCIL_KEY}`),
        ]);
      case "tools":
        return data(councilTools);
      case "install": {
        const created = !c.installed;
        c.installed = true;
        return { created, id: "council", keyStored: c.keyStored };
      }
      case "uninstall": {
        if (!c.installed) return { id: null, keyPurged: false, removed: false };
        const purged = has(argv, "--purge-key") && c.keyStored;
        c.installed = false;
        if (purged) c.keyStored = false;
        return { id: "council", keyPurged: purged, removed: true };
      }
      default:
        return undefined;
    }
  }

  const profileState = (id: string) => ({
    id,
    enabled: s.self.profiles.includes(id),
    optedOut: s.self.state === "opted-out",
  });

  function mcpReply(argv: string[]) {
    const self = s.self;
    switch (argv[1]) {
      case "doctor": {
        const installed = self.state === "enabled";
        return doctorReply(
          "mcp",
          [
            check(
              "registry_entry",
              installed,
              installed ? SELF_ID : "run `toolportctl mcp install`",
            ),
            check("command_matches_binary", installed, BINARY),
            check("binary_present", true, BINARY),
            check(
              "enabled_in_active_profile",
              self.profiles.includes("default"),
              self.profiles.includes("default") ? "default" : "-",
            ),
            check("enabled_in_client_profiles", true, "no client is scoped to a profile"),
            check("handshake", true, SELF_ID),
            check("catalog", true, "97 tools, 11 resources"),
          ],
          {
            activeProfile: installed ? profileState("default") : null,
            clientProfiles: self.profiles
              .filter((id) => id !== "default")
              .map(profileState),
            state: self.state,
          },
        );
      }
      case "tools":
        return data(mcpTools);
      case "install": {
        const profile = flag(argv, "--profile") ?? null;
        const action = self.state === "enabled" ? "unchanged" : "created";
        self.state = "enabled";
        const id = profile ?? "default";
        if (!self.profiles.includes(id)) self.profiles.push(id);
        return {
          action,
          command: BINARY,
          enabled: [...self.profiles],
          id: SELF_ID,
          profile,
        };
      }
      case "uninstall": {
        const removed = self.state === "enabled";
        self.state = "opted-out";
        self.profiles = [];
        return { id: SELF_ID, removed };
      }
      default:
        return undefined;
    }
  }

  // ---- import -------------------------------------------------------------------------

  const REJECT = { id: "gamma-mock", reason: "its launcher is not on the path" };

  function importReply(argv: string[]) {
    if (argv[1] === "mcpm") {
      if (has(argv, "--name-map")) return data(nameMap);
      const dry = has(argv, "--dry-run");
      const action = s.mcpm.imported ? "unchanged" : "created";
      const out = {
        clientDiscovery: [],
        clientScopes: [],
        clients: [],
        counts: { [action]: 3 },
        dryRun: dry,
        profiles: [{ action, id: "smoke" }],
        rejects: [REJECT],
        scripts: [],
        secrets: [],
        servers: [
          { action, id: "alpha-mock" },
          { action, id: "beta-mock" },
        ],
        skillsSync: [],
        skippedClients: [],
        warnings: [],
      };
      if (!dry) s.mcpm.imported = true;
      return out;
    }
    if (argv[1] === "rename-refs") {
      const dry = has(argv, "--dry-run");
      const at = argv.indexOf("--paths");
      const paths = (at >= 0 ? argv.slice(at + 1) : []).filter(
        (item) => !item.startsWith("--"),
      );
      const files = s.mcpm.rewritten
        ? []
        : paths.map((path) => ({ path: `${path}/note.md`, replaced: 2 }));
      const out = {
        dead: [],
        dryRun: dry,
        files,
        orphans: paths.length
          ? [
              {
                path: `${paths[0]}/rules.json`,
                reference: "mcp__mcpm_acme-erp__*",
                reason: "the wildcard cuts the server name",
                rule: "ask",
              },
            ]
          : [],
        replaced: files.length * 2,
        scanned: paths.length + 1,
      };
      if (!dry) s.mcpm.rewritten = true;
      return out;
    }
    return undefined;
  }

  const changesEmpty = () => ({
    new: [],
    modified: [],
    removed: [],
    conflicts: [],
    unchanged: [],
  });

  /** The reply to an argv, or undefined when the world does not run it. `stdin` is what the
   * run was given on its standard input: a passphrase or a key, never kept. */
  function reply(argv: string[], stdin?: string | null): unknown {
    switch (argv[0]) {
      case "sync":
        return syncReply(argv, stdin);
      case "update":
        return updateReply(argv);
      case "council":
        return councilReply(argv);
      case "mcp":
        return mcpReply(argv);
      case "import":
        return importReply(argv);
      case "secret":
        if (argv[1] === "set" && argv[2] === "council") {
          if (!stdin) return fail("secret", "a value is required on stdin");
          if (!s.council.installed) return fail("secret", "unknown server council");
          s.council.keyStored = true;
          return { key: argv[3], server: "council", stored: true };
        }
        return undefined;
      default:
        return undefined;
    }
  }

  return {
    state: s,
    reply,
    /** Another machine pushed a newer version of a file to the bundle. */
    remoteEdit(path: string, version: string) {
      sync().remote = { ...(sync().remote ?? {}), [path]: version };
    },
    localEdit(path: string, version: string) {
      sync().local[path] = version;
    },
  };
}

export type SystemWorld = ReturnType<typeof createSystemWorld>;
