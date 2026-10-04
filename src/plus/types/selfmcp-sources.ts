import type { Shape } from "../bridge/shape";
import { sourcesLsData } from "../bridge/data";

/** Results of the sources self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

/** `sources_ls` answers with the same report as `toolportctl sources ls --json`. */
export const sourcesLsResult = sourcesLsData;

/** Tool name to the shape of its `structuredContent`. */
export const sourcesToolShapes: Record<string, Shape<unknown>> = {
  sources_ls: sourcesLsResult,
};
