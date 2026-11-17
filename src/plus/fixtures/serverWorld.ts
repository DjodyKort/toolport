import type { AuthFix, AuthRow, AuthSection, LoginState } from "../bridge/data";
import type { CtlReply } from "./ctlWorld";
import type {
  AccessSetsData,
  EditData,
  InteropData,
  MembershipData,
  ModeData,
  RestartData,
  RestartGateway,
  ServerInfoData,
  LoginData,
  ProbeData,
  ProbeReport,
  SecretGetData,
  SecretRemoveData,
  SecretSetData,
  ServerListData,
  ServerTransport,
  UninstallData,
} from "../servers/data";

export interface WorldServer {
  id: string;
  name: string;
  enabled: boolean;
  transport: ServerTransport;
  command: string | null;
  args: string[];
  url: string | null;
  cwd: string | null;
  env: Array<{ key: string; secret: boolean }>;
  inheritEnv: boolean;
  mode: "auto" | "search";
  declareClientCapabilities: boolean;
  forwardInstructions: boolean;
  maxRequestTimeoutMs: number | null;
  requestTimeoutMs: number | null;
  source: string | null;
}

export interface WorldSet {
  id: string;
  name: string;
  serverIds: string[];
}

/** What a restart of one server does in the fixture. `busy` answers the first `busyProbes` idle
 * checks with a call in flight; `slow` takes `delayMs` to answer. */
export type RestartScript =
  | { kind: "ok"; delayMs?: number }
  | { kind: "partial" }
  | { kind: "none" }
  | { kind: "busy"; busyProbes?: number };

/** The login check of a stdio server as the world starts: what it found and what brings it back. */
export interface LoginSeed {
  state: LoginState;
  reason?: string;
  /** Seconds before now. */
  checkedAgo?: number;
  sinceAgo?: number;
  expiresIn?: number;
  /** `secret`: the next check passes once a secret was set. `sign-in`: once the sign-in ran. */
  fixedBy?: "secret" | "sign-in" | "never";
}

/** How the server's own sign-in answers: it prints a consent link and finishes after `delayMs`,
 * says it is signed in already, or fails. */
export type SignInScript =
  | { kind: "consent"; delayMs?: number; url?: string }
  | { kind: "already" }
  | { kind: "fail"; message: string };

export interface ServerWorldOptions {
  servers?: WorldServer[];
  sets?: WorldSet[];
  /** Used when `sets` is not given: the ids of the fixture registry's access sets. */
  setIds?: string[];
  defaultAccess?: string | null;
  restart?: Record<string, RestartScript>;
  /** Vault entries that exist, as `<server id>/<KEY>`. */
  vault?: string[];
  /** Login checks by server id; a server without one has no probe. */
  logins?: Record<string, LoginSeed>;
  signIn?: Record<string, SignInScript>;
  /** Every vault command fails the way a locked keychain does. */
  vaultLocked?: boolean;
}

const PLACEHOLDER = /^<.*>$|^(changeme|your[-_ ]?(api[-_ ]?)?key.*)$/i;
const CONSENT_URL = "https://accounts.example.test/consent?client=fixture";
const HTTP_AUTH_KEY = "__http_auth__";

interface LoginRuntime {
  seed: LoginSeed;
  state: LoginState;
  reason: string;
  since: number;
  lastProbe: number | null;
  expiresAt: number | null;
  healed: boolean;
}

function fixOf(server: string, state: LoginState): AuthFix | null {
  switch (state) {
    case "expiring":
    case "needs_reauth":
      return {
        action: "reauth",
        server,
        label: `Sign in to ${server} again`,
        command: `toolportctl auth login ${server}`,
      };
    case "revoked":
      return {
        action: "reconsent",
        server,
        label: `Re-consent access for ${server}`,
        command: `toolportctl auth login ${server}`,
      };
    case "misconfigured":
      return {
        action: "fix_config",
        server,
        label: `Check the OAuth client configuration of ${server}`,
        command: null,
      };
    case "unreachable":
      return {
        action: "retry",
        server,
        label: `Re-check ${server}`,
        command: `toolportctl auth probe --server ${server} --force`,
      };
    default:
      return null;
  }
}

