import type { Shape } from "../bridge/shape";
import { measureData } from "../bridge/data";
import {
  contextBundleApplyData,
  contextBundleLsData,
  contextBundleStatusData,
} from "./context-bundle";

/** Results of the context self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

/** `context_measure` answers with the same report as `toolportctl context measure --json`. */
export const contextMeasureResult = measureData;

/** The `context_bundle_*` tools answer with the data of `toolportctl context bundle *`. */
export const contextBundleLsResult = contextBundleLsData;
export const contextBundleStatusResult = contextBundleStatusData;
export const contextBundleApplyResult = contextBundleApplyData;
export const contextBundleUndoResult = contextBundleApplyData;

/** Tool name to the shape of its `structuredContent`. */
export const contextToolShapes: Record<string, Shape<unknown>> = {
  context_bundle_apply: contextBundleApplyResult,
  context_bundle_ls: contextBundleLsResult,
  context_bundle_status: contextBundleStatusResult,
  context_bundle_undo: contextBundleUndoResult,
  context_measure: contextMeasureResult,
};
