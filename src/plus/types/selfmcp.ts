import {
  any,
  bool,
  lit,
  obj,
  opt,
  rec,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";
import { agentsToolShapes } from "./selfmcp-agents";
import { clientsToolShapes } from "./selfmcp-clients";
import { compressionToolShapes } from "./selfmcp-compression";
import { contextToolShapes } from "./selfmcp-context";
import { coreToolShapes } from "./selfmcp-core";
import { pluginsToolShapes } from "./selfmcp-plugins";
import { serversToolShapes } from "./selfmcp-servers";
import { skillsToolShapes } from "./selfmcp-skills";
import { sourcesToolShapes } from "./selfmcp-sources";
import { stylesToolShapes } from "./selfmcp-styles";
import { syncToolShapes } from "./selfmcp-sync";

/** What `tools/call` answers with when a tool fails: `structuredContent` of an error result. */
export const toolErrorShape = obj({
  error: obj({
    kind: lit(
      "backend_error",
      "conflict",
      "invalid_arguments",
      "invalid_input",
      "not_found",
      "refused",
      "registry_error",
      "unknown_resource",
      "unknown_tool",
    ),
    message: str,
  }),
});
export type ToolError = Infer<typeof toolErrorShape>;

/** One golden of a tool: the call, and the result with the error text only when it failed. */
export const toolGoldenShape = obj({
  tool: str,
  arguments: rec(any),
  isError: bool,
  result: any,
  text: opt(str),
});

/** One golden of a resource: its uri and media type, and the parsed JSON or the text. */
export const resourceGoldenShape = obj({
  uri: str,
  mimeType: lit("application/json", "text/markdown", "text/plain"),
  json: opt(any),
  text: opt(str),
});

/** Every self-MCP tool to the shape of its successful `structuredContent`. */
export const selfmcpToolShapes: Record<string, Shape<unknown>> = {
  ...agentsToolShapes,
  ...clientsToolShapes,
  ...compressionToolShapes,
  ...contextToolShapes,
  ...coreToolShapes,
  ...pluginsToolShapes,
  ...serversToolShapes,
  ...skillsToolShapes,
  ...sourcesToolShapes,
  ...stylesToolShapes,
  ...syncToolShapes,
};
