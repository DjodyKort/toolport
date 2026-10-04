import {
  any,
  arr,
  bool,
  nullable,
  num,
  obj,
  rec,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** Results of the styles self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

export const stylesActiveResult = obj({
  activeStyles: rec(str),
  lockfilePresent: bool,
  repo: str,
});
export type StylesActiveResult = Infer<typeof stylesActiveResult>;

export const stylesApplyResult = obj({
  activeStyles: rec(str),
  applied: str,
  dryRun: bool,
});
export type StylesApplyResult = Infer<typeof stylesApplyResult>;

export const stylesCleanResult = obj({
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
export type StylesCleanResult = Infer<typeof stylesCleanResult>;

export const stylesDiffResult = obj({
  clean: bool,
  discoveryWarnings: arr(any),
  modified: arr(any),
  new: arr(str),
  noLockfile: bool,
  removed: arr(any),
  repo: str,
  unchanged: num,
});
export type StylesDiffResult = Infer<typeof stylesDiffResult>;

export const stylesEditBodyResult = obj({
  newHash: str,
  sourcePath: str,
});
export type StylesEditBodyResult = Infer<typeof stylesEditBodyResult>;

export const stylesGetResult = obj({
  body: str,
  description: str,
  keepCodingInstructions: bool,
  name: str,
  path: str,
});
export type StylesGetResult = Infer<typeof stylesGetResult>;

export const stylesLintResult = obj({
  errors: num,
  messages: arr(any),
  warnings: num,
});
export type StylesLintResult = Infer<typeof stylesLintResult>;

export const stylesListResult = obj({
  repo: str,
  styles: arr(
    obj({
      description: str,
      keepCodingInstructions: bool,
      name: str,
      path: str,
    }),
  ),
});
export type StylesListResult = Infer<typeof stylesListResult>;

export const stylesListTranspilersResult = obj({
  tier1: arr(str),
  tier2: arr(str),
});
export type StylesListTranspilersResult = Infer<typeof stylesListTranspilersResult>;

export const stylesRemoveResult = obj({
  activeStyles: rec(str),
  dryRun: bool,
});
export type StylesRemoveResult = Infer<typeof stylesRemoveResult>;

export const stylesScaffoldResult = obj({
  created_path: str,
  kind: str,
});
export type StylesScaffoldResult = Infer<typeof stylesScaffoldResult>;

export const stylesStatusResult = obj({
  applyRemove: arr(
    obj({
      active: nullable(any),
      client: str,
      name: str,
    }),
  ),
  lockfilePresent: bool,
  native: arr(
    obj({
      client: str,
      name: str,
      styles: arr(str),
    }),
  ),
  repo: str,
});
export type StylesStatusResult = Infer<typeof stylesStatusResult>;

export const stylesSyncTier1Result = obj({
  dryRun: bool,
  repo: str,
  styleCount: num,
  syncedAt: str,
});
export type StylesSyncTier1Result = Infer<typeof stylesSyncTier1Result>;

/** Tool name to the shape of its `structuredContent`. */
export const stylesToolShapes: Record<string, Shape<unknown>> = {
  styles_active: stylesActiveResult,
  styles_apply: stylesApplyResult,
  styles_clean: stylesCleanResult,
  styles_diff: stylesDiffResult,
  styles_edit_body: stylesEditBodyResult,
  styles_get: stylesGetResult,
  styles_lint: stylesLintResult,
  styles_list: stylesListResult,
  styles_list_transpilers: stylesListTranspilersResult,
  styles_remove: stylesRemoveResult,
  styles_scaffold: stylesScaffoldResult,
  styles_status: stylesStatusResult,
  styles_sync_tier1: stylesSyncTier1Result,
};
