import { commandsGolden } from "./commandsGolden";
import { createAttentionWorld, type AttentionWorldOptions } from "./attentionWorld";
import { createCatalogWorld, type CatalogWorldOptions } from "./catalogWorld";
import toolsGolden from "../../../plus/fixtures/ctl-envelopes/mcp-tools.json";
import { createCompressionWorld, type CompressionWorldOptions } from "./compressionWorld";
import { createContextWorld, type ContextWorldOptions } from "./contextWorld";
import { createMemoryWorld, type MemoryWorldOptions } from "./memoryWorld";
import { createLibraryWorld, type LibraryWorldOptions } from "./libraryWorld";
import { createSetupWorld } from "./setupWorld";
import { createSyncWorld, type SyncWorldOptions } from "./syncWorld";
import { createServerWorld, type ServerWorldOptions } from "./serverWorld";
import { createTaskWorld, type TaskWorldOptions } from "./taskWorld";
import type {
  AccessSetStanding,
  CommandsData,
  DoctorCheck,
  DoctorData,
  McpDoctorData,
  McpInstallData,
  McpToolsData,
  McpUninstallData,
  SelfMcpCheck,
  SelfMcpState,
  StatusData,
} from "../bridge/data";

export const SELF_ID = "toolport-plus-self";
const DATA_DIR = "/Users/example/Library/Application Support/Toolport";
const BIN_DIR = "/Applications/Toolport.app/Contents/MacOS";
const SELFMCP_BIN = `${BIN_DIR}/toolport-selfmcp`;

export interface CtlReply {
  ok: boolean;
  data?: unknown;
  error?: { code: string; message: string };
  /** How long the fixture takes to answer, for a restart that waits on the gateways. */
  delayMs?: number;
  /** Lines the command prints on stderr while it runs, such as a sign-in link. */
  stderr?: string[];
}

export interface WorldOptions {
  /** Where the self-management server starts. */
  self?: SelfMcpState;
  /** `warn`: the gateway lacks the fork seam. `fail`: the registry cannot be read. */
  doctor?: "ok" | "warn" | "fail";
  /** Access sets of the fixture registry, default first. */
  accessSets?: string[];
  /** The servers, their access sets and how a restart answers. */
  servers?: ServerWorldOptions;
  /** The tasks, their runs and how the `task` commands answer. */
  tasks?: TaskWorldOptions;
  /** What `attention ls` answers; without it the command fails and the bell stays out. */
  attention?: AttentionWorldOptions;
  /** The Context screen's folders, bundles and measurements. */
  context?: ContextWorldOptions;
  /** The hook entries, the pause and the measurements of the Hooks & compression tab. */
  compression?: CompressionWorldOptions;
  /** The Library screen's library, plan, remote, taps and sources. */
  library?: LibraryWorldOptions;
  /** The auto memory, instruction files and sessions of the Memory & history views. */
  memory?: MemoryWorldOptions;
  /** The local registry index the Settings card reads. */
  catalog?: CatalogWorldOptions;
  /** The Sync view of Setup & backup: set up or not, locked keychain, conflicts, the remote. */
  sync?: SyncWorldOptions;
}

/** A small in-memory toolportctl: enough state for install, uninstall and doctor to answer the way
 * the real binary does, and for a second read to see the first write. */
