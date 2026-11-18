import type { CtlReply } from "./ctlWorld";
import type { WorldServer, WorldSet } from "./serverWorld";
import type {
  ReachClient,
  ReachData,
  ReachFinding,
  ReachFolder,
  ReachScope,
} from "../servers/data";

export interface ReachClientSeed {
  id: string;
  name?: string;
  /** An access set id; empty or missing follows the default access. */
  set?: string;
  gateway?: boolean;
}

export interface ReachWorldOptions {
  clients?: ReachClientSeed[];
  /** Folder mappings: the folder and the access set id it maps to. */
  folders?: Array<{ path: string; set: string }>;
  /** Servers with a direct entry in a client: client id, server id, state. */
  direct?: Array<{ client: string; server: string; state?: string }>;
}

export interface ReachState {
  servers: WorldServer[];
  sets: WorldSet[];
  defaultAccess: string | null;
}

const RULE =
  "A folder mapping outranks a client's own access set: inside a mapped folder, and below it, the gateway uses the folder's set for every client. Order: folder mapping, the client's own set, TOOLPORT_PROFILE, Default access.";

/** `client reach` of the in-memory toolportctl, resolving the way the gateway does. */
export function createReachWorld(options: ReachWorldOptions, state: () => ReachState) {
  function scopeOf(world: ReachState, reference: string | null | undefined): ReachScope {
    const key = reference ? reference : (world.defaultAccess ?? "");
    const set = key ? world.sets.find((s) => s.id === key) : undefined;
    const everything = key === "";
    return {
      accessSet: set?.id ?? null,
      accessSetName: set?.name ?? null,
      missing: !set && !everything,
      servers: world.servers
        .filter((s) => s.enabled && (everything || set?.serverIds.includes(s.id)))
        .map((s) => s.id),
    };
  }

  function report(): ReachData {
    const world = state();
    const names = (ids: string[]) =>
      ids.map((id) => world.servers.find((s) => s.id === id)?.name ?? id).join(", ");
    const defaultAccess = scopeOf(world, null);
    const clients: ReachClient[] = (options.clients ?? [])
      .map((seed) => ({
        ...scopeOf(world, seed.set),
        id: seed.id,
        name: seed.name ?? seed.id,
        gateway: seed.gateway ?? true,
        ownSet: Boolean(seed.set),
      }))
      .sort((a, b) => a.id.localeCompare(b.id));
    const folders: ReachFolder[] = (options.folders ?? []).map((f) => ({
      ...scopeOf(world, f.set),
      path: f.path,
    }));
    const direct = options.direct ?? [];
    const servers = world.servers.map((server) => {
      const folderPaths = folders
        .filter((f) => f.servers.includes(server.id))
        .map((f) => f.path);
      const viaDefault = defaultAccess.servers.includes(server.id);
      const ownSetReaches = clients.some(
        (c) => c.ownSet && c.servers.includes(server.id),
      );
      return {
        id: server.id,
        name: server.name,
        enabled: server.enabled,
        listedIn: world.sets
          .filter((s) => s.serverIds.includes(server.id))
          .map((s) => s.id),
        default: viaDefault,
        clients: clients.filter((c) => c.servers.includes(server.id)).map((c) => c.id),
        folders: folderPaths,
        direct: direct
          .filter((d) => d.server === server.id)
          .map((d) => ({ client: d.client, state: d.state ?? "ok" })),
        unreached:
          server.enabled && !viaDefault && !ownSetReaches && folderPaths.length === 0,
      };
    });
    const findings: ReachFinding[] = servers
      .filter((s) => s.unreached && !s.direct.some((d) => d.state === "ok"))
      .map((s) => ({
        kind: "unreached" as const,
        server: s.id,
        client: null,
        folder: null,
        servers: [s.id],
        text: `No access set reaches ${s.name}: the default access, the clients' own sets and the folder mappings all leave it out.`,
      }));
    for (const client of clients.filter((c) => c.missing)) {
      findings.push({
        kind: "missing-set",
        server: null,
        client: client.id,
        folder: null,
        servers: [],
        text: `${client.name} is bound to an access set that no longer exists, so it sees no servers.`,
      });
    }
    for (const folder of folders.filter((f) => !f.missing)) {
      for (const client of clients.filter(
        (c) => c.ownSet && !c.missing && c.accessSet !== folder.accessSet,
      )) {
        const hidden = client.servers.filter((id) => !folder.servers.includes(id));
        const added = folder.servers.filter((id) => !client.servers.includes(id));
        if (hidden.length === 0 && added.length === 0) continue;
        const parts = [
          hidden.length > 0
            ? `hides ${hidden.length} ${hidden.length === 1 ? "server" : "servers"} its own set lists (${names(hidden)})`
            : "",
          added.length > 0
            ? `adds ${added.length} ${added.length === 1 ? "server" : "servers"} its own set leaves out (${names(added)})`
            : "",
        ].filter(Boolean);
        findings.push({
          kind: "folder-outranks",
          server: null,
          client: client.id,
          folder: folder.path,
          servers: [...hidden, ...added],
          text: `In ${folder.path} the gateway uses ${folder.accessSetName ?? "its access set"}, not ${client.name}'s own set: it ${parts.join(" and ")}. ${RULE}`,
        });
      }
    }
    return {
      defaultAccess,
      accessSets: world.sets.map((set) => ({
        id: set.id,
        name: set.name,
        listed: set.serverIds,
        reached: set.serverIds.filter(
          (id) => world.servers.find((s) => s.id === id)?.enabled,
        ),
        default: world.defaultAccess === set.id,
        clients: clients.filter((c) => c.accessSet === set.id).map((c) => c.id),
        folders: folders.filter((f) => f.accessSet === set.id).map((f) => f.path),
      })),
      clients,
      folders,
      servers,
      findings,
      accessRows: [],
    };
  }

  return {
    run(argv: string[]): CtlReply {
      const flag = (name: string) => {
        const at = argv.indexOf(name);
        return at >= 0 ? argv[at + 1] : undefined;
      };
      const data = report();
      const server = flag("--server");
      const client = flag("--client");
      if (server && !data.servers.some((s) => s.id === server || s.name === server)) {
        return {
          ok: false,
          error: { code: "not_found", message: `no server '${server}'` },
        };
      }
      if (client && !data.clients.some((c) => c.id === client)) {
        return {
          ok: false,
          error: {
            code: "not_found",
            message: `no client '${client}' reaches the gateway`,
          },
        };
      }
      return { ok: true, data };
    },
  };
}
