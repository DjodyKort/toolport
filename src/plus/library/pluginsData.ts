/** The `data` of `plugins ls|show|config|mcp ls|deny|allow` and `hooks ls`. The Rust goldens under
 * plus/fixtures/ctl-envelopes (plugins-*, hooks-ls.*) are the source; pluginsWorld.test.ts checks
 * the fixture world against them. */

import type { Plan, WriteResult } from "../context/data";

export type UpdateState = "current" | "update" | "unknown";

export interface Tokens {
  value: number;
  basis: "estimate" | "measured" | "projected";
}

export interface PluginEnabled {
  user: boolean | null;
  project: boolean | null;
  local: boolean | null;
  managed: boolean | null;
  effective: boolean;
}

export interface PluginRow {
  id: string;
  name: string;
  marketplace: string | null;
  version: string | null;
  scope: string | null;
  installPath: string | null;
  source: string | null;
  enabled: PluginEnabled;
  update: { state: UpdateState; available: string | null };
  brings: {
    skills: number;
    agents: number;
    commands: number;
    hooks: number;
    mcpServers: number;
    lspServers: number;
  };
  cost: { projected: Tokens | null; measured: Tokens | null; measuredAt: string | null };
  adapter: string | null;
  /** The `library select` source that chooses single items of this plugin, when a tap holds it. */
  selectSource: string | null;
  mcpOutsideGateway: Array<{ key: string; name: string; denied: boolean }>;
  errors: string[];
  from: "claude-cli" | "files";
}

export interface MarketplaceRow {
  name: string;
  source: string | null;
  followsFolder: boolean;
  installed: number;
}

export interface PluginsLsData {
  cwd: string | null;
  plugins: PluginRow[];
  marketplaces: MarketplaceRow[];
  updatesAvailable: number;
  managedSettings: { path: string | null; present: boolean; problem: string | null };
  refreshed: boolean;
  refreshError: string | null;
  partial: boolean;
}

export interface HookEntry {
  owner: { kind: string; name: string };
  event: string;
  matcher: string;
  type: string;
  command: string;
  runtimeId: string | null;
  timeoutSec: number | null;
  async: boolean;
  source: string;
  marker: string | null;
  switch: { method: string; detail: string };
  tools: string[];
}

export interface DeniedWhere {
  user: boolean;
  project: boolean;
  local: boolean;
  managed: boolean;
}

export interface McpServerRow {
  key: string;
  name: string;
  type: string | null;
  command: string[] | null;
  url: string | null;
  toolPrefix: string;
  denied: DeniedWhere;
  deniedByToolport?: boolean;
}

export interface OptionRow {
  key: string;
  title: string;
  description: string;
  type: string;
  default: unknown;
  sensitive: boolean;
  /** `null` for a sensitive option: its value is never returned. */
  current: string | null;
  configured: boolean | null;
  choices: string[] | null;
}

export type KnobKind = "enum" | "bool" | "bool-off" | "csv" | "globs" | "hook-ids";

export interface KnobRow {
  key: string;
  label: string;
  kind: KnobKind;
  env: string;
  option: string | null;
  choices: string[] | null;
  default: string | null;
  folderOnly: boolean;
  list: boolean;
  current: { value: string | null; from: string };
}

export interface PluginShowData extends PluginRow {
  description: string | null;
  components: {
    skills: string[];
    agents: string[];
    commands: string[];
    lspServers: string[];
  };
  hooks: HookEntry[];
  mcpServers: McpServerRow[];
  options: OptionRow[];
  optionsFrom: "claude-cli" | "files";
  knobs: KnobRow[];
  adapterProblems: Array<{ file: string; message: string }>;
}

export interface PluginConfigData {
  id: string;
  scope: "folder" | "global";
  cwd: string | null;
  adapter: string | null;
  options: OptionRow[];
  optionsFrom: "claude-cli" | "files";
  knobs: KnobRow[];
  writtenByToolport: Array<{ knob: string; env: string; value: string }>;
  adapterProblems: Array<{ file: string; message: string }>;
}

export interface PluginMcpData {
  id: string;
  cwd: string | null;
  servers: McpServerRow[];
  outsideGateway: string;
}

/** `plugins config --set|--unset` and `plugins mcp deny|allow`: a plan and, once applied, a result. */
export interface PluginWriteData {
  id: string;
  scope: "folder" | "global";
  cwd: string | null;
  dryRun: boolean;
  plan: Plan;
  result: WriteResult | null;
  changes: Array<Record<string, unknown>>;
  conflicts: string[];
  ledger: string | null;
  server?: string;
  serverName?: string;
  toolPrefix?: string;
}

export interface HooksLsData {
  cwd: string | null;
  disabledAll: boolean;
  managedOnly: boolean;
  managedSettings: { path: string | null; present: boolean; problem: string | null };
  hooks: HookEntry[];
  counts: {
    byOwner: Record<string, number>;
    perTool: Record<string, { pre: number; post: number; total: number }>;
    otherEvents: Record<string, number>;
  };
  conflicts: Array<{ event: string; tools: string[]; hooks: string[]; note: string }>;
}
