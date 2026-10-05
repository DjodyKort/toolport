import type { Shape } from "../bridge/shape";
import { selfToolCallData, type SelfToolCallData } from "./mcp-call-generic";
import {
  flowDiagramResult,
  whereAmIResult,
  type FlowDiagramResult,
  type WhereAmIResult,
} from "./selfmcp-core";

/** `data` of `toolportctl mcp call` for the tools of the System screen's Self-management tab,
 * checked against the golden envelopes by `data.test.ts`. */

export type WhereAmIData = SelfToolCallData<WhereAmIResult>;
export type FlowDiagramData = SelfToolCallData<FlowDiagramResult>;

/** Golden file stem to the shape of its envelope `data`. */
export const mcpCallSystemShapes: Record<string, Shape<unknown>> = {
  "mcp-call.where_am_i": selfToolCallData(whereAmIResult),
  "mcp-call.flow_diagram": selfToolCallData(flowDiagramResult),
};
