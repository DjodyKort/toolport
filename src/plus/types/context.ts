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
import { loadsData, type LoadsData } from "../bridge/data";
import { contextSyncBundles } from "./context-bundle";

/** `data` of the context commands, checked against the golden envelopes by `data.test.ts`. */

export const contextApplyData = obj({
  actions: arr(str),
  checks: arr(arr(str)),
  dryRun: bool,
  warnings: arr(any),
});
export type ContextApplyData = Infer<typeof contextApplyData>;

export const contextCheckpointStatusData = obj({
  at_checkpoint: bool,
  checkpoint_at: num,
  checkpoint_point: num,
  remaining_to_checkpoint: num,
  remaining_to_compact: num,
  used_tokens: num,
  window: num,
  window_source: str,
});
export type ContextCheckpointStatusData = Infer<typeof contextCheckpointStatusData>;

export const contextDisableData = obj({
  actions: arr(str),
  dryRun: bool,
  purgedProfiles: arr(str),
  shims: obj({
    path: str,
    removed: bool,
  }),
  warnings: arr(any),
});
export type ContextDisableData = Infer<typeof contextDisableData>;

export const contextFoldersData = obj({
  enabled: bool,
  folders: arr(
    obj({
      applies: bool,
      launchProfile: nullable(any),
      profile: nullable(str),
      reason: str,
      root: str,
      rule: nullable(str),
      tokens: num,
      wouldApply: nullable(any),
    }),
  ),
  mappings: arr(any),
});
export type ContextFoldersData = Infer<typeof contextFoldersData>;

export const contextInitData = obj({
  config: obj({
    keptUnreadable: nullable(any),
    path: str,
    saved: bool,
  }),
  dryRun: bool,
  migration: nullable(any),
  nextSteps: arr(
    obj({
      command: str,
      label: str,
    }),
  ),
  personal: obj({
    created: bool,
    path: str,
  }),
});
export type ContextInitData = Infer<typeof contextInitData>;

/** `context loads` is described once, in `bridge/data.ts`, which the GUI parses. */
export const contextLoadsData = loadsData;
export type ContextLoadsData = LoadsData;

export const contextPlanData = obj({
  actions: arr(str),
  checks: arr(arr(str)),
  dryRun: bool,
  warnings: arr(any),
});
export type ContextPlanData = Infer<typeof contextPlanData>;

export const contextProfileAddData = obj({
  actions: arr(str),
  dryRun: bool,
  profile: obj({
    created: bool,
    dir: str,
    generated: bool,
    name: str,
    org: bool,
    orgMode: str,
    rules: str,
    servers: str,
    shim: str,
  }),
  warnings: arr(any),
});
export type ContextProfileAddData = Infer<typeof contextProfileAddData>;

export const contextProfileListData = obj({
  profiles: arr(any),
});
export type ContextProfileListData = Infer<typeof contextProfileListData>;

export const contextProfileRemoveData = obj({
  actions: arr(str),
  dryRun: bool,
  inConfig: bool,
  name: str,
  purged: bool,
  warnings: arr(any),
});
export type ContextProfileRemoveData = Infer<typeof contextProfileRemoveData>;

export const contextStatusData = obj({
  config: obj({
    exists: bool,
    path: str,
  }),
  layers: arr(any),
  legacyDupes: arr(any),
  profiles: arr(any),
  shims: obj({
    exists: bool,
    legacyExists: bool,
    legacyPath: str,
    path: str,
  }),
  zshrc: obj({
    deadAliases: arr(any),
    exists: bool,
    legacyLines: arr(any),
    path: str,
  }),
});
export type ContextStatusData = Infer<typeof contextStatusData>;

export const contextSyncData = obj({
  bundles: opt(contextSyncBundles),
  apply: nullable(
    obj({
      actions: arr(str),
      checks: arr(arr(str)),
      dryRun: bool,
      warnings: arr(any),
    }),
  ),
  dryRun: bool,
  plan: obj({
    actions: arr(str),
    checks: arr(arr(str)),
    dryRun: bool,
    warnings: arr(any),
  }),
});
export type ContextSyncData = Infer<typeof contextSyncData>;

/** Golden file stem to the shape of its envelope `data`. */
export const contextShapes: Record<string, Shape<unknown>> = {
  "context-apply.apply": contextApplyData,
  "context-apply.preview": contextApplyData,
  "context-checkpoint-status.status": contextCheckpointStatusData,
  "context-disable.apply": contextDisableData,
  "context-disable.preview": contextDisableData,
  "context-folders": contextFoldersData,
  "context-init.apply": contextInitData,
  "context-init.preview": contextInitData,
  "context-loads.home": contextLoadsData,
  "context-plan": contextPlanData,
  "context-profile-add.apply": contextProfileAddData,
  "context-profile-add.preview": contextProfileAddData,
  "context-profile-list": contextProfileListData,
  "context-profile-remove.apply": contextProfileRemoveData,
  "context-profile-remove.preview": contextProfileRemoveData,
  "context-status": contextStatusData,
  "context-sync.apply": contextSyncData,
  "context-sync.preview": contextSyncData,
};