const SEVERITY: LoginState[] = [
  "revoked",
  "needs_reauth",
  "misconfigured",
  "expiring",
  "unreachable",
  "unknown",
  "ok",
];

const GATEWAYS = [4121, 4188, 4302];

const LIMIT_MIN_MS = 1;
const LIMIT_MAX_MS = 86_400_000;
const DEFAULTS = {
  defaultLimitMs: 3_600_000,
  defaultRequestTimeoutMs: 30_000,
  bridgeBaseMs: 600_000,
  bridgeGraceMs: 60_000,
};

/** What one server allows a call, as the backend counts it: a local server stretches to its limit. */
function ceilingOf(server: WorldServer): number {
  const window = server.requestTimeoutMs ?? DEFAULTS.defaultRequestTimeoutMs;
  return server.command === null
    ? window
    : Math.max(server.maxRequestTimeoutMs ?? DEFAULTS.defaultLimitMs, window);
}

function interopOf(server: WorldServer, all: WorldServer[]): InteropData {
  const live = all.filter((s) => s.enabled);
  const longest = (list: WorldServer[]) =>
    list.reduce<WorldServer | null>(
      (best, s) => (best === null || ceilingOf(s) > ceilingOf(best) ? s : best),
      null,
    );
  const top = longest(live);
  const other = longest(live.filter((s) => s.id !== server.id));
  const ceiling = top ? ceilingOf(top) + DEFAULTS.bridgeGraceMs : 0;
  return {
    ...DEFAULTS,
    bridgeCeilingMs: Math.max(DEFAULTS.bridgeBaseMs, ceiling),
    bridgeSetBy: ceiling > DEFAULTS.bridgeBaseMs && top ? top.name : null,
    otherLongest: other ? { name: other.name, ceilingMs: ceilingOf(other) } : null,
  };
}

export function stdioServer(id: string, name: string, extra: Partial<WorldServer> = {}) {
  return {
    id,
    name,
    enabled: true,
    transport: "stdio" as const,
    command: "fixture-only",
    args: [],
    url: null,
    cwd: null,
    env: [],
    inheritEnv: false,
    mode: "auto" as const,
    declareClientCapabilities: false,
    forwardInstructions: false,
    maxRequestTimeoutMs: null,
    requestTimeoutMs: null,
    source: "manual",
    ...extra,
  } satisfies WorldServer;
}

/** The three servers of the app fixture (src/test/browser-fixture.tsx), and its `auth-guidance`
 * variant, so the screen shows the same names as upstream's list. */
export function fixtureServers(authGuidance = false): WorldServer[] {
  const servers = ["GitHub", "Linear", "Stripe"].map((name, i) =>
    stdioServer(`fixture-${i}`, name),
  );
  if (!authGuidance) return servers;
  servers[0] = stdioServer("fixture-0", "Remote project tools", {
    transport: "http",
    command: null,
    url: "https://example.invalid/mcp",
  });
  servers[1] = stdioServer("fixture-1", "Service API tools", {
    env: [{ key: "API_KEY", secret: true }],
  });
  return servers;
}

/** The first set lists every server and the others only the first, as the app fixture's `local` and
 * `work` profiles do. */
export function setsFromIds(ids: string[], servers: WorldServer[]): WorldSet[] {
  return ids.map((id, i) => ({
    id,
    name: id === "local" ? "Local fixture" : id.charAt(0).toUpperCase() + id.slice(1),
    serverIds: (i === 0 ? servers : servers.slice(0, 1)).map((s) => s.id),
  }));
}

function fail(code: string, message: string): CtlReply {
  return { ok: false, error: { code, message } };
}

