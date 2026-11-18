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

export type RepoState =
  "up_to_date" | "behind" | "dirty" | "diverged" | "detached" | "no_upstream" | "busy";

/** One server that runs from a git checkout, as `server repo status` reports it. */
export interface RepoRowData {
  server: string;
  id: string;
  root: string;
  branch: string | null;
  head: string | null;
  remote: string | null;
  url: string | null;
  upstream: string | null;
  upstreamHead: string | null;
  ahead: number;
  behind: number;
  dirty: number;
  unpushed: number;
  state: RepoState;
  checkedAt: string | null;
  error: string | null;
}

export interface RepoStatusData {
  repos: RepoRowData[];
}

export interface RepoCheckData {
  checkedAt: string;
  repos: Array<{ server: string; ok: boolean; error?: string }>;
}

export interface RepoPlan {
  server: string;
  id: string;
  root: string;
  branch: string | null;
  upstream: string | null;
  from: string | null;
  to: string | null;
  commits: Array<{ id: string; subject: string }>;
  commitsTotal: number;
  files: Array<{ status: string; path: string }>;
  filesTotal: number;
  sharedWith: string[];
  refusal: { code: string; message: string } | null;
  build: { command: string | null; remembered: boolean; timeoutSeconds: number };
  /** `null` when no gateway answered: nothing is running that a restart could cut. */
  inFlight: number | null;
  planHash: string;
  undo: string;
}

export interface RepoSwitchPlan {
  server: string;
  id: string;
  root: string;
  fromBranch: string | null;
  fromHead: string | null;
  toBranch: string;
  toHead: string;
  machine: string;
  createsBranch: boolean;
  build: { command: string | null; remembered: boolean };
  refusal: { code: string; message: string } | null;
  planHash: string;
}

export interface RepoPlanData {
  plan: RepoPlan;
}

export interface RepoBuildRun {
  command: string;
  ok: boolean;
  exitCode: number;
  elapsedMs: number;
  output: string;
}

export interface RepoDoneData {
  applied: boolean;
  server: string;
  id: string;
  root: string;
  branch: string;
  from: string;
  to: string;
  build: RepoBuildRun | null;
  restart: { status: string; detail: string } | null;
  warnings: string[];
  undo: string;
  /** Set by a branch switch: the branch it left. */
  fromBranch?: string;
}
