/** The `data` of the toolportctl envelopes the Servers screen reads. The Rust goldens under
 * plus/fixtures/ctl-envelopes and plus/fixtures/selfmcp-envelopes are the source; data.test.ts
 * parses every server golden against these shapes. */

import type { AuthRow, LoginState } from "../bridge/data";

export type ServerTransport = "stdio" | "http" | "sse";

export interface ServerListRow {
  id: string;
  name: string;
  enabled: boolean;
  transport: ServerTransport;
}

export interface ServerListData {
  /** An access set id; `null` means every enabled server. */
  defaultAccess: string | null;
  servers: ServerListRow[];
}

export interface ServerEnvKey {
  key: string;
  secret: boolean;
}

/** How the stdio bridge's wait follows the call limits of the enabled servers. The `Ms` constants are
 * the backend's own, so the screen's preview agrees with what the bridge does. */
export interface InteropData {
  /** How long the bridge keeps a call open now. */
  bridgeCeilingMs: number;
  /** The server whose limit sets it; `null` while the bridge keeps its base wait. */
  bridgeSetBy: string | null;
  /** The longest per-call wait among the other enabled servers. */
  otherLongest: { name: string; ceilingMs: number } | null;
  defaultLimitMs: number;
  defaultRequestTimeoutMs: number;
  bridgeBaseMs: number;
  bridgeGraceMs: number;
  /** A running gateway has the server connected with fewer client capabilities than its clients
   * declared since, and the server opted in to being told them. It learns them at the next client
   * connect or restart. */
  pendingRedeclare: boolean;
}

export interface ServerInfoData {
  id: string;
  name: string;
  enabled: boolean;
  transport: ServerTransport;
  command: string | null;
  args: string[];
  url: string | null;
  cwd: string | null;
  env: ServerEnvKey[];
  inheritEnv: boolean;
  /** `search` keeps the server's tools out of a Full tool list; they stay callable and searchable. */
  mode: "auto" | "search";
  declareClientCapabilities: boolean;
  forwardInstructions: boolean;
  /** Longest a call to a local server may run while it reports progress; `null` means the default. */
  maxRequestTimeoutMs: number | null;
  /** The server's own request timeout (set in Edit); `null` means the default. */
  requestTimeoutMs: number | null;
  interop: InteropData;
  disabledTools: string[];
  source: string | null;
  /** Ids of the access sets that list the server. */
  accessSets: string[];
}

export interface ModeData {
  id: string;
  name: string;
  mode: "auto" | "search";
  changed: boolean;
}

export interface EditData {
  id: string;
  changed: string[];
}

export type RestartStatus =
  "restarted" | "failed" | "not_running" | "requested" | "dry_run" | "busy";

/** One gateway process's answer. Servers that run once per project folder will add a `root` per
 * row; the screen shows it when it is there. */
export interface RestartGateway {
  pid: number;
  restarted: boolean;
  detail: string;
  inFlight?: number;
  root?: string;
}

export interface RestartData {
  id: string;
  name: string;
  status: RestartStatus;
  gateways: RestartGateway[];
}

export interface UninstallData {
  id: string;
  name: string;
  dryRun: boolean;
  clients: unknown[];
  secretsRemoved: string[];
}

export interface AccessSetRow {
  id: string;
  name: string;
  serverIds: string[];
}

export interface AccessSetsData {
  defaultAccess: string | null;
  accessSets: AccessSetRow[];
}

/** What `client reach` says about one scope: the default access, a client or a folder mapping. */
export interface ReachScope {
  accessSet: string | null;
  accessSetName: string | null;
  /** The scope names an access set that no longer exists, so it reaches nothing. */
  missing: boolean;
  /** Ids of the enabled servers it reaches. */
  servers: string[];
}

export interface ReachClient extends ReachScope {
  id: string;
  name: string;
  /** The client holds Toolport's gateway entry. */
  gateway: boolean;
  /** The client has an access set of its own; otherwise it follows the default access. */
  ownSet: boolean;
}

export interface ReachFolder extends ReachScope {
  path: string;
}

export interface ReachSet {
  id: string;
  name: string;
  listed: string[];
  reached: string[];
  default: boolean;
  /** Ids of the clients that use this set. */
  clients: string[];
  folders: string[];
}

export interface ReachServer {
  id: string;
  name: string;
  enabled: boolean;
  listedIn: string[];
  default: boolean;
  clients: string[];
  folders: string[];
  direct: Array<{ client: string; state: string }>;
  /** On, and reached by no access set. */
  unreached: boolean;
}

export interface ReachFinding {
  kind: "unreached" | "folder-outranks" | "missing-set" | "direct-orphan";
  server: string | null;
  client: string | null;
  folder: string | null;
  servers: string[];
  text: string;
}

export interface ReachData {
  defaultAccess: ReachScope | null;
  accessSets: ReachSet[];
  clients: ReachClient[];
  folders: ReachFolder[];
  servers: ReachServer[];
  findings: ReachFinding[];
  /** The rows the Access check of Attention shows: what is worth a look about who reaches what. */
  accessRows: Array<{
    id: string;
    title: string;
    detail: string;
    action: { label: string; command: string[] };
  }>;
}

export interface MembershipData {
  name: string;
  enabled: boolean;
  accessSets: string[];
}

export interface SecretGetData {
  server: string;
  key: string;
  set: true;
  /** Only in the answer of `--reveal`. */
  value?: string;
}

export interface SecretSetData {
  server: string;
  key: string;
  stored: true;
}

export interface SecretRemoveData {
  server: string;
  key: string;
  removed: true;
}

export interface ProbeReport {
  server: string;
  ran: boolean;
  skipped: string | null;
  tracked: { reason: string; since: number; state: string };
  /** Unix seconds. */
  nextDueAt: number;
}

export interface ProbeData {
  counts: Record<LoginState, number>;
  mode: "due" | "all" | "server";
  probes: ProbeReport[];
  failures: Array<{ server: string; error: string }>;
  servers: AuthRow[];
}

export interface LoginData {
  server: string;
  name: string;
  flow: "stdio" | "browser";
  consentUrl: string | null;
  signedIn: boolean;
  message: string;
  probe: ProbeReport | null;
  servers: AuthRow[];
}
