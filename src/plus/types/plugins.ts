import {
  any,
  arr,
  bool,
  lit,
  nullable,
  num,
  obj,
  opt,
  rec,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";
import { planV1, resultV1 } from "../bridge/data";

/** `data` of `plugins ls`, `plugins show` and `hooks ls`, checked against the golden envelopes by
 * `data.test.ts`. The reader prefers `claude plugin ...` and falls back to the files; a figure
 * only the CLI knows is `null` then and `from` says which one answered. */

const tokens = obj({ basis: lit("projected", "measured"), value: num });

const enabled = obj({
  user: nullable(bool),
  project: nullable(bool),
  local: nullable(bool),
  effective: bool,
});

export const hookTool = lit("Bash", "Edit", "Write", "Read");
export const hookOwnerKind = lit(
  "plugin",
  "user",
  "project",
  "local",
  "skill",
  "toolport",
  "managed",
);

export const hookEntry = obj({
  owner: obj({ kind: hookOwnerKind, name: str }),
  event: str,
  matcher: str,
  type: str,
  command: str,
  runtimeId: nullable(str),
  timeoutSec: nullable(num),
  async: bool,
  source: str,
  marker: nullable(str),
  switch: obj({
    method: lit(
      "toolport-command",
      "skills-sync",
      "edit-settings",
      "disable-plugin",
      "none",
    ),
    detail: str,
  }),
  tools: arr(hookTool),
  active: bool,
});
export type HookEntry = Infer<typeof hookEntry>;

const rowProps = {
  id: str,
  name: str,
  marketplace: str,
  version: str,
  installPath: str,
  source: str,
  enabled,
  update: obj({
    state: lit("update", "current", "unknown"),
    available: nullable(str),
  }),
  brings: obj({
    skills: num,
    agents: num,
    commands: num,
    hooks: num,
    mcpServers: num,
    lspServers: num,
  }),
  cost: obj({ projected: nullable(tokens), measured: nullable(tokens) }),
  /** `null` until the adapter row of `plugins config` exists. */
  adapter: any,
  mcpOutsideGateway: arr(obj({ key: str, name: str, denied: bool })),
  from: lit("claude-cli", "files"),
};

export const pluginRow = obj(rowProps);
export type PluginRow = Infer<typeof pluginRow>;

export const pluginsLsData = obj({
  plugins: arr(pluginRow),
  refreshError: nullable(str),
  restartRequired: bool,
  partial: bool,
  warnings: arr(str),
});
export type PluginsLsData = Infer<typeof pluginsLsData>;

export const pluginOption = obj({
  key: str,
  title: str,
  description: str,
  type: str,
  default: any,
  current: any,
  choices: nullable(arr(str)),
  sensitive: bool,
  configured: bool,
});
export type PluginOption = Infer<typeof pluginOption>;

export const adapterKnob = obj({
  key: str,
  label: str,
  kind: lit("enum", "bool", "bool-off", "csv", "globs", "hook-ids"),
  env: str,
  option: nullable(str),
  choices: nullable(arr(str)),
  default: nullable(str),
  current: obj({
    value: nullable(str),
    from: lit("folder-env", "user-env", "option", "default"),
  }),
});
export type AdapterKnob = Infer<typeof adapterKnob>;

export const pluginsShowData = obj({
  ...rowProps,
  description: str,
  components: obj({
    skills: arr(str),
    agents: arr(str),
    commands: arr(str),
    lspServers: arr(str),
  }),
  hooks: arr(hookEntry),
  mcpServers: arr(
    obj({
      name: str,
      key: str,
      command: nullable(arr(str)),
      url: nullable(str),
      toolPrefix: str,
      denied: obj({ user: bool, project: bool, local: bool, managed: bool }),
    }),
  ),
  options: arr(pluginOption),
  knobs: arr(adapterKnob),
  adapterProblems: arr(obj({ file: str, message: str })),
  warnings: arr(str),
});
export type PluginsShowData = Infer<typeof pluginsShowData>;

/** `plugins config` and `plugins mcp`: the plan, then the result once applied. A folder write
 * goes through the bundle ledger (`ledger` is its path); a global option change does not. */
const controlProps = {
  id: str,
  scope: lit("folder", "global"),
  cwd: nullable(str),
  dryRun: bool,
  plan: planV1,
  result: nullable(resultV1),
  conflicts: arr(str),
  ledger: opt(str),
};

export const knobChange = obj({
  knob: str,
  action: lit("set", "remove", "unset"),
  /** The environment variable of a folder write. */
  env: opt(str),
  /** The plugin option of a global write. */
  option: opt(str),
  value: nullable(str),
});
export type KnobChange = Infer<typeof knobChange>;

export const pluginsConfigData = obj({
  ...controlProps,
  adapter: str,
  changes: arr(knobChange),
});
export type PluginsConfigData = Infer<typeof pluginsConfigData>;

export const pluginsMcpData = obj({
  ...controlProps,
  server: str,
  /** The `deniedMcpServers` entry, `plugin:<plugin>:<server>`. */
  serverName: str,
  toolPrefix: str,
});
export type PluginsMcpData = Infer<typeof pluginsMcpData>;

/** `plugins off|on` (a folder, through the bundle ledger) and `plugins disable|enable` (user scope,
 * through Claude Code). Both answer with the plan first and the result once applied. */
const switchProps = {
  id: str,
  dryRun: bool,
  plan: planV1,
  result: nullable(resultV1),
  conflicts: arr(str),
};

export const folderSwitchChange = obj({
  /** The settings key, `enabledPlugins.<id>`. `restore` puts back a value the user had. */
  key: str,
  action: lit("set", "remove", "restore", "none"),
  value: nullable(bool),
});
export type FolderSwitchChange = Infer<typeof folderSwitchChange>;

export const pluginsFolderSwitchData = obj({
  ...switchProps,
  scope: lit("folder"),
  cwd: str,
  changes: arr(folderSwitchChange),
  /** `null` when nothing was written, so no ledger record exists. */
  ledger: nullable(str),
});
export type PluginsFolderSwitchData = Infer<typeof pluginsFolderSwitchData>;

export const userSwitchChange = obj({
  scope: lit("user"),
  action: lit("disable", "enable"),
  /** The one `claude` call, as an argument list. */
  command: arr(str),
});
export type UserSwitchChange = Infer<typeof userSwitchChange>;

export const pluginsUserSwitchData = obj({
  ...switchProps,
  scope: lit("user"),
  cwd: nullable(str),
  changes: arr(userSwitchChange),
});
export type PluginsUserSwitchData = Infer<typeof pluginsUserSwitchData>;

const perTool = obj({ pre: num, post: num, total: num });

export const hooksLsData = obj({
  cwd: nullable(str),
  disabledAll: bool,
  hooks: arr(hookEntry),
  counts: obj({
    /** Keyed `<kind>:<name>`, for example `plugin:ecc@ecc`. */
    byOwner: rec(num),
    perTool: obj({
      Bash: perTool,
      Edit: perTool,
      Write: perTool,
      Read: perTool,
    }),
    /** Events other than PreToolUse and PostToolUse of the four tracked tools, summed. */
    otherEvents: rec(num),
    otherEventsByOwner: rec(rec(num)),
  }),
  conflicts: arr(
    obj({
      event: str,
      tools: arr(hookTool),
      hooks: arr(str),
      note: str,
    }),
  ),
  warnings: arr(str),
});
export type HooksLsData = Infer<typeof hooksLsData>;

/** Golden file stem to the shape of its envelope `data`. */
export const pluginsShapes: Record<string, Shape<unknown>> = {
  "plugins-ls.cli": pluginsLsData,
  "plugins-ls.files": pluginsLsData,
  "plugins-ls.folder": pluginsLsData,
  "plugins-ls.measured": pluginsLsData,
  "plugins-ls.refresh": pluginsLsData,
  "plugins-show.cli": pluginsShowData,
  "plugins-show.denied": pluginsShowData,
  "plugins-show.files": pluginsShowData,
  "plugins-config.folder.plan": pluginsConfigData,
  "plugins-config.folder.apply": pluginsConfigData,
  "plugins-config.folder.unset-plan": pluginsConfigData,
  "plugins-config.folder.unset": pluginsConfigData,
  "plugins-config.global.plan": pluginsConfigData,
  "plugins-config.global.apply": pluginsConfigData,
  "plugins-mcp.deny.plan": pluginsMcpData,
  "plugins-mcp.deny.apply": pluginsMcpData,
  "plugins-mcp.deny.foreign": pluginsMcpData,
  "plugins-mcp.allow.plan": pluginsMcpData,
  "plugins-mcp.allow.apply": pluginsMcpData,
  "plugins-off.plan": pluginsFolderSwitchData,
  "plugins-off.apply": pluginsFolderSwitchData,
  "plugins-off.again": pluginsFolderSwitchData,
  "plugins-off.foreign": pluginsFolderSwitchData,
  "plugins-on.nothing": pluginsFolderSwitchData,
  "plugins-on.setup": pluginsFolderSwitchData,
  "plugins-on.plan": pluginsFolderSwitchData,
  "plugins-on.apply": pluginsFolderSwitchData,
  "plugins-on.foreign": pluginsFolderSwitchData,
  "plugins-on.setup-conflict": pluginsFolderSwitchData,
  "plugins-on.conflict.plan": pluginsFolderSwitchData,
  "plugins-on.conflict": pluginsFolderSwitchData,
  "plugins-disable.plan": pluginsUserSwitchData,
  "plugins-disable.apply": pluginsUserSwitchData,
  "plugins-enable.plan": pluginsUserSwitchData,
  "plugins-enable.apply": pluginsUserSwitchData,
  "hooks-ls.bash": hooksLsData,
  "hooks-ls.disabled": hooksLsData,
  "hooks-ls.full": hooksLsData,
  "hooks-ls.project": hooksLsData,
  "hooks-ls.skill": hooksLsData,
};
