import {
  any,
  arr,
  bool,
  nullable,
  num,
  obj,
  opt,
  rec,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** Results of the servers self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

export const serversAddProfileTagResult = obj({
  name: str,
  profileTags: arr(str),
});
export type ServersAddProfileTagResult = Infer<typeof serversAddProfileTagResult>;

export const serversApplyUpdateResult = obj({
  counts: rec(num),
  mode: str,
  servers: arr(
    obj({
      detected: bool,
      id: str,
      kind: str,
      message: str,
      status: str,
    }),
  ),
});
export type ServersApplyUpdateResult = Infer<typeof serversApplyUpdateResult>;

export const serversAuthResult = obj({
  authUrl: nullable(any),
  exited: bool,
  hint: nullable(any),
  name: str,
  started: bool,
  stderrTail: arr(any),
  timedOut: bool,
});
export type ServersAuthResult = Infer<typeof serversAuthResult>;

export const serversCheckUpdatesResult = obj({
  counts: rec(num),
  mode: str,
  servers: arr(
    obj({
      detected: bool,
      id: str,
      kind: str,
      message: str,
      status: str,
    }),
  ),
});
export type ServersCheckUpdatesResult = Infer<typeof serversCheckUpdatesResult>;

const gitSourceMeta = obj({
  branch: opt(str),
  drift: opt(bool),
  path: opt(str),
  post_update: opt(str),
  reason: opt(str),
  remote: opt(str),
  type: str,
  upstream: opt(obj({ branch: str, remote: str })),
});

export const serversDetectSourceResult = obj({
  branches: opt(rec(arr(str))),
  detected: obj({
    kind: str,
    meta: gitSourceMeta,
  }),
  name: str,
  remotes: opt(arr(str)),
  stored: bool,
});
export type ServersDetectSourceResult = Infer<typeof serversDetectSourceResult>;

export const serversForkSyncResult = obj({
  branch: str,
  mode: str,
  picked: opt(num),
  previousBranch: str,
  synced: bool,
});
export type ServersForkSyncResult = Infer<typeof serversForkSyncResult>;

export const serversGetResult = obj({
  args: arr(any),
  command: str,
  cwd: nullable(str),
  declareClientCapabilities: bool,
  disabledTools: arr(any),
  enabled: bool,
  env: arr(
    obj({
      key: str,
      secret: bool,
    }),
  ),
  forwardInstructions: bool,
  id: str,
  name: str,
  source: nullable(str),
  transport: str,
  url: nullable(str),
});
export type ServersGetResult = Infer<typeof serversGetResult>;

export const serversGitStatusResult = obj({
  ahead: opt(num),
  behind: opt(num),
  branch: opt(str),
  dirty: opt(bool),
  isGit: bool,
  message: opt(str),
  name: str,
  path: opt(str),
  remoteRef: opt(str),
  summaries: opt(arr(str)),
});
export type ServersGitStatusResult = Infer<typeof serversGitStatusResult>;

export const serversInstallResult = obj({
  id: str,
  installed: bool,
  name: str,
});
export type ServersInstallResult = Infer<typeof serversInstallResult>;

export const serversListResult = obj({
  activeProfile: str,
  servers: arr(
    obj({
      enabled: bool,
      id: str,
      name: str,
      source: nullable(str),
      transport: str,
    }),
  ),
});
export type ServersListResult = Infer<typeof serversListResult>;

export const serversListProfilesResult = obj({
  activeProfile: str,
  profiles: arr(
    obj({
      enabledServerIds: arr(str),
      id: str,
      name: str,
    }),
  ),
});
export type ServersListProfilesResult = Infer<typeof serversListProfilesResult>;

export const serversRemoveProfileTagResult = obj({
  name: str,
  profileTags: arr(any),
});
export type ServersRemoveProfileTagResult = Infer<typeof serversRemoveProfileTagResult>;

export const serversSetModeResult = obj({
  changed: bool,
  dropped: bool,
  mode: str,
  name: str,
  note: str,
});
export type ServersSetModeResult = Infer<typeof serversSetModeResult>;

export const serversSetSourceResult = obj({
  name: str,
  source: obj({ kind: str, meta: gitSourceMeta }),
});
export type ServersSetSourceResult = Infer<typeof serversSetSourceResult>;

export const serversUninstallResult = obj({
  clients: arr(any),
  dryRun: bool,
  id: str,
  name: str,
  secretsRemoved: str,
});
export type ServersUninstallResult = Infer<typeof serversUninstallResult>;

export const serversUpdateConfigResult = obj({
  id: str,
  updatedKeys: arr(str),
});
export type ServersUpdateConfigResult = Infer<typeof serversUpdateConfigResult>;

/** Tool name to the shape of its `structuredContent`. */
export const serversToolShapes: Record<string, Shape<unknown>> = {
  servers_add_profile_tag: serversAddProfileTagResult,
  servers_apply_update: serversApplyUpdateResult,
  servers_auth: serversAuthResult,
  servers_check_updates: serversCheckUpdatesResult,
  servers_detect_source: serversDetectSourceResult,
  servers_fork_sync: serversForkSyncResult,
  servers_get: serversGetResult,
  servers_git_status: serversGitStatusResult,
  servers_install: serversInstallResult,
  servers_list: serversListResult,
  servers_list_profiles: serversListProfilesResult,
  servers_remove_profile_tag: serversRemoveProfileTagResult,
  servers_set_mode: serversSetModeResult,
  servers_set_source: serversSetSourceResult,
  servers_uninstall: serversUninstallResult,
  servers_update_config: serversUpdateConfigResult,
};
