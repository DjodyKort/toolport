import {
  any,
  arr,
  bool,
  nullable,
  num,
  obj,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** Results of the agents self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

export const agentsAuditResult = obj({
  agentCount: num,
  clean: bool,
  discoveryWarnings: arr(any),
  findings: arr(any),
  high: num,
  low: num,
  medium: num,
  repo: str,
});
export type AgentsAuditResult = Infer<typeof agentsAuditResult>;

export const agentsCleanResult = obj({
  cleanRoot: str,
  dryRun: bool,
  ignored: arr(any),
  lockDir: str,
  lockfilePresent: bool,
  managed: arr(str),
  removed: arr(str),
  scope: str,
  skipped: arr(any),
});
export type AgentsCleanResult = Infer<typeof agentsCleanResult>;

export const agentsDiffResult = obj({
  clean: bool,
  discoveryWarnings: arr(any),
  modified: arr(any),
  new: arr(str),
  noLockfile: bool,
  removed: arr(any),
  repo: str,
  unchanged: num,
});
export type AgentsDiffResult = Infer<typeof agentsDiffResult>;

export const agentsEditBodyResult = obj({
  newHash: str,
  sourcePath: str,
});
export type AgentsEditBodyResult = Infer<typeof agentsEditBodyResult>;

export const agentsGetResult = obj({
  body: str,
  description: str,
  model: str,
  name: str,
  path: str,
  tools: arr(any),
});
export type AgentsGetResult = Infer<typeof agentsGetResult>;

export const agentsLintResult = obj({
  errors: num,
  messages: arr(any),
  warnings: num,
});
export type AgentsLintResult = Infer<typeof agentsLintResult>;

export const agentsListResult = obj({
  agents: arr(
    obj({
      description: str,
      model: str,
      name: str,
      path: str,
      tools: arr(any),
    }),
  ),
  repo: str,
});
export type AgentsListResult = Infer<typeof agentsListResult>;

export const agentsListTranspilersResult = obj({
  transpilers: arr(str),
});
export type AgentsListTranspilersResult = Infer<typeof agentsListTranspilersResult>;

export const agentsScaffoldResult = obj({
  created_path: str,
  kind: str,
});
export type AgentsScaffoldResult = Infer<typeof agentsScaffoldResult>;

export const agentsStatusResult = obj({
  drift: bool,
  lockedCount: num,
  lockfilePresent: bool,
  outputRoot: nullable(str),
  outputs: arr(
    obj({
      client: str,
      name: str,
      present: bool,
    }),
  ),
  repo: str,
});
export type AgentsStatusResult = Infer<typeof agentsStatusResult>;

export const agentsSyncResult = obj({
  agentCount: num,
  dryRun: bool,
  globalMode: bool,
  repo: str,
  syncedAt: str,
});
export type AgentsSyncResult = Infer<typeof agentsSyncResult>;

export const agentsUninstallResult = obj({
  dryRun: bool,
  lockDir: str,
  lockUpdated: bool,
  name: str,
  outputRoot: str,
  outputs: arr(str),
  repo: str,
  scope: str,
  sourcePath: str,
});
export type AgentsUninstallResult = Infer<typeof agentsUninstallResult>;

/** Tool name to the shape of its `structuredContent`. */
export const agentsToolShapes: Record<string, Shape<unknown>> = {
  agents_audit: agentsAuditResult,
  agents_clean: agentsCleanResult,
  agents_diff: agentsDiffResult,
  agents_edit_body: agentsEditBodyResult,
  agents_get: agentsGetResult,
  agents_lint: agentsLintResult,
  agents_list: agentsListResult,
  agents_list_transpilers: agentsListTranspilersResult,
  agents_scaffold: agentsScaffoldResult,
  agents_status: agentsStatusResult,
  agents_sync: agentsSyncResult,
  agents_uninstall: agentsUninstallResult,
};
