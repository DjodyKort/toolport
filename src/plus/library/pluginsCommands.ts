import { ctlData } from "../bridge/ctl";
import type {
  HooksLsData,
  PluginConfigData,
  PluginMcpData,
  PluginShowData,
  PluginsLsData,
  PluginWriteData,
} from "./pluginsData";

const dry = (dryRun: boolean) => (dryRun ? ["--dry-run"] : []);
const inFolder = (cwd: string | null | undefined) => (cwd ? ["--cwd", cwd] : []);

export const listPlugins = (cwd?: string | null, refresh = false) =>
  ctlData<PluginsLsData>([
    "plugins",
    "ls",
    ...inFolder(cwd),
    ...(refresh ? ["--refresh"] : []),
  ]);

export const showPlugin = (id: string, cwd?: string | null) =>
  ctlData<PluginShowData>(["plugins", "show", id, ...inFolder(cwd)]);

export const listHooks = (cwd?: string | null) =>
  ctlData<HooksLsData>(["hooks", "ls", ...inFolder(cwd)]);

export const pluginConfig = (id: string, cwd?: string | null) =>
  ctlData<PluginConfigData>(["plugins", "config", id, ...inFolder(cwd)]);

export interface KnobChange {
  set: Record<string, string>;
  unset: string[];
}

export const changeKnobs = (
  id: string,
  cwd: string | null,
  change: KnobChange,
  dryRun: boolean,
) =>
  ctlData<PluginWriteData>([
    "plugins",
    "config",
    id,
    ...inFolder(cwd),
    ...Object.entries(change.set).flatMap(([knob, value]) => [
      "--set",
      `${knob}=${value}`,
    ]),
    ...change.unset.flatMap((knob) => ["--unset", knob]),
    ...dry(dryRun),
  ]);

export const pluginMcp = (id: string, cwd?: string | null) =>
  ctlData<PluginMcpData>(["plugins", "mcp", "ls", id, ...inFolder(cwd)]);

export const switchMcp = (
  action: "deny" | "allow",
  id: string,
  server: string,
  cwd: string,
  dryRun: boolean,
) =>
  ctlData<PluginWriteData>([
    "plugins",
    "mcp",
    action,
    id,
    server,
    "--cwd",
    cwd,
    ...dry(dryRun),
  ]);

/** `claude plugin disable|enable|update` for the user, after a plan. */
export const userScope = (
  action: "disable" | "enable" | "update",
  id: string,
  dryRun: boolean,
) => ctlData<PluginWriteData>(["plugins", action, id, ...dry(dryRun)]);
