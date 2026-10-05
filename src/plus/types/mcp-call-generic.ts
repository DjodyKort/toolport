import { bool, num, obj, str, type Shape } from "../bridge/shape";
import { whereAmIResult } from "./selfmcp-core";

/** `data` of `toolportctl mcp call <tool>`: the tool's own `structuredContent` as `result`,
 * with the tool name and tier it ran at. A tool that fails is not this shape: the envelope
 * itself fails, with the tool's error kind as its code. */
export interface SelfToolCallData<T> {
  isError: boolean;
  result: T;
  tier: number;
  tool: string;
}

export function selfToolCallData<T>(result: Shape<T>): Shape<SelfToolCallData<T>> {
  return obj({ isError: bool, result, tier: num, tool: str }) as Shape<
    SelfToolCallData<T>
  >;
}

/** Golden file stem to the shape of its envelope `data`. `--args-stdin` answers exactly like
 * `--args`, so the one golden of it is a `where_am_i` call. */
export const mcpCallGenericShapes: Record<string, Shape<unknown>> = {
  "mcp-call.stdin": selfToolCallData(whereAmIResult),
};
