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
import { syncStatusData } from "../bridge/data";

/** `data` of the sync commands, checked against the golden envelopes by `data.test.ts`. */

export const syncAddProjectData = obj({
  name: str,
  replaced: bool,
});
export type SyncAddProjectData = Infer<typeof syncAddProjectData>;

export const syncDiffData = obj({
  changes: obj({
    conflicts: arr(any),
    modified: arr(any),
    new: arr(any),
    removed: arr(any),
    unchanged: arr(str),
  }),
  machineId: str,
  noRemote: bool,
});
export type SyncDiffData = Infer<typeof syncDiffData>;

export const syncGitSyncData = obj({
  autoSync: bool,
  branch: nullable(str),
  cleared: bool,
  cloned: bool,
  configured: bool,
  head: nullable(str),
  localPath: nullable(str),
  pulled: bool,
  repo: nullable(str),
});
export type SyncGitSyncData = Infer<typeof syncGitSyncData>;

export const syncInitData = obj({
  branch: str,
  freshRemote: bool,
  machineId: str,
});
export type SyncInitData = Infer<typeof syncInitData>;

export const syncPullData = obj({
  applied: arr(str),
  changes: nullable(
    obj({
      conflicts: arr(any),
      modified: arr(any),
      new: arr(any),
      removed: arr(any),
      unchanged: arr(str),
    }),
  ),
  conflicts: arr(any),
  dryRun: bool,
  keptLocal: arr(any),
  machineId: str,
  noRemote: bool,
  pushedAt: str,
  resolved: arr(any),
  skillsRepoChanged: num,
  skipped: arr(any),
});
export type SyncPullData = Infer<typeof syncPullData>;

export const syncPushData = obj({
  committed: bool,
  dryRun: bool,
  entries: arr(str),
  machineId: str,
  pushed: bool,
});
export type SyncPushData = Infer<typeof syncPushData>;

export const syncRemoveProjectData = obj({
  name: str,
  removed: bool,
});
export type SyncRemoveProjectData = Infer<typeof syncRemoveProjectData>;

export const syncResetData = obj({
  removed: arr(str),
});
export type SyncResetData = Infer<typeof syncResetData>;

export const syncRotatePassphraseData = obj({
  rotated: num,
  skipped: arr(any),
});
export type SyncRotatePassphraseData = Infer<typeof syncRotatePassphraseData>;

/** Golden file stem to the shape of its envelope `data`. */
export const syncShapes: Record<string, Shape<unknown>> = {
  "sync-add-project.after": syncStatusData,
  "sync-init.after": syncStatusData,
  "sync-remove-project.after": syncStatusData,
  "sync-reset.after": syncStatusData,
  "sync-add-project.apply": syncAddProjectData,
  "sync-diff.configured": syncDiffData,
  "sync-git-sync.clear": syncGitSyncData,
  "sync-git-sync.configure": syncGitSyncData,
  "sync-git-sync.status": syncGitSyncData,
  "sync-init.apply": syncInitData,
  "sync-pull.apply": syncPullData,
  "sync-pull.preview": syncPullData,
  "sync-push.apply": syncPushData,
  "sync-push.preview": syncPushData,
  "sync-remove-project.apply": syncRemoveProjectData,
  "sync-reset.apply": syncResetData,
  "sync-rotate-passphrase.apply": syncRotatePassphraseData,
};
