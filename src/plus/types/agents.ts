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

/** `data` of the agents commands, checked against the golden envelopes by `data.test.ts`. */

export const agentsAddData = obj({
  dryRun: bool,
  name: str,
  path: str,
  repo: str,
});
export type AgentsAddData = Infer<typeof agentsAddData>;

export const agentsAuditData = obj({
  agentCount: num,
  clean: bool,
  discoveryWarnings: arr(any),
  findings: arr(any),
  high: num,
  low: num,
  medium: num,
  repo: str,
});
export type AgentsAuditData = Infer<typeof agentsAuditData>;

export const agentsCleanData = obj({
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
export type AgentsCleanData = Infer<typeof agentsCleanData>;

export const agentsDiffData = obj({
  clean: bool,
  discoveryWarnings: arr(any),
  modified: arr(any),
  new: arr(str),
  noLockfile: bool,
  removed: arr(any),
  repo: str,
  unchanged: num,
});
export type AgentsDiffData = Infer<typeof agentsDiffData>;

export const agentsLintData = obj({
  agentCount: num,
  discoveryWarnings: arr(any),
  errors: num,
  infos: num,
  messages: arr(any),
  repo: str,
  warnings: num,
});
export type AgentsLintData = Infer<typeof agentsLintData>;

export const agentsStatusData = obj({
  drift: bool,
  lockedCount: num,
  lockfilePresent: bool,
  outputRoot: nullable(str),
  outputs: arr(any),
  repo: str,
});
export type AgentsStatusData = Infer<typeof agentsStatusData>;

export const agentsSyncData = obj({
  agentCount: num,
  agents: arr(
    obj({
      clientsSynced: arr(str),
      found: bool,
      model: str,
      name: str,
      outputFiles: arr(
        obj({
          client: str,
          files: arr(str),
        }),
      ),
      warnings: arr(any),
    }),
  ),
  clientCount: num,
  discoveryWarnings: arr(any),
  dryRun: bool,
  foundCount: num,
  lockDir: str,
  outputRoot: str,
  repo: str,
  scope: str,
  syncedAt: str,
});
export type AgentsSyncData = Infer<typeof agentsSyncData>;

export const agentsUninstallData = obj({
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
export type AgentsUninstallData = Infer<typeof agentsUninstallData>;

/** Golden file stem to the shape of its envelope `data`. */
export const agentsShapes: Record<string, Shape<unknown>> = {
  "agents-add.apply": agentsAddData,
  "agents-add.preview": agentsAddData,
  "agents-audit": agentsAuditData,
  "agents-clean.apply": agentsCleanData,
  "agents-clean.preview": agentsCleanData,
  "agents-diff": agentsDiffData,
  "agents-lint": agentsLintData,
  "agents-status": agentsStatusData,
  "agents-sync.apply": agentsSyncData,
  "agents-sync.preview": agentsSyncData,
  "agents-uninstall.apply": agentsUninstallData,
  "agents-uninstall.preview": agentsUninstallData,
};
