/** The `data` of the toolportctl envelopes the Toolport+ settings page reads. The Rust goldens under
 * plus/fixtures/ctl-envelopes are the source; data.test.ts parses every one of them. */

export type Tier = "read" | "write" | "destructive";
export type Need = "stdin" | "browser" | "long-running" | "network" | "terminal-only";

export interface StatusData {
  version: string;
  dataDir: string;
  serverCount: number;
  accessSetCount: number;
  /** `null` means every enabled server. */
  defaultAccess: string | null;
  secretsBackend: string;
  gateway: { path: string; present: boolean };
  registry: { path: string; exists: boolean; readable: boolean; error: string | null };
  auth: AuthSection;
}

export type LoginState =
  | "ok"
  | "expiring"
  | "needs_reauth"
  | "revoked"
  | "misconfigured"
  | "unreachable"
  | "unknown";

/** Login state of the stdio and Google servers that carry a probe; remotes sign in in upstream. */
export const NO_LOGINS: AuthSection = {
  counts: {
    ok: 0,
    expiring: 0,
    needs_reauth: 0,
    revoked: 0,
    misconfigured: 0,
    unreachable: 0,
    unknown: 0,
  },
  servers: [],
};

/** The one step that fixes a row, as the status file's presentation layer names it. */
export interface AuthFix {
  action: "reauth" | "reconsent" | "fix_config" | "retry";
  server: string;
  label: string;
  /** The `toolportctl` line that runs the fix; `null` when the fix is a change of configuration. */
  command: string | null;
}

export interface AuthRow {
  /** The server id. */
  server: string;
  state: LoginState;
  reason: string;
  /** Unix seconds since the state last changed. */
  since: number;
  /** Unix seconds; `null` until the first probe ran. */
  lastProbe: number | null;
  expiresAt: number | null;
  ttlSecs: number | null;
  fix: AuthFix | null;
}

export interface AuthSection {
  counts: Record<LoginState, number>;
  servers: AuthRow[];
}

export type CheckStatus = "ok" | "warn" | "fail";

export interface DoctorCheck {
  name: string;
  status: CheckStatus;
  detail: string;
}

export interface DoctorData {
  healthy: boolean;
  checks: DoctorCheck[];
}

/** Where the self-management server stands: not registered, removed on purpose, registered but in
 * no access set, working, or switched off in Servers. */
export type SelfMcpState =
  "missing" | "opted-out" | "not-enabled" | "enabled" | "disabled";

export interface SelfMcpCheck {
  name: string;
  ok: boolean;
  detail: string;
}

export interface AccessSetStanding {
  id: string;
  member: boolean;
  optedOut: boolean;
}

export interface McpDoctorData {
  state: SelfMcpState;
  switchedOn: boolean;
  defaultAccess: AccessSetStanding | null;
  clientAccessSets: AccessSetStanding[];
  checks: SelfMcpCheck[];
}

export interface McpInstallData {
  id: string;
  action: "created" | "updated" | "unchanged";
  command: string;
  accessSet: string | null;
  enabled: string[];
}

export interface McpUninstallData {
  id: string;
  removed: boolean;
}

export interface McpToolParam {
  name: string;
  required?: boolean;
  description?: string;
}

export interface McpToolRow {
  name: string;
  description: string;
  tier: number;
  gate: string;
  dryRunDefault: boolean;
  params: McpToolParam[];
}

export interface McpResourceRow {
  name: string;
  uri: string;
  description: string;
  mimeType: string;
}

export interface McpToolsData {
  server: string;
  tools: McpToolRow[];
  resources: McpResourceRow[];
}

export interface McpCallData {
  tool: string;
  tier: number;
  isError: boolean;
  result: unknown;
}

export interface CommandFlag {
  name: string;
  aliases: string[];
  valueType: string;
  effect: string;
  required: boolean;
  repeatable: boolean;
  escalates: boolean;
  hidden: boolean;
  sensitive: boolean;
}

export interface CommandOperand {
  name: string;
  required: boolean;
  variadic: boolean;
}

export interface CommandRow {
  id: string;
  kind: "command" | "group";
  path: string[];
  group: string;
  parent: string | null;
  summary: string;
  tier: Tier | null;
  baseTier: Tier | null;
  planned: boolean;
  surface: "screen" | "terminal" | null;
  needs: Need[];
  flags: CommandFlag[];
  operands: CommandOperand[];
  maxOperands: number | null;
  oneOf: string[][];
  dryRun: boolean;
  preview: { mode: string; flag: string | null } | null;
  tools: string[];
}

export interface CommandToolRow {
  name: string;
  tier: Tier;
  toolTier: number;
  command: string | null;
  dryRun: string;
}

export interface CommandsData {
  commands: CommandRow[];
  tools: CommandToolRow[];
  counts: { commands: number; groups: number; rows: number; tools: number };
  needs: Need[];
  tiers: Tier[];
}

/** `attention ls`: what needs the person now, from the four areas the bell reads. */
export interface AttentionData {
  checkedAt: string;
  counts: { needsYou: number; worthALook: number };
  checks: Array<{
    name: string;
    ok: boolean;
    error?: string;
    /** The person switched this check off; it raises nothing. */
    off?: boolean;
    /** When a check that asks the network last ran. */
    checkedAt?: string;
  }>;
  items: AttentionRow[];
  /** Rows a snooze, skip or dismissal keeps out of `items`. */
  hiddenCount?: number;
}

export interface AttentionRow {
  id: string;
  urgency: "needs-you" | "worth-a-look";
  title: string;
  detail: string;
  source: string;
  /** RFC 3339; absent when the store behind the row keeps no time. */
  since?: string;
  action?: {
    label: string;
    screen: "servers" | "tasks" | "library";
    /** The `toolportctl` argv that fixes the row without opening the screen. */
    command?: string[];
  };
  /** Only a row whose id carries a version or commit range can be skipped. */
  skip?: { label: string };
}
