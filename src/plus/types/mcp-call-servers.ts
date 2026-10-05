import { bool, lit, num, obj, type Shape } from "../bridge/shape";
import {
  serversAddProfileTagResult,
  serversApplyUpdateResult,
  serversCheckUpdatesResult,
  serversDetectSourceResult,
  serversForkSyncResult,
  serversGitStatusResult,
  serversRemoveProfileTagResult,
  serversSetModeResult,
} from "./selfmcp-servers";

/** `data` of `toolportctl mcp call` for the servers tools (contract section 15): the tool, its
 * tier and its result, checked against the `mcp-call.*` goldens by `data.test.ts`. */
const called = <T>(tool: string, result: Shape<T>) =>
  obj({ isError: bool, result, tier: num, tool: lit(tool) });

export const mcpCallServersShapes: Record<string, Shape<unknown>> = {
  "mcp-call.servers_add_profile_tag": called(
    "servers_add_profile_tag",
    serversAddProfileTagResult,
  ),
  "mcp-call.servers_apply_update": called(
    "servers_apply_update",
    serversApplyUpdateResult,
  ),
  "mcp-call.servers_check_updates": called(
    "servers_check_updates",
    serversCheckUpdatesResult,
  ),
  "mcp-call.servers_detect_source": called(
    "servers_detect_source",
    serversDetectSourceResult,
  ),
  "mcp-call.servers_fork_sync": called("servers_fork_sync", serversForkSyncResult),
  "mcp-call.servers_git_status": called("servers_git_status", serversGitStatusResult),
  "mcp-call.servers_remove_profile_tag": called(
    "servers_remove_profile_tag",
    serversRemoveProfileTagResult,
  ),
  "mcp-call.servers_set_mode": called("servers_set_mode", serversSetModeResult),
};
