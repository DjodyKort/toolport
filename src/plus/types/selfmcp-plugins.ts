import type { Shape } from "../bridge/shape";
import { hooksLsData, pluginsLsData, pluginsShowData } from "./plugins";

/** Results of the plugin and hook self-MCP tools, checked against the golden results by
 * `selfmcp.test.ts`. Each answers with the same report as its `toolportctl` command. */

/** Tool name to the shape of its `structuredContent`. */
export const pluginsToolShapes: Record<string, Shape<unknown>> = {
  plugins_ls: pluginsLsData,
  plugins_show: pluginsShowData,
  hooks_ls: hooksLsData,
};
