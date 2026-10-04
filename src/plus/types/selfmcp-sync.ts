import { any, arr, bool, obj, str, type Infer, type Shape } from "../bridge/shape";

/** Results of the sync self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

export const syncPushResult = obj({
  committed: bool,
  dryRun: bool,
  entries: arr(any),
  machineId: str,
  pushed: bool,
});
export type SyncPushResult = Infer<typeof syncPushResult>;

/** Tool name to the shape of its `structuredContent`. */
export const syncToolShapes: Record<string, Shape<unknown>> = {
  sync_push: syncPushResult,
};
