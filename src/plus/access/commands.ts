import { ctlData } from "../bridge/ctl";
import type { McpCallData } from "../bridge/data";
import type {
  AccessSetListData,
  DiscoveryMode,
  RoutesData,
  ToolsData,
  WriteOutcome,
} from "./data";

export const listSets = () => ctlData<AccessSetListData>(["access-set", "ls"]);
export const listRoutes = () => ctlData<RoutesData>(["folder", "route", "ls"]);
export const listNarrowed = () => ctlData<ToolsData>(["access-set", "tools"]);

const dry = (dryRun: boolean) => (dryRun ? ["--dry-run"] : []);

const write = (argv: string[], dryRun: boolean) =>
  ctlData<WriteOutcome>([...argv, ...dry(dryRun)]);

export const addSet = (name: string, serverIds: string[], dryRun: boolean) =>
  write(
    ["access-set", "add", name, ...serverIds.flatMap((id) => ["--server", id])],
    dryRun,
  );

export type Orphans = "detach" | "leave" | null;

export const removeSet = (set: string, orphans: Orphans, dryRun: boolean) =>
  write(["access-set", "rm", set, ...(orphans ? [`--${orphans}`] : [])], dryRun);

/** `null` returns the default access to every enabled server. */
export const makeDefault = (set: string | null, dryRun: boolean) =>
  write(["access-set", "default", set ?? "--none"], dryRun);

export type ToolChange =
  { kind: "all" } | { kind: "none" } | { kind: "only"; tools: string[] };

export function setTools(
  set: string,
  server: string,
  change: ToolChange,
  dryRun: boolean,
) {
  const flags =
    change.kind === "all"
      ? ["--all"]
      : change.kind === "none"
        ? ["--none"]
        : change.tools.flatMap((tool) => ["--tool", tool]);
  return write(["access-set", "tools", set, server, ...flags], dryRun);
}

/** `null` takes the client's own set away, so it follows the default access. */
export const bindClient = (client: string, set: string | null, dryRun: boolean) =>
  write(["client", "scope", client, set ?? "--none"], dryRun);

export const routeFolder = (path: string, set: string, dryRun: boolean) =>
  write(["folder", "route", "add", path, set], dryRun);

export const unrouteFolder = (path: string, dryRun: boolean) =>
  write(["folder", "route", "rm", path], dryRun);

/** The self-management tool has no command of its own, so it runs through `mcp call`; the
 * arguments travel over stdin. Without `dry_run: false` and `confirm` it only plans. */
export async function runTool<T>(tool: string, args: Record<string, unknown>) {
  const data = await ctlData<McpCallData>(["mcp", "call", tool, "--args-stdin"], {
    stdinSecret: JSON.stringify(args),
  });
  if (data.isError) {
    const error = (data.result as { error?: { message?: string } }).error;
    throw new Error(error?.message ?? `${tool} failed`);
  }
  return data.result as T;
}

export const setDiscovery = (client: string, mode: DiscoveryMode, dryRun: boolean) =>
  runTool<WriteOutcome>(
    "client_discovery_set",
    dryRun ? { client, mode } : { client, mode, dry_run: false, confirm: true },
  );
