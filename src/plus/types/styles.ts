import { any, arr, bool, num, obj, str, type Infer, type Shape } from "../bridge/shape";

/** `data` of the styles commands, checked against the golden envelopes by `data.test.ts`. */

export const stylesAddData = obj({
  dryRun: bool,
  name: str,
  path: str,
  repo: str,
});
export type StylesAddData = Infer<typeof stylesAddData>;

export const stylesApplyData = obj({
  active: arr(
    obj({
      client: str,
      style: str,
    }),
  ),
  applied: arr(
    obj({
      client: str,
      path: str,
    }),
  ),
  appliedCount: num,
  discoveryWarnings: arr(any),
  dryRun: bool,
  lockDir: str,
  name: str,
  nativeClients: arr(any),
  outputRoot: str,
  replaced: arr(any),
  repo: str,
  scope: str,
});
export type StylesApplyData = Infer<typeof stylesApplyData>;

export const stylesCleanData = obj({
  cleanRoot: str,
  dryRun: bool,
  ignored: arr(any),
  lockDir: str,
  lockUpdated: bool,
  lockfilePresent: bool,
  managed: arr(str),
  removed: arr(str),
  scope: str,
  skipped: arr(any),
});
export type StylesCleanData = Infer<typeof stylesCleanData>;

export const stylesDiffData = obj({
  clean: bool,
  discoveryWarnings: arr(any),
  modified: arr(any),
  new: arr(str),
  noLockfile: bool,
  removed: arr(any),
  repo: str,
  unchanged: num,
});
export type StylesDiffData = Infer<typeof stylesDiffData>;

export const stylesLintData = obj({
  discoveryWarnings: arr(any),
  errors: num,
  infos: num,
  messages: arr(any),
  repo: str,
  styleCount: num,
  warnings: num,
});
export type StylesLintData = Infer<typeof stylesLintData>;

export const stylesRemoveData = obj({
  active: arr(any),
  clientKeys: arr(any),
  dryRun: bool,
  hadActive: bool,
  lockDir: str,
  outputRoot: str,
  removed: arr(
    obj({
      client: str,
      path: str,
      style: str,
    }),
  ),
  repo: str,
  scope: str,
});
export type StylesRemoveData = Infer<typeof stylesRemoveData>;

export const stylesStatusData = obj({
  applyRemove: arr(any),
  lockfilePresent: bool,
  native: arr(any),
  repo: str,
});
export type StylesStatusData = Infer<typeof stylesStatusData>;

export const stylesSyncData = obj({
  clientCount: num,
  discoveryWarnings: arr(any),
  dryRun: bool,
  foundCount: num,
  lockDir: str,
  outputRoot: str,
  outputs: arr(str),
  repo: str,
  scope: str,
  styleCount: num,
  styles: arr(
    obj({
      clientsSynced: arr(str),
      description: str,
      name: str,
      warnings: arr(any),
    }),
  ),
  syncedAt: str,
});
export type StylesSyncData = Infer<typeof stylesSyncData>;

/** Golden file stem to the shape of its envelope `data`. */
export const stylesShapes: Record<string, Shape<unknown>> = {
  "styles-add.apply": stylesAddData,
  "styles-add.preview": stylesAddData,
  "styles-apply.apply": stylesApplyData,
  "styles-apply.preview": stylesApplyData,
  "styles-clean.apply": stylesCleanData,
  "styles-clean.preview": stylesCleanData,
  "styles-diff": stylesDiffData,
  "styles-lint": stylesLintData,
  "styles-remove.apply": stylesRemoveData,
  "styles-remove.preview": stylesRemoveData,
  "styles-status": stylesStatusData,
  "styles-sync.apply": stylesSyncData,
  "styles-sync.preview": stylesSyncData,
};
