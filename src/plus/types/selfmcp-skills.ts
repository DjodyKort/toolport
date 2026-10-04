import {
  any,
  arr,
  bool,
  nullable,
  num,
  obj,
  opt,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** Results of the skills self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

export const skillsAuditResult = obj({
  clean: bool,
  findings: arr(any),
  high: num,
  low: num,
  medium: num,
  repo: str,
  skillCount: num,
});
export type SkillsAuditResult = Infer<typeof skillsAuditResult>;

export const skillsBundleResult = obj({
  bundleBytes: nullable(num),
  dryRun: bool,
  fileCount: num,
  output: str,
  repo: str,
  skills: arr(
    obj({
      files: num,
      name: str,
      type: str,
    }),
  ),
  sourceBytes: num,
});
export type SkillsBundleResult = Infer<typeof skillsBundleResult>;

export const skillsCleanResult = obj({
  cleanRoot: str,
  dryRun: bool,
  ignored: arr(any),
  lockDir: str,
  lockfilePresent: bool,
  lockfileRemoved: bool,
  managed: arr(str),
  removed: arr(str),
  scope: str,
  skipped: arr(any),
});
export type SkillsCleanResult = Infer<typeof skillsCleanResult>;

export const skillsDeleteResult = obj({
  removedPath: str,
});
export type SkillsDeleteResult = Infer<typeof skillsDeleteResult>;

export const skillsDiffResult = obj({
  clean: bool,
  modified: arr(any),
  new: arr(str),
  noLockfile: bool,
  removed: arr(any),
  repo: str,
  unchanged: num,
});
export type SkillsDiffResult = Infer<typeof skillsDiffResult>;

export const skillsEditBodyResult = obj({
  newHash: str,
  sourcePath: str,
});
export type SkillsEditBodyResult = Infer<typeof skillsEditBodyResult>;

export const skillsEditFrontmatterResult = obj({
  newHash: str,
  sourcePath: str,
});
export type SkillsEditFrontmatterResult = Infer<typeof skillsEditFrontmatterResult>;

export const skillsGetResult = obj({
  activation: str,
  body: str,
  description: str,
  name: str,
  path: str,
  type: str,
});
export type SkillsGetResult = Infer<typeof skillsGetResult>;

export const skillsGitPushResult = obj({
  commitSha: opt(str),
  message: opt(str),
  pushed: bool,
  repo: opt(str),
});
export type SkillsGitPushResult = Infer<typeof skillsGitPushResult>;

export const skillsInstallResult = obj({
  audit: obj({
    findings: arr(any),
    high: num,
    low: num,
    medium: num,
    ran: bool,
  }),
  blocked: bool,
  cloneUrl: str,
  discoveryWarnings: arr(any),
  dryRun: bool,
  foundCount: num,
  installedCount: num,
  skills: arr(any),
  skippedCount: num,
  spec: str,
  symlinksSkipped: arr(any),
  tap: str,
  tapAdded: bool,
  tapMissing: bool,
  target: str,
  version: nullable(str),
  versionIgnored: bool,
});
export type SkillsInstallResult = Infer<typeof skillsInstallResult>;

export const skillsLintResult = obj({
  errors: num,
  messages: arr(
    obj({
      level: str,
      message: str,
      name: str,
    }),
  ),
  warnings: num,
});
export type SkillsLintResult = Infer<typeof skillsLintResult>;

export const skillsListResult = obj({
  repo: str,
  skills: arr(
    obj({
      activation: str,
      description: str,
      name: str,
      path: str,
      type: str,
    }),
  ),
});
export type SkillsListResult = Infer<typeof skillsListResult>;

export const skillsListTranspilersResult = obj({
  transpilers: arr(str),
});
export type SkillsListTranspilersResult = Infer<typeof skillsListTranspilersResult>;

export const skillsResolveResult = obj({
  backupRoot: str,
  collisions: arr(any),
  dryRun: bool,
  kept: num,
  migrate: nullable(any),
  outputRoot: str,
  replaced: num,
  repo: str,
  scope: str,
  skillCount: num,
});
export type SkillsResolveResult = Infer<typeof skillsResolveResult>;

export const skillsScaffoldResult = obj({
  created_path: str,
  kind: str,
});
export type SkillsScaffoldResult = Infer<typeof skillsScaffoldResult>;

export const skillsSearchResult = obj({
  discoveryWarnings: arr(any),
  query: str,
  results: arr(
    obj({
      description: str,
      name: str,
      repo: str,
      tap: str,
      type: str,
    }),
  ),
  tapCount: num,
});
export type SkillsSearchResult = Infer<typeof skillsSearchResult>;

export const skillsStatusResult = obj({
  drift: bool,
  entries: arr(
    obj({
      clientsSynced: arr(str),
      currentHash: str,
      drifted: bool,
      knownToLockfile: bool,
      lockfileHash: nullable(str),
      name: str,
      type: str,
    }),
  ),
  lockedCount: num,
  lockfilePresent: bool,
  lockfileSyncedAt: nullable(str),
  outputRoot: nullable(str),
  outputs: arr(
    obj({
      client: str,
      name: str,
      present: bool,
    }),
  ),
  rejected: arr(any),
  repo: str,
  targetedClients: arr(str),
});
export type SkillsStatusResult = Infer<typeof skillsStatusResult>;

export const skillsSyncResult = obj({
  backupRoot: str,
  cleaned: arr(any),
  clientCount: num,
  collisions: arr(any),
  dryRun: bool,
  entries: arr(
    obj({
      clientsSynced: arr(str),
      name: str,
      type: str,
      warnings: arr(any),
    }),
  ),
  globalMode: bool,
  kept: num,
  outputRoot: str,
  replaced: num,
  repo: str,
  ruleCount: num,
  skillCount: num,
  syncedAt: str,
});
export type SkillsSyncResult = Infer<typeof skillsSyncResult>;

export const skillsTapAddResult = obj({
  cloned: bool,
  dryRun: bool,
  head: nullable(str),
  name: str,
  path: str,
  repo: str,
  url: str,
});
export type SkillsTapAddResult = Infer<typeof skillsTapAddResult>;

export const skillsTapListResult = obj({
  taps: arr(
    obj({
      cloned: bool,
      name: str,
      path: str,
      repo: str,
      url: str,
    }),
  ),
  tapsRoot: str,
});
export type SkillsTapListResult = Infer<typeof skillsTapListResult>;

export const skillsTapRemoveResult = obj({
  dryRun: bool,
  hadClone: bool,
  name: str,
  path: str,
  removed: bool,
});
export type SkillsTapRemoveResult = Infer<typeof skillsTapRemoveResult>;

export const skillsTapUpdateResult = obj({
  dryRun: bool,
  failed: num,
  results: arr(
    obj({
      error: nullable(str),
      head: nullable(str),
      name: str,
      ok: bool,
    }),
  ),
});
export type SkillsTapUpdateResult = Infer<typeof skillsTapUpdateResult>;

export const skillsUnbundleResult = obj({
  bundle: str,
  dryRun: bool,
  files: arr(str),
  names: arr(str),
  overwritten: arr(any),
  skipped: arr(any),
  target: str,
});
export type SkillsUnbundleResult = Infer<typeof skillsUnbundleResult>;

export const skillsUninstallResult = obj({
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
export type SkillsUninstallResult = Infer<typeof skillsUninstallResult>;

/** Tool name to the shape of its `structuredContent`. */
export const skillsToolShapes: Record<string, Shape<unknown>> = {
  skills_audit: skillsAuditResult,
  skills_bundle: skillsBundleResult,
  skills_clean: skillsCleanResult,
  skills_delete: skillsDeleteResult,
  skills_diff: skillsDiffResult,
  skills_edit_body: skillsEditBodyResult,
  skills_edit_frontmatter: skillsEditFrontmatterResult,
  skills_get: skillsGetResult,
  skills_git_push: skillsGitPushResult,
  skills_install: skillsInstallResult,
  skills_lint: skillsLintResult,
  skills_list: skillsListResult,
  skills_list_transpilers: skillsListTranspilersResult,
  skills_resolve: skillsResolveResult,
  skills_scaffold: skillsScaffoldResult,
  skills_search: skillsSearchResult,
  skills_status: skillsStatusResult,
  skills_sync: skillsSyncResult,
  skills_tap_add: skillsTapAddResult,
  skills_tap_list: skillsTapListResult,
  skills_tap_remove: skillsTapRemoveResult,
  skills_tap_update: skillsTapUpdateResult,
  skills_unbundle: skillsUnbundleResult,
  skills_uninstall: skillsUninstallResult,
};
