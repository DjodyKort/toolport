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

/** `data` of the skills commands, checked against the golden envelopes by `data.test.ts`. */

export const skillsAddData = obj({
  dryRun: bool,
  files: arr(str),
  name: str,
  path: str,
  progressive: bool,
  repo: str,
  type: str,
});
export type SkillsAddData = Infer<typeof skillsAddData>;

export const skillsAuditData = obj({
  clean: bool,
  findings: arr(any),
  high: num,
  low: num,
  medium: num,
  repo: str,
  skillCount: num,
});
export type SkillsAuditData = Infer<typeof skillsAuditData>;

export const skillsBundleData = obj({
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
export type SkillsBundleData = Infer<typeof skillsBundleData>;

export const skillsCleanData = obj({
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
export type SkillsCleanData = Infer<typeof skillsCleanData>;

export const skillsDiffData = obj({
  clean: bool,
  lockfile: nullable(str),
  modified: arr(any),
  new: arr(str),
  noLockfile: bool,
  removed: arr(any),
  repo: str,
  unchanged: num,
  warnings: arr(str),
});
export type SkillsDiffData = Infer<typeof skillsDiffData>;

export const skillsInitData = obj({
  alreadyExists: bool,
  created: arr(str),
  dryRun: bool,
  name: str,
  repo: str,
});
export type SkillsInitData = Infer<typeof skillsInitData>;

export const skillsInstallData = obj({
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
export type SkillsInstallData = Infer<typeof skillsInstallData>;

export const skillsResolveData = obj({
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
export type SkillsResolveData = Infer<typeof skillsResolveData>;

export const skillsSearchData = obj({
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
export type SkillsSearchData = Infer<typeof skillsSearchData>;

export const skillsStatusData = obj({
  drift: bool,
  entries: arr(
    obj({
      clientsSynced: arr(any),
      currentHash: str,
      drifted: bool,
      knownToLockfile: bool,
      lockfileHash: nullable(str),
      name: str,
      type: str,
    }),
  ),
  lockedCount: num,
  lockfile: nullable(str),
  lockfilePresent: bool,
  lockfileSyncedAt: nullable(str),
  outputRoot: nullable(str),
  outputs: arr(any),
  rejected: arr(any),
  repo: str,
  targetedClients: arr(str),
  warnings: arr(str),
});
export type SkillsStatusData = Infer<typeof skillsStatusData>;

export const skillsTapLsData = obj({
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
export type SkillsTapLsData = Infer<typeof skillsTapLsData>;

export const skillsTapAddData = obj({
  cloned: bool,
  dryRun: bool,
  head: nullable(str),
  name: str,
  path: str,
  repo: str,
  url: str,
});
export type SkillsTapAddData = Infer<typeof skillsTapAddData>;

export const skillsTapRemoveData = obj({
  dryRun: bool,
  hadClone: bool,
  name: str,
  path: str,
  removed: bool,
});
export type SkillsTapRemoveData = Infer<typeof skillsTapRemoveData>;

export const skillsTapUpdateData = obj({
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
export type SkillsTapUpdateData = Infer<typeof skillsTapUpdateData>;

export const skillsUnbundleData = obj({
  bundle: str,
  dryRun: bool,
  files: arr(str),
  names: arr(str),
  overwritten: arr(any),
  skipped: arr(any),
  target: str,
});
export type SkillsUnbundleData = Infer<typeof skillsUnbundleData>;

export const skillsUninstallData = obj({
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
export type SkillsUninstallData = Infer<typeof skillsUninstallData>;

/** Golden file stem to the shape of its envelope `data`. */
export const skillsShapes: Record<string, Shape<unknown>> = {
  "skills-add.apply": skillsAddData,
  "skills-add.preview": skillsAddData,
  "skills-audit": skillsAuditData,
  "skills-bundle.apply": skillsBundleData,
  "skills-bundle.preview": skillsBundleData,
  "skills-clean.apply": skillsCleanData,
  "skills-clean.preview": skillsCleanData,
  "skills-diff": skillsDiffData,
  "skills-init.apply": skillsInitData,
  "skills-init.preview": skillsInitData,
  "skills-install.preview": skillsInstallData,
  "skills-resolve.apply": skillsResolveData,
  "skills-resolve.preview": skillsResolveData,
  "skills-search.empty": skillsSearchData,
  "skills-search.hit": skillsSearchData,
  "skills-status": skillsStatusData,
  "skills-tap-add.after": skillsTapLsData,
  "skills-tap-ls": skillsTapLsData,
  "skills-tap-add.apply": skillsTapAddData,
  "skills-tap-add.preview": skillsTapAddData,
  "skills-tap-remove.apply": skillsTapRemoveData,
  "skills-tap-remove.preview": skillsTapRemoveData,
  "skills-tap-update.apply": skillsTapUpdateData,
  "skills-tap-update.none": skillsTapUpdateData,
  "skills-tap-update.preview": skillsTapUpdateData,
  "skills-unbundle.apply": skillsUnbundleData,
  "skills-unbundle.preview": skillsUnbundleData,
  "skills-uninstall.apply": skillsUninstallData,
  "skills-uninstall.preview": skillsUninstallData,
};
