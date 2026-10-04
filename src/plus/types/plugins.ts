import {
  any,
  arr,
  bool,
  lit,
  nullable,
  num,
  obj,
  rec,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

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
  /** Reserved for the adapter knobs of `plugins config`. */
  knobs: arr(any),
  warnings: arr(str),
});
export type PluginsShowData = Infer<typeof pluginsShowData>;

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
  "hooks-ls.bash": hooksLsData,
  "hooks-ls.disabled": hooksLsData,
  "hooks-ls.full": hooksLsData,
  "hooks-ls.project": hooksLsData,
  "hooks-ls.skill": hooksLsData,
};
