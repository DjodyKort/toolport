import type { Shape } from "../bridge/shape";
import { measureData } from "../bridge/data";

/** Results of the context self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

/** `context_measure` answers with the same report as `toolportctl context measure --json`. */
export const contextMeasureResult = measureData;

/** Tool name to the shape of its `structuredContent`. */
export const contextToolShapes: Record<string, Shape<unknown>> = {
  context_measure: contextMeasureResult,
};