export function createCtlWorld(options: WorldOptions = {}) {
  const accessSets = options.accessSets ?? ["local", "work"];
  const serverWorld = createServerWorld({ setIds: accessSets, ...options.servers });
  const taskWorld = createTaskWorld(options.tasks);
  const contextWorld = createContextWorld(options.context);
  const compressionWorld = createCompressionWorld(options.compression);
  const setupWorld = createSetupWorld();
  const syncWorld = createSyncWorld(options.sync);
  const libraryWorld = createLibraryWorld(options.library);
  const memoryWorld = createMemoryWorld(options.memory);
  const attentionWorld = createAttentionWorld(options.attention);
  const catalogWorld = createCatalogWorld(options.catalog);
  const defaultAccess = accessSets[0] ?? null;
  const doctorMode = options.doctor ?? "ok";
  let state: SelfMcpState = options.self ?? "missing";
  let listedIn = new Set<string>(
    state === "enabled" || state === "disabled" ? [accessSets[0]] : [],
  );

  const standing = (id: string): AccessSetStanding => ({
    id,
    member: listedIn.has(id),
    optedOut: state === "opted-out",
  });

  function selfChecks(): SelfMcpCheck[] {
    const registered = state !== "missing" && state !== "opted-out";
    const skipped = state === "opted-out" ? "skipped: opted out" : "-";
    const detail = (ok: string) => (registered ? ok : skipped);
    return [
      {
        name: "registry_entry",
        ok: state !== "missing",
        detail:
          state === "missing"
            ? "run `toolportctl mcp install`"
            : state === "opted-out"
              ? "opted out; `toolportctl mcp install` brings it back"
              : SELF_ID,
      },
      { name: "command_matches_binary", ok: true, detail: detail(SELFMCP_BIN) },
      { name: "binary_present", ok: true, detail: SELFMCP_BIN },
      {
        name: "server_enabled",
        ok: state === "enabled" || state === "not-enabled",
        detail: registered
          ? state === "disabled"
            ? "switched off in Servers"
            : "switched on"
          : skipped,
      },
      {
        name: "listed_in_default_access",
        ok: state !== "not-enabled" && state !== "missing",
        detail: !registered
          ? skipped
          : state === "not-enabled"
            ? `${defaultAccess}: run \`toolportctl mcp install\``
            : (defaultAccess ?? ""),
      },
      {
        name: "listed_in_client_access_sets",
        ok: true,
        detail: "no client is scoped to an access set",
      },
      { name: "handshake", ok: true, detail: SELF_ID },
      {
        name: "catalog",
        ok: true,
        detail: `${toolsGolden.envelope.data.tools.length} tools, ${toolsGolden.envelope.data.resources.length} resources`,
      },
    ];
  }

  function mcpDoctor(): CtlReply {
    const checks = selfChecks();
    const data: McpDoctorData = {
      state,
      switchedOn: state === "enabled" || state === "not-enabled",
      defaultAccess: defaultAccess === null ? null : standing(defaultAccess),
      clientAccessSets: [],
      checks,
    };
    return checks.some((c) => !c.ok)
      ? {
          ok: false,
          data,
          error: { code: "unhealthy", message: "one or more checks failed" },
        }
      : { ok: true, data };
  }

  function doctor(): CtlReply {
    const checks: DoctorCheck[] = [
      { name: "dataDir", status: "ok", detail: `${DATA_DIR} is writable` },
      {
        name: "registry",
        status: doctorMode === "fail" ? "fail" : "ok",
        detail:
          doctorMode === "fail"
            ? `registry is not readable: ${DATA_DIR}/registry.json: key must be a string at line 1 column 3`
            : `3 servers, ${accessSets.length} access sets`,
      },
      {
        name: "defaultAccess",
        status: doctorMode === "fail" ? "warn" : "ok",
        detail: doctorMode === "fail" ? "no registry to inspect" : "all enabled servers",
      },
      { name: "secretsBackend", status: "ok", detail: "encrypted-file" },
      {
        name: "gatewayBinary",
        status: "ok",
        detail: `${DATA_DIR}/bin/toolport-gateway`,
      },
      {
        name: "plusBuilt",
        status: doctorMode === "ok" ? "ok" : "warn",
        detail:
          doctorMode === "ok"
            ? `this binary and ${DATA_DIR}/bin/toolport-gateway carry the fork seam`
            : `${DATA_DIR}/bin/toolport-gateway was built without the fork seam: the egress guard and fork hooks are off in it`,
      },
    ];
    const data: DoctorData = {
      checks,
      healthy: checks.every((c) => c.status !== "fail"),
    };
    return data.healthy
      ? { ok: true, data }
      : {
          ok: false,
          data,
          error: { code: "unhealthy", message: "one or more checks failed" },
        };
  }

  function status(): CtlReply {
    const broken = doctorMode === "fail";
    const data: StatusData = {
      version: "2.3.6",
      dataDir: DATA_DIR,
      serverCount: broken ? 0 : serverWorld.servers.length,
      accessSetCount: broken ? 0 : accessSets.length,
      defaultAccess: broken ? null : defaultAccess,
      secretsBackend: "encrypted-file",
      gateway: { path: `${DATA_DIR}/bin/toolport-gateway`, present: true },
      auth: serverWorld.authSection(),
      registry: {
        path: `${DATA_DIR}/registry.json`,
        exists: true,
        readable: !broken,
        error: broken
          ? `registry is not readable: ${DATA_DIR}/registry.json: key must be a string at line 1 column 3`
          : null,
      },
    };
    return { ok: true, data };
  }

  function install(argv: string[]): CtlReply {
    const at = argv.findIndex((a) => a === "--access-set" || a === "--profile");
    const set = at >= 0 ? (argv[at + 1] ?? null) : null;
    if (set !== null && !accessSets.includes(set)) {
      return {
        ok: false,
        error: { code: "not_found", message: `unknown access set: ${set}` },
      };
    }
    const created = state === "missing";
    state = state === "disabled" ? "disabled" : "enabled";
    if (state === "enabled")
      listedIn = new Set([...listedIn, defaultAccess ?? "", ...(set ? [set] : [])]);
    listedIn.delete("");
    const data: McpInstallData = {
      id: SELF_ID,
      action: created ? "created" : "updated",
      command: SELFMCP_BIN,
      accessSet: set,
      enabled: [...listedIn],
    };
    return { ok: true, data };
  }

  function uninstall(): CtlReply {
    if (state === "missing" || state === "opted-out") {
      return {
        ok: false,
        error: { code: "not_found", message: `${SELF_ID} is not registered` },
      };
    }
    state = "opted-out";
    listedIn = new Set();
    const data: McpUninstallData = { id: SELF_ID, removed: true };
    return { ok: true, data };
  }

  const commands = commandsGolden.envelope.data as unknown as CommandsData;
  const tools = toolsGolden.envelope.data as unknown as McpToolsData;

  function call(argv: string[], stdin: string | undefined): CtlReply {
    const tool = argv[2];
    const given = stdin?.trim() ? JSON.parse(stdin) : {};
    const handled =
      serverWorld.tool(tool ?? "", given) ?? libraryWorld.tool(tool ?? "", given);
    if (handled) return handled;
    const row = tools.tools.find((t) => t.name === tool);
    if (!row) {
      return {
        ok: false,
        error: { code: "usage", message: `unknown tool: ${tool ?? ""}` },
      };
    }
    if (stdin?.trim() && stdin.trim() !== "{}") {
      return {
        ok: false,
        error: {
          code: "invalid_arguments",
          message: `unknown argument: ${Object.keys(JSON.parse(stdin) as object)[0]}`,
        },
      };
    }
    const result =
      tool === "doctor"
        ? doctor().data
        : tool === "where_am_i"
          ? status().data
          : {
              format: "mermaid",
              diagram: "flowchart LR\n  skills --> claude\n  servers --> gateway",
            };
    return { ok: true, data: { tool, tier: row.tier, isError: false, result } };
  }

  return {
    get state() {
      return state;
    },
    servers: serverWorld,
    tasks: taskWorld,
    library: libraryWorld,
    memory: memoryWorld,
    catalog: catalogWorld,
    run(argv: string[], stdin?: string): CtlReply {
      const key = argv.filter((a) => a !== "--json");
      const [first, second] = key;
      if (first === "status") return status();
      if (first === "doctor") return doctor();
      if (first === "commands") return { ok: true, data: commands };
      if (first === "attention") return attentionWorld.run(key);
      if (first === "server") return serverWorld.run(key);
      if (first === "secret") return serverWorld.secret(key, stdin);
      if (first === "auth") return serverWorld.auth(key);
      if (first === "task") return taskWorld.run(key.slice(1), stdin);
      if (first === "context" || first === "plugins") return contextWorld.run(key);
      if (first === "memory" || first === "sessions") return memoryWorld.run(key);
      if (first === "compression") return compressionWorld.run(key);
      if (first === "setup") return setupWorld.run(key, stdin);
      if (first === "catalog") return catalogWorld.run(key);
      if (first === "sync") return syncWorld.run(key, stdin);
      if (["skills", "library", "sources", "agents", "styles"].includes(first ?? ""))
        return libraryWorld.run(key);
      if (first === "mcp") {
        if (second === "install") return install(key);
        if (second === "uninstall") return uninstall();
        if (second === "doctor") return mcpDoctor();
        if (second === "tools") return { ok: true, data: tools };
        if (second === "call") return call(key, stdin);
      }
      throw new Error(`Unimplemented fixture command: plus_ctl ${key.join(" ")}`);
    },
  };
}

export type CtlWorld = ReturnType<typeof createCtlWorld>;
