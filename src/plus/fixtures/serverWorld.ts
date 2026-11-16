import type { CtlReply } from "./ctlWorld";
import type {
  AccessSetsData,
  MembershipData,
  RestartData,
  RestartGateway,
  ServerInfoData,
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
  declareClientCapabilities: boolean;
  forwardInstructions: boolean;
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

export interface ServerWorldOptions {
  servers?: WorldServer[];
  sets?: WorldSet[];
  /** Used when `sets` is not given: the ids of the fixture registry's access sets. */
  setIds?: string[];
  defaultAccess?: string | null;
  restart?: Record<string, RestartScript>;
  /** Vault entries that exist, as `<server id>/<KEY>`. */
  vault?: string[];
}

const GATEWAYS = [4121, 4188, 4302];

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
    declareClientCapabilities: false,
    forwardInstructions: false,
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
  const vault = new Set(options.vault ?? []);
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
      declareClientCapabilities: server.declareClientCapabilities,
      forwardInstructions: server.forwardInstructions,
      disabledTools: [],
      source: server.source,
      accessSets: setsOf(server.id),
    };
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

  return {
    servers,
    sets,
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
      if (verb === "edit") {
        const command = flagValues(argv, "--command")[0];
        if (command) server.command = command;
        return { ok: true, data: { id: server.id, changed: command ? ["command"] : [] } };
      }
      return fail("usage", `unsupported in the fixture: server ${verb}`);
    },
  };
}

export type ServerWorld = ReturnType<typeof createServerWorld>;
