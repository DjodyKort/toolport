import type { Shape } from "../bridge/shape";
import { agentsShapes } from "./agents";
import { authShapes } from "./auth";
import { ccShapes } from "./cc";
import { clientShapes } from "./client";
import { compressionShapes } from "./compression";
import { contextShapes } from "./context";
import { contextBundleShapes } from "./context-bundle";
import { contextLayerShapes } from "./context-layers";
import { councilShapes } from "./council";
import { importShapes } from "./import";
import { inspectShapes } from "./inspect";
import { mcpShapes } from "./mcp";
import { obsShapes } from "./obs";
import { pluginsShapes } from "./plugins";
import { profileShapes } from "./profile";
import { secretShapes } from "./secret";
import { serverShapes } from "./server";
import { skillsShapes } from "./skills";
import { stylesShapes } from "./styles";
import { syncShapes } from "./sync";
import { taskShapes } from "./tasks";
import { usageShapes } from "./usage";

export * from "./agents";
export * from "./auth";
export * from "./cc";
export * from "./client";
export * from "./compression";
export * from "./context";
export * from "./context-bundle";
export * from "./context-layers";
export * from "./tasks";
export * from "./council";
export * from "./import";
export * from "./inspect";
export * from "./mcp";
export * from "./obs";
export * from "./plugins";
export * from "./profile";
export * from "./secret";
export * from "./server";
export * from "./skills";
export * from "./styles";
export * from "./sync";
export * from "./usage";
export * from "./selfmcp-agents";
export * from "./selfmcp-clients";
export * from "./selfmcp-compression";
export * from "./selfmcp-context";
export * from "./selfmcp-core";
export * from "./selfmcp-plugins";
export * from "./selfmcp-servers";
export * from "./selfmcp-skills";
export * from "./selfmcp-sources";
export * from "./selfmcp-styles";
export * from "./selfmcp-sync";
export * from "./selfmcp-resources";
export * from "./selfmcp";

/** Every ctl golden stem that `bridge/data.ts` does not describe, to the shape of its `data`. */
export const ctlTypeShapes: Record<string, Shape<unknown>> = {
  ...agentsShapes,
  ...authShapes,
  ...ccShapes,
  ...clientShapes,
  ...compressionShapes,
  ...contextShapes,
  ...contextBundleShapes,
  ...contextLayerShapes,
  ...councilShapes,
  ...importShapes,
  ...inspectShapes,
  ...mcpShapes,
  ...obsShapes,
  ...pluginsShapes,
  ...profileShapes,
  ...secretShapes,
  ...serverShapes,
  ...skillsShapes,
  ...stylesShapes,
  ...syncShapes,
  ...taskShapes,
  ...usageShapes,
};
