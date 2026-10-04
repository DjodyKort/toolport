import {
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
import { planV1, resultV1 } from "../bridge/data";

/** `data` of `context bundle *` and `context use` (D-064, D-066), checked against the golden
 * envelopes by `data.test.ts`. The `context_bundle_*` tools answer with the same objects. */

const bundleIssue = obj({ key: str, level: str, message: str });

const appliedTo = arr(obj({ appliedAt: str, drift: bool, folder: str }));

/** `apply`, `undo` and `use` return the plan, then the result with the path of the ledger. */
const ledgerResult = obj({
  applied: bool,
  backups: arr(str),
  changed: arr(str),
  ledger: str,
  undo: str,
});

export const contextBundleLsData = obj({
  bundles: arr(
    obj({
      agents: obj({ off: arr(str) }),
      appliedTo,
      bind: arr(str),
      description: str,
      error: opt(str),
      issues: num,
      layers: obj({ add: arr(str), exclude: arr(str) }),
      name: str,
      path: str,
      plugins: obj({ off: arr(str) }),
      servers: nullable(str),
      skills: obj({ allow: num, nameOnly: num, off: num }),
    }),
  ),
  directory: str,
});
export type ContextBundleLsData = Infer<typeof contextBundleLsData>;

export const contextBundleShowData = obj({
  agents: obj({ off: arr(str) }),
  appliedTo,
  bind: arr(str),
  description: str,
  issues: arr(bundleIssue),
  layers: obj({ add: arr(str), exclude: arr(str) }),
  legacy: bool,
  name: str,
  path: str,
  plugins: obj({ off: arr(str) }),
  servers: nullable(str),
  skills: obj({ allow: arr(str), nameOnly: arr(str), off: arr(str) }),
  yaml: str,
});
export type ContextBundleShowData = Infer<typeof contextBundleShowData>;

/** `bundle add` and `bundle edit`: the plan writes the yaml in the skills repository. */
export const contextBundleSaveData = obj({
  created: bool,
  dryRun: bool,
  issues: arr(bundleIssue),
  name: str,
  path: str,
  plan: planV1,
  result: nullable(resultV1),
});
export type ContextBundleSaveData = Infer<typeof contextBundleSaveData>;

export const contextBundleRmData = obj({
  dryRun: bool,
  name: str,
  path: str,
  plan: planV1,
  result: nullable(resultV1),
});
export type ContextBundleRmData = Infer<typeof contextBundleRmData>;

/** `bundle apply` and `bundle undo`; `conflicts` lists the keys that changed since the apply. */
export const contextBundleApplyData = obj({
  bundle: str,
  conflicts: arr(str),
  cwd: str,
  dryRun: bool,
  plan: planV1,
  result: nullable(ledgerResult),
});
export type ContextBundleApplyData = Infer<typeof contextBundleApplyData>;

export const contextBundleStatusData = obj({
  applied: nullable(
    obj({
      appliedAt: str,
      bundle: str,
      changedKeys: arr(str),
      drift: bool,
      ownedKeys: obj({
        claudeMdExcludes: arr(str),
        enabledPlugins: arr(str),
        permissionsDeny: arr(str),
        skillOverrides: arr(str),
      }),
    }),
  ),
  conflicts: arr(str),
  folder: str,
});
export type ContextBundleStatusData = Infer<typeof contextBundleStatusData>;

export const contextBundleLaunchData = obj({
  bundle: str,
  command: str,
  cwd: nullable(str),
  notes: arr(str),
  settingsFile: str,
});
export type ContextBundleLaunchData = Infer<typeof contextBundleLaunchData>;

export const contextBundleConfigData = obj({ autoApply: bool, file: str });
export type ContextBundleConfigData = Infer<typeof contextBundleConfigData>;

/** `context use <name>`: the bundle part is absent when only a server profile has the name. */
export const contextUseData = obj({
  bundle: str,
  bundlePart: bool,
  conflicts: arr(str),
  cwd: str,
  dryRun: bool,
  name: str,
  plan: planV1,
  result: nullable(
    obj({
      applied: bool,
      backups: arr(str),
      changed: arr(str),
      ledger: opt(str),
      undo: str,
    }),
  ),
  server: obj({ bound: bool, foldersEnabled: bool, profile: nullable(str) }),
});
export type ContextUseData = Infer<typeof contextUseData>;

export const contextUseNoneData = obj({
  bundle: nullable(str),
  conflicts: arr(str),
  cwd: str,
  dryRun: bool,
  plan: planV1,
  result: nullable(
    obj({
      applied: bool,
      backups: arr(str),
      changed: arr(str),
      ledger: opt(str),
      undo: str,
    }),
  ),
  server: obj({ unrouted: bool }),
});
export type ContextUseNoneData = Infer<typeof contextUseNoneData>;

/** What `context sync` adds when `bundle config --auto-apply on` is set. */
export const contextSyncBundles = arr(
  obj({ applied: bool, bundle: str, error: opt(str), folder: str }),
);

/** Golden file stem to the shape of its envelope `data`. */
export const contextBundleShapes: Record<string, Shape<unknown>> = {
  "context-bundle-add.apply": contextBundleSaveData,
  "context-bundle-add.from-folder": contextBundleSaveData,
  "context-bundle-add.preview": contextBundleSaveData,
  "context-bundle-add.show": contextBundleShowData,
  "context-bundle-apply.again": contextBundleApplyData,
  "context-bundle-apply.legacy": contextBundleApplyData,
  "context-bundle-apply.plan": contextBundleApplyData,
  "context-bundle-apply.result": contextBundleApplyData,
  "context-bundle-config.after": contextBundleConfigData,
  "context-bundle-config.off": contextBundleConfigData,
  "context-bundle-config.on": contextBundleConfigData,
  "context-bundle-config.show": contextBundleConfigData,
  "context-bundle-edit.apply": contextBundleSaveData,
  "context-bundle-edit.preview": contextBundleSaveData,
  "context-bundle-edit.show": contextBundleShowData,
  "context-bundle-launch.apply": contextBundleLaunchData,
  "context-bundle-launch.legacy": contextBundleLaunchData,
  "context-bundle-ls.applied": contextBundleLsData,
  "context-bundle-ls.library": contextBundleLsData,
  "context-bundle-rm.apply": contextBundleRmData,
  "context-bundle-rm.forced": contextBundleRmData,
  "context-bundle-rm.preview": contextBundleRmData,
  "context-bundle-show.bundle": contextBundleShowData,
  "context-bundle-show.legacy": contextBundleShowData,
  "context-bundle-status.clean": contextBundleStatusData,
  "context-bundle-status.default": contextBundleStatusData,
  "context-bundle-status.drift": contextBundleStatusData,
  "context-bundle-status.none": contextBundleStatusData,
  "context-bundle-undo.conflicted": contextBundleApplyData,
  "context-bundle-undo.conflicts": contextBundleApplyData,
  "context-bundle-undo.plan": contextBundleApplyData,
  "context-bundle-undo.result": contextBundleApplyData,
  "context-use.bundle": contextUseData,
  "context-use.none": contextUseNoneData,
  "context-use.none-apply": contextUseNoneData,
  "context-use.off": contextUseData,
  "context-use.routed": contextUseData,
  "context-use.routed-apply": contextUseData,
};
