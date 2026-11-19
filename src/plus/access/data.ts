/** The `data` of `toolportctl access-set`, `client scope` and `folder route`, and of the
 * `client_discovery_set` tool. The Rust goldens under plus/fixtures/ctl-envelopes
 * (access-set-*, client-scope-*, folder-route-*) and plus/fixtures/selfmcp-envelopes are the
 * source; data.test.ts parses them against these shapes. */

export interface NamedRef {
  id: string;
  name: string;
}

export interface WriteStep {
  op: string;
  detail: string;
}

export interface WritePlan {
  summary: string;
  steps: WriteStep[];
  warnings: string[];
  /** The command line that puts the change back, as the CLI words it. */
  undo: string;
}

export interface WriteResult {
  applied: boolean;
  changed: string[];
  undo: string;
  backups: string[];
}

/** What every writer answers: the plan always, the result once it ran for real. */
export interface WriteOutcome {
  dryRun: boolean;
  plan: WritePlan;
  result: WriteResult | null;
  /** The set the write is about; a full row once an add has run. */
  accessSet?: (NamedRef & Partial<AccessSetRow>) | null;
  client?: string;
  path?: string;
  server?: NamedRef;
  tools?: string[] | null;
  before?: string[] | null;
  defaultAccess?: NamedRef | null;
  name?: string;
  mode?: string;
}

export interface AccessSetRow {
  id: string;
  name: string;
  default: boolean;
  servers: NamedRef[];
  /** Enabled servers the set lists. */
  reached: number;
  /** Clients with this set of their own. */
  clients: string[];
  /** Folder routes that use it. */
  folders: string[];
}

export interface AccessSetListData {
  /** An access set id; `null` means every enabled server. */
  defaultAccess: string | null;
  accessSets: AccessSetRow[];
}

export interface RouteRow {
  path: string;
  accessSet: NamedRef | null;
  /** The route names an access set that no longer exists. */
  missing: boolean;
  reference: string;
  servers: NamedRef[];
}

export interface RoutesData {
  rule: string;
  routes: RouteRow[];
}

export interface NarrowedServer {
  server: NamedRef;
  listed: boolean;
  tools: string[];
}

export interface ToolsData {
  rule: string;
  accessSets: Array<NamedRef & { narrowed: NarrowedServer[] }>;
}

/** The `data` of a refused `access-set rm` while clients or folders still use the set. */
export interface InUse {
  clients: string[];
  folders: string[];
}

export type DiscoveryMode = "full" | "lazy" | "grouped" | "inherit";

export const DISCOVERY_MODES: Array<{
  mode: DiscoveryMode;
  label: string;
  hint: string;
}> = [
  { mode: "inherit", label: "Follow the global mode", hint: "The global mode applies." },
  { mode: "full", label: "Full", hint: "The client sees every tool in the list." },
  {
    mode: "lazy",
    label: "Lazy",
    hint: "The client finds tools through toolport_search_tools.",
  },
  { mode: "grouped", label: "Grouped", hint: "The client browses one server at a time." },
];
