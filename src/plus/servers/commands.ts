import { CtlError, ctlData } from "../bridge/ctl";
import type { McpCallData } from "../bridge/data";
import type {
  AccessSetsData,
  MembershipData,
  RestartData,
  ServerInfoData,
  ServerListData,
  UninstallData,
} from "./data";

/** A failed envelope that still carries data (a restart that failed in one gateway) returns that
 * data; one without data throws. */
async function ctlDataOrFailure<T>(argv: string[], signal?: AbortSignal): Promise<T> {
  try {
    return await ctlData<T>(argv, { signal });
  } catch (error) {
    if (error instanceof CtlError && error.data !== undefined && error.data !== null) {
      return error.data as T;
    }
    throw error;
  }
}

/** Runs a self-management tool through `mcp call`; the arguments travel over stdin. */
async function callTool<T>(tool: string, args: Record<string, unknown>): Promise<T> {
  const data = await ctlData<McpCallData>(["mcp", "call", tool, "--args-stdin"], {
    stdinSecret: JSON.stringify(args),
  });
  if (data.isError) {
    const error = (data.result as { error?: { message?: string } }).error;
    throw new Error(error?.message ?? `${tool} failed`);
  }
  return data.result as T;
}

export const listServers = () => ctlData<ServerListData>(["server", "ls"]);

export const serverInfo = (key: string) =>
  ctlData<ServerInfoData>(["server", "info", key]);

export const listAccessSets = () => callTool<AccessSetsData>("servers_list_profiles", {});

export const setMembership = (server: string, set: string, member: boolean) =>
  callTool<MembershipData>(
    member ? "servers_add_profile_tag" : "servers_remove_profile_tag",
    { name: server, profile_tag: set },
  );

export interface RestartOptions {
  /** Ask the gateways first and restart only when no call is running. */
  ifIdle?: boolean;
  signal?: AbortSignal;
}

export function restartServer(
  key: string,
  { ifIdle = false, signal }: RestartOptions = {},
): Promise<RestartData> {
  return ctlDataOrFailure<RestartData>(
    ["server", "restart", key, ...(ifIdle ? ["--if-idle"] : [])],
    signal,
  );
}

export interface UninstallOptions {
  keepSecrets: boolean;
  /** The flag is passed on; it takes effect when direct client entries exist (slice S11). */
  keepClients: boolean;
}

export function previewUninstall(key: string, options: UninstallOptions) {
  return ctlData<UninstallData>(uninstallArgv(key, options, true));
}

export function uninstallServer(key: string, options: UninstallOptions) {
  return ctlData<UninstallData>(uninstallArgv(key, options, false));
}

function uninstallArgv(key: string, options: UninstallOptions, dryRun: boolean) {
  return [
    "server",
    "uninstall",
    key,
    ...(dryRun ? ["--dry-run"] : []),
    ...(options.keepSecrets ? ["--keep-secrets"] : []),
    ...(options.keepClients ? ["--keep-clients"] : []),
  ];
}