function flagValues(argv: string[], flag: string): string[] {
  return argv.flatMap((arg, i) => (arg === flag && argv[i + 1] ? [argv[i + 1]] : []));
}

/** The server commands of the in-memory toolportctl: enough state for a write to show in the next
 * read, and a restart that answers the way the golden envelopes do. */
export function createServerWorld(options: ServerWorldOptions = {}) {
  const servers = structuredClone(options.servers ?? fixtureServers());
  const sets = structuredClone(
    options.sets ?? setsFromIds(options.setIds ?? ["local", "work"], servers),
  );
  const defaultAccess =
    options.defaultAccess === undefined ? null : options.defaultAccess;
  const script = options.restart ?? {};
  const vault = new Map(
    (options.vault ?? []).map((entry) => [
      entry,
      `fake-${entry.split("/").pop()?.toLowerCase()}-0000-demo`,
    ]),
  );
  const now = () => Math.floor(Date.now() / 1000);
  const logins = new Map<string, LoginRuntime>(
    Object.entries(options.logins ?? {}).map(([id, seed]) => [
      id,
      {
        seed,
        state: seed.state,
        reason: seed.reason ?? "ok",
        since: now() - (seed.sinceAgo ?? 3600),
        lastProbe: seed.checkedAgo === undefined ? null : now() - seed.checkedAgo,
        expiresAt: seed.expiresIn === undefined ? null : now() + seed.expiresIn,
        healed: false,
      },
    ]),
  );
  const probes = new Map<string, number>();

  const find = (key: string) => servers.find((s) => s.id === key || s.name === key);
  const setOf = (key: string) =>
    sets.find((s) => s.id === key) ??
    sets.find((s) => s.name.toLowerCase() === key.trim().toLowerCase());
  const setsOf = (id: string) =>
    sets.filter((s) => s.serverIds.includes(id)).map((s) => s.id);

  function info(server: WorldServer): ServerInfoData {
    return {
      id: server.id,
      name: server.name,
      enabled: server.enabled,
      transport: server.transport,
      command: server.command,
      args: server.args,
      url: server.url,
      cwd: server.cwd,
      env: server.env,
      inheritEnv: server.inheritEnv,
      mode: server.mode,
      declareClientCapabilities: server.declareClientCapabilities,
      forwardInstructions: server.forwardInstructions,
      maxRequestTimeoutMs: server.maxRequestTimeoutMs,
      requestTimeoutMs: server.requestTimeoutMs,
      interop: interopOf(server, servers),
      disabledTools: [],
      source: server.source,
      accessSets: setsOf(server.id),
    };
  }

  function mode(server: WorldServer, requested: string | undefined): CtlReply {
    if (requested !== "auto" && requested !== "search") {
      return fail("usage", "mode must be auto or search");
    }
    const changed = server.mode !== requested;
    server.mode = requested;
    return {
      ok: true,
      data: {
        id: server.id,
        name: server.name,
        mode: requested,
        changed,
      } satisfies ModeData,
    };
  }

  function edit(server: WorldServer, argv: string[]): CtlReply {
    const changed: string[] = [];
    const command = flagValues(argv, "--command")[0];
    if (command) {
      server.command = command;
      changed.push("command");
    }
    for (const [flag, field] of [
      ["--declare-client-capabilities", "declareClientCapabilities"],
      ["--forward-instructions", "forwardInstructions"],
    ] as const) {
      const given = flagValues(argv, flag)[0];
      if (given === undefined) continue;
      if (given !== "on" && given !== "off")
        return fail("usage", `${flag}: '${given}' is not on or off`);
      if (server[field] !== (given === "on")) changed.push(field);
      server[field] = given === "on";
    }
    const limit = flagValues(argv, "--max-request-timeout-ms")[0];
    if (limit !== undefined) {
      const ms = limit === "default" ? null : Number(limit);
      if (
        ms !== null &&
        !(Number.isInteger(ms) && ms >= LIMIT_MIN_MS && ms <= LIMIT_MAX_MS)
      )
        return fail(
          "usage",
          `--max-request-timeout-ms: the call limit must be between ${LIMIT_MIN_MS} and ${LIMIT_MAX_MS} milliseconds (24 hours)`,
        );
      if (ms !== server.maxRequestTimeoutMs) changed.push("maxRequestTimeoutMs");
      server.maxRequestTimeoutMs = ms;
    }
    return { ok: true, data: { id: server.id, changed } satisfies EditData };
  }

  function restart(server: WorldServer, argv: string[]): CtlReply {
    const how = script[server.id] ?? { kind: "ok" as const };
    const base = { id: server.id, name: server.name };
    const gateway = (
      pid: number,
      extra: Partial<RestartGateway> = {},
    ): RestartGateway => ({
      pid,
      restarted: true,
      detail: "restarted",
      ...extra,
    });
    if (argv.includes("--dry-run"))
      return {
        ok: true,
        data: { ...base, status: "dry_run", gateways: [] } satisfies RestartData,
      };
    if (how.kind === "none")
      return { ok: true, data: { ...base, status: "not_running", gateways: [] } };
    if (argv.includes("--if-idle") && how.kind === "busy") {
      const asked = probes.get(server.id) ?? 0;
      probes.set(server.id, asked + 1);
      if (asked < (how.busyProbes ?? Number.POSITIVE_INFINITY)) {
        return {
          ok: true,
          data: {
            ...base,
            status: "busy",
            gateways: [
              gateway(GATEWAYS[0], {
                restarted: false,
                detail: "1 call in flight",
                inFlight: 1,
              }),
              gateway(GATEWAYS[1], { restarted: false, detail: "idle", inFlight: 0 }),
            ],
          } satisfies RestartData,
        };
      }
    }
    if (how.kind === "partial") {
      return {
        ok: false,
        data: {
          ...base,
          status: "failed",
          gateways: [
            gateway(GATEWAYS[0]),
            gateway(GATEWAYS[1]),
            gateway(GATEWAYS[2], {
              restarted: false,
              detail: "could not start it: uv: command not found",
            }),
          ],
        } satisfies RestartData,
        error: { code: "unhealthy", message: "one or more checks failed" },
      };
    }
    return {
      ok: true,
      data: {
        ...base,
        status: "restarted",
        gateways: GATEWAYS.map((pid) => gateway(pid)),
      },
      delayMs: how.kind === "ok" ? how.delayMs : undefined,
    };
  }

  function uninstall(server: WorldServer, argv: string[]): CtlReply {
    const dryRun = argv.includes("--dry-run");
    const secrets = server.env.filter((e) => e.secret).map((e) => e.key);
    const removed = dryRun || argv.includes("--keep-secrets") ? [] : secrets;
    if (!dryRun) {
      servers.splice(servers.indexOf(server), 1);
      for (const set of sets)
        set.serverIds = set.serverIds.filter((id) => id !== server.id);
      for (const key of removed) vault.delete(`${server.id}/${key}`);
      logins.delete(server.id);
    }
    return {
      ok: true,
      data: {
        id: server.id,
        name: server.name,
        dryRun,
        clients: [],
        secretsRemoved: removed,
      } satisfies UninstallData,
    };
  }

  function setAccess(set: WorldSet, serverId: string, included: boolean) {
    set.serverIds = set.serverIds.filter((id) => id !== serverId);
    if (included) set.serverIds.push(serverId);
  }

  function tool(name: string, args: Record<string, unknown>): CtlReply | null {
    const wrap = (result: unknown, tier: number): CtlReply => ({
      ok: true,
      data: { tool: name, tier, isError: false, result },
    });
    if (name === "servers_list_profiles") {
      return wrap({ defaultAccess, accessSets: sets } satisfies AccessSetsData, 1);
    }
    if (name === "servers_add_profile_tag" || name === "servers_remove_profile_tag") {
      const server = find(String(args.name ?? ""));
      const set = setOf(String(args.profile_tag ?? ""));
      if (!server || !set) {
        const what = server
          ? `access set not found: ${args.profile_tag}`
          : `server not found: ${args.name}`;
        return {
          ok: true,
          data: {
            tool: name,
            tier: 2,
            isError: true,
            result: { error: { kind: "not_found", message: what } },
          },
        };
      }
      setAccess(set, server.id, name === "servers_add_profile_tag");
      return wrap(
        {
          name: server.name,
          enabled: server.enabled,
          accessSets: setsOf(server.id),
        } satisfies MembershipData,
        2,
      );
    }
    return null;
  }

  function row(server: string, entry: LoginRuntime): AuthRow {
    return {
      server,
      state: entry.state,
      reason: entry.reason,
      since: entry.since,
      lastProbe: entry.lastProbe,
      expiresAt: entry.expiresAt,
      ttlSecs: entry.expiresAt === null ? null : entry.expiresAt - now(),
      fix: fixOf(server, entry.state),
    };
  }

  function authRows(): AuthRow[] {
    return [...logins.entries()]
      .filter(([id]) => servers.some((server) => server.id === id))
      .map(([id, entry]) => row(id, entry))
      .sort(
        (a, b) =>
          SEVERITY.indexOf(a.state) - SEVERITY.indexOf(b.state) ||
          a.server.localeCompare(b.server),
      );
  }

  function authSection(): AuthSection {
    const rows = authRows();
    const counts = Object.fromEntries(
      SEVERITY.map((state) => [state, rows.filter((r) => r.state === state).length]),
    ) as AuthSection["counts"];
    return { counts, servers: rows };
  }

  function probe(id: string): ProbeReport {
    const entry = logins.get(id)!;
    const at = now();
    const fixed = entry.healed && entry.seed.fixedBy !== "never";
    const next: LoginState = fixed ? "ok" : entry.seed.state;
    if (next !== entry.state) entry.since = at;
    entry.state = next;
    entry.reason = fixed ? "ok" : (entry.seed.reason ?? "ok");
    entry.lastProbe = at;
    entry.expiresAt = fixed ? null : entry.expiresAt;
    return {
      server: id,
      ran: true,
      skipped: null,
      tracked: { reason: entry.reason, since: entry.since, state: entry.state },
      nextDueAt: at + 1800,
    };
  }

  function locked(): CtlReply | null {
    return options.vaultLocked
      ? fail("vault", "the keychain is locked; unlock it and try again")
      : null;
  }

  function secret(argv: string[], stdin: string | undefined): CtlReply {
    const [, verb, key, name] = argv;
    const flags = argv.slice(2);
    if (!key || !name) return fail("usage", `usage: secret ${verb} <server> <KEY>`);
    const server = find(key);
    if (!server) return fail("not_found", `unknown server: ${key}`);
    const secretKey = name;
    const path = `${server.id}/${secretKey}`;
    const refused = locked();
    if (refused) return refused;
    if (verb === "get") {
      if (secretKey === HTTP_AUTH_KEY && flags.includes("--reveal"))
        return fail("usage", `${HTTP_AUTH_KEY} is an access token and is never printed`);
      const value = vault.get(path);
      if (value === undefined)
        return fail("not_found", `${secretKey} is not set for ${server.id}`);
      const data: SecretGetData = { server: server.id, key: secretKey, set: true };
      if (flags.includes("--reveal")) data.value = value;
      return { ok: true, data };
    }
    if (verb === "set") {
      const value = (stdin ?? "").replace(/\r?\n$/, "");
      if (value === "") return fail("input", "value is empty");
      if (PLACEHOLDER.test(value))
        return fail(
          "vault",
          "Enter a credential value. Placeholder values cannot be saved.",
        );
      vault.set(path, value);
      if (!server.env.some((e) => e.key === secretKey))
        server.env.push({ key: secretKey, secret: true });
      const entry = logins.get(server.id);
      if (entry?.seed.fixedBy === "secret") entry.healed = true;
      return {
        ok: true,
        data: { server: server.id, key: secretKey, stored: true } satisfies SecretSetData,
      };
    }
    if (verb === "rm") {
      vault.delete(path);
      server.env = server.env.filter((e) => e.key !== secretKey);
      return {
        ok: true,
        data: {
          server: server.id,
          key: secretKey,
          removed: true,
        } satisfies SecretRemoveData,
      };
    }
    return fail("usage", `unsupported in the fixture: secret ${verb}`);
  }

  function auth(argv: string[]): CtlReply {
    const [, verb, target] = argv;
    if (verb === "probe") {
      const id = flagValues(argv, "--server")[0];
      const found = id ? find(id) : undefined;
      if (id && (!found || !logins.has(found.id)))
        return fail(
          "not_found",
          `no auth probe is registered for ${id}; \`toolportctl auth login ${id}\` shows its sign-in step`,
        );
      const ids = found ? [found.id] : [...logins.keys()];
      const probes = ids.map(probe);
      const data: ProbeData = {
        counts: authSection().counts,
        mode: found ? "server" : "all",
        probes,
        failures: [],
        servers: authRows().filter((r) => ids.includes(r.server)),
      };
      return { ok: true, data };
    }
    if (verb === "login") {
      const server = target ? find(target) : undefined;
      if (!server) return fail("not_found", `unknown server: ${target ?? ""}`);
      const how = options.signIn?.[server.id] ?? ({ kind: "consent" } as SignInScript);
      const base = { server: server.id, name: server.name, flow: "stdio" as const };
      if (how.kind === "fail") return fail("auth_login", how.message);
      const entry = logins.get(server.id);
      if (entry && entry.seed.fixedBy === "sign-in") entry.healed = true;
      const report = entry ? probe(server.id) : null;
      const data: LoginData = {
        ...base,
        consentUrl: how.kind === "consent" ? (how.url ?? CONSENT_URL) : null,
        signedIn: true,
        message:
          how.kind === "consent"
            ? `Signed in to ${server.name}.`
            : `${server.name} reports it is already signed in.`,
        probe: report,
        servers: authRows().filter((r) => r.server === server.id),
      };
      return how.kind === "consent"
        ? {
            ok: true,
            data,
            stderr: [`Open this URL to sign in:`, `  ${how.url ?? CONSENT_URL}`],
            delayMs: how.delayMs ?? 1500,
          }
        : { ok: true, data };
    }
    return fail("usage", `unsupported in the fixture: auth ${verb}`);
  }

  return {
    servers,
    sets,
    authSection,
    secret,
    auth,
    secretStatus(serverId: string, keys: string[]): Array<[string, boolean]> {
      return keys.map((key) => [key, vault.has(`${serverId}/${key}`)]);
    },
    tool,
    /** Upstream's own write of an access set (its Access page), so both readers see one state. */
    setAccessServer(setId: string, serverId: string, included: boolean) {
      const set = setOf(setId);
      if (set) setAccess(set, serverId, included);
    },
    run(argv: string[]): CtlReply {
      const [, verb, key] = argv;
      if (verb === "ls") {
        const data: ServerListData = {
          defaultAccess,
          servers: servers.map(({ id, name, enabled, transport }) => ({
            id,
            name,
            enabled,
            transport,
          })),
        };
        return { ok: true, data };
      }
      if (!key) return fail("usage", `usage: server ${verb} <id|name>`);
      const server = find(key);
      if (!server) return fail("not_found", `no server '${key}'`);
      if (verb === "info") return { ok: true, data: info(server) };
      if (verb === "restart") return restart(server, argv);
      if (verb === "uninstall") return uninstall(server, argv);
      if (verb === "mode") return mode(server, argv[3]);
      if (verb === "edit") return edit(server, argv);
      return fail("usage", `unsupported in the fixture: server ${verb}`);
    },
  };
}

export type ServerWorld = ReturnType<typeof createServerWorld>;
