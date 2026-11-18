import { CtlError, ctlData, ctlResultData, runCtl } from "../bridge/ctl";
import type { AuthSection, McpCallData, StatusData } from "../bridge/data";
import type {
  AccessSetsData,
  EditData,
  LoginData,
  MembershipData,
  ModeData,
  ProbeData,
  ReachData,
  RepoCheckData,
  RepoDoneData,
  RepoPlanData,
  RepoSwitchPlan,
  RepoStatusData,
  RestartData,
  SecretGetData,
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

export type InteropSwitch = "declareClientCapabilities" | "forwardInstructions";

const SWITCH_FLAG: Record<InteropSwitch, string> = {
  declareClientCapabilities: "--declare-client-capabilities",
  forwardInstructions: "--forward-instructions",
};

export const setToolListing = (key: string, mode: "auto" | "search") =>
  ctlData<ModeData>(["server", "mode", key, mode]);

export const setInteropSwitch = (key: string, name: InteropSwitch, on: boolean) =>
  ctlData<EditData>(["server", "edit", key, SWITCH_FLAG[name], on ? "on" : "off"]);

/** `null` removes the limit, so the default applies. */
export const setCallLimit = (key: string, ms: number | null) =>
  ctlData<EditData>([
    "server",
    "edit",
    key,
    "--max-request-timeout-ms",
    ms === null ? "default" : String(ms),
  ]);

export const loadReach = () => ctlData<ReachData>(["client", "reach"]);

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
  /** Leaves the direct client entries of the server in the clients' files. */
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

export async function loadLogins(): Promise<AuthSection> {
  return (await ctlData<StatusData>(["status"])).auth;
}

/** `--force` because a probe that is not due yet is skipped, and the person asked now. */
export function checkLogin(server: string) {
  return ctlDataOrFailure<ProbeData>(["auth", "probe", "--server", server, "--force"]);
}

export interface SignIn {
  /** Rejects with the CLI's own reason; resolves when the sign-in is done. */
  done: Promise<LoginData>;
  cancel: () => Promise<void>;
}

/** Runs the server's own sign-in. The browser opens by itself; the consent link is also handed to
 * `onLink` the moment the command prints it, for a person whose browser did not open. */
export function signIn(server: string, onLink: (url: string) => void): SignIn {
  const job = runCtl<LoginData>(["auth", "login", server], {
    onStderr: (line) => {
      const link = line.match(/https?:\/\/\S+/)?.[0];
      if (link) onLink(link);
    },
  });
  return {
    done: job.result.then((result) => {
      const data = ctlResultData(result);
      const link =
        data.consentUrl ?? result.stderr.join("\n").match(/https?:\/\/\S+/)?.[0];
      if (link) onLink(link);
      return data;
    }),
    cancel: job.cancel,
  };
}

/** `set: false` for a key that is not in the vault; any other failure (a locked keychain) throws. */
export async function secretIsSet(server: string, key: string): Promise<boolean> {
  try {
    await ctlData<SecretGetData>(["secret", "get", server, key]);
    return true;
  } catch (error) {
    if (error instanceof CtlError && error.code === "not_found") return false;
    throw error;
  }
}

/** The CLI's one command that prints a value. The caller shows it for a few seconds and drops it. */
export async function revealSecret(server: string, key: string): Promise<string> {
  const data = await ctlData<SecretGetData>(["secret", "get", server, key, "--reveal"]);
  return data.value ?? "";
}

export const repoStatus = (key: string) =>
  ctlData<RepoStatusData>(["server", "repo", "status", key]);

/** Fetches every checkout; a fetch that failed in one still returns the others' answers. */
export const repoCheck = () =>
  ctlDataOrFailure<RepoCheckData>(["server", "repo", "check"]);

export interface RepoBuildChoice {
  /** The command to run after the update; `null` leaves the files alone. */
  command: string | null;
  /** The command the person typed or ticked rather than the remembered one. */
  explicit: boolean;
}

function buildArgs(choice: RepoBuildChoice | undefined): string[] {
  if (!choice) return [];
  if (choice.command === null) return ["--no-build"];
  return choice.explicit ? ["--build", choice.command] : [];
}

/** The plan of an update; nothing changes. `--wait` bounds the question to the gateways. */
export const repoPlan = (key: string, build?: RepoBuildChoice, fetch = false) =>
  ctlData<RepoPlanData>([
    "server",
    "repo",
    "update",
    key,
    "--dry-run",
    "--wait",
    "4",
    ...(fetch ? ["--fetch"] : []),
    ...buildArgs(build),
  ]);

/** The plan of switching to the branch another machine is on; it fetches first. */
export const switchPlan = (key: string) =>
  ctlData<{ plan: RepoSwitchPlan }>([
    "server",
    "repo",
    "switch",
    key,
    "--dry-run",
    "--fetch",
  ]);

export const switchApply = (key: string, planHash: string) =>
  repoDone(["server", "repo", "switch", key, "--plan-hash", planHash]);

export const switchBack = (key: string) =>
  repoDone(["server", "repo", "switch", key, "--undo"]);

/** A build that failed exits 1 but still says what moved; a refusal carries only its reason and
 * throws with the message. */
async function repoDone(argv: string[]): Promise<RepoDoneData> {
  try {
    return await ctlData<RepoDoneData>(argv);
  } catch (error) {
    const data = error instanceof CtlError ? (error.data as Partial<RepoDoneData>) : null;
    if (data && typeof data.applied === "boolean") return data as RepoDoneData;
    throw error;
  }
}

export function repoApply(
  key: string,
  options: { planHash: string; build?: RepoBuildChoice; restartNow?: boolean },
) {
  return repoDone([
    "server",
    "repo",
    "update",
    key,
    "--plan-hash",
    options.planHash,
    ...buildArgs(options.build),
    ...(options.restartNow ? ["--restart-now"] : []),
  ]);
}

export const repoUndo = (key: string) => repoDone(["server", "repo", "undo", key]);
