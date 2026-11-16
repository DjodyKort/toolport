/** The `data` of the toolportctl envelopes the Servers screen reads. The Rust goldens under
 * plus/fixtures/ctl-envelopes and plus/fixtures/selfmcp-envelopes are the source; data.test.ts
 * parses every server golden against these shapes. */

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
  disabledTools: string[];
  source: string | null;
  /** Ids of the access sets that list the server. */
  accessSets: string[];
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
