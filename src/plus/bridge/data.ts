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
