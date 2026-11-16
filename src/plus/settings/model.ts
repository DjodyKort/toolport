import type {
  CheckStatus,
  CommandRow,
  CommandToolRow,
  McpDoctorData,
  McpToolRow,
  SelfMcpState,
  Tier,
} from "../bridge/data";
import type { Overview } from "./useCtl";

export const CHECK_LABELS: Record<string, string> = {
  dataDir: "Data folder",
  registry: "Registry",
  defaultAccess: "Default access",
  secretsBackend: "Secrets storage",
  gatewayBinary: "Gateway program",
  plusBuilt: "Toolport+ build",
};

export const SELF_CHECK_LABELS: Record<string, string> = {
  registry_entry: "Registered",
  command_matches_binary: "Points at this app's program",
  binary_present: "Program present",
  server_enabled: "Switched on",
  listed_in_default_access: "Listed in the default access set",
  listed_in_client_access_sets: "Listed in client access sets",
  handshake: "Answers a handshake",
  catalog: "Tools and resources",
};

export function checkLabel(name: string, labels: Record<string, string>): string {
  return labels[name] ?? name;
}

export const CHECK_BADGE: Record<
  CheckStatus,
  { variant: "success" | "warning" | "destructive"; text: string }
> = {
  ok: { variant: "success", text: "OK" },
  warn: { variant: "warning", text: "Check" },
  fail: { variant: "destructive", text: "Problem" },
};

export interface SelfMcpView {
  badge: { variant: "success" | "warning" | "off"; text: string };
  summary: string;
  /** The one action the state offers first. */
  primary: "connect" | "list" | "open-servers" | null;
  /** Whether the server can be removed from here. */
  removable: boolean;
}

/** What the self-management row says and offers in each state. */
export function selfMcpView(state: SelfMcpState): SelfMcpView {
  switch (state) {
    case "missing":
      return {
        badge: { variant: "off", text: "Not connected" },
        summary:
          "Lets an agent list, check and change this setup, with a preview and a confirm. Not connected yet.",
        primary: "connect",
        removable: false,
      };
    case "opted-out":
      return {
        badge: { variant: "off", text: "Removed" },
        summary: "You removed it. Connect it again to bring it back.",
        primary: "connect",
        removable: false,
      };
    case "not-enabled":
      return {
        badge: { variant: "warning", text: "Not listed" },
        summary: "Registered, but no access set lists it, so clients cannot reach it.",
        primary: "list",
        removable: true,
      };
    case "disabled":
      return {
        badge: { variant: "warning", text: "Switched off" },
        summary:
          "It is switched off in Servers. Connecting again does not switch it back on.",
        primary: "open-servers",
        removable: true,
      };
    case "enabled":
      return {
        badge: { variant: "success", text: "Connected" },
        summary:
          "Agents can list, check and change this setup, with a preview and a confirm.",
        primary: null,
        removable: true,
      };
  }
}

export type LeadVariant = "danger" | "warning" | "success" | "info";

export interface Lead {
  variant: LeadVariant;
  title: string;
  detail: string;
}

function failing(overview: Overview): string[] {
  return (
    overview.doctor.data?.checks.filter((c) => c.status === "fail").map((c) => c.name) ??
    []
  );
}

function warning(overview: Overview): string[] {
  return (
    overview.doctor.data?.checks.filter((c) => c.status === "warn").map((c) => c.name) ??
    []
  );
}

/** The one message that leads the page: the worst thing first, then the first thing to do. */
export function leadFor(overview: Overview): Lead {
  const status = overview.status.data;
  if (status && !status.registry.readable) {
    return {
      variant: "danger",
      title: "Toolport cannot read its registry",
      detail: status.registry.error ?? "The registry file could not be read.",
    };
  }
  const failed = failing(overview);
  if (failed.length > 0) {
    return {
      variant: "danger",
      title:
        failed.length === 1
          ? "1 installation check failed"
          : `${failed.length} installation checks failed`,
      detail: failed.map((name) => checkLabel(name, CHECK_LABELS)).join(", "),
    };
  }
  const warned = warning(overview);
  if (warned.includes("plusBuilt")) {
    return {
      variant: "warning",
      title: "Toolport+ is not fully active in this build",
      detail:
        "A program next to the app was built without the fork seam, so the egress guard and fork hooks are off in it.",
    };
  }
  if (warned.length > 0) {
    return {
      variant: "warning",
      title:
        warned.length === 1
          ? "1 installation check needs a look"
          : `${warned.length} installation checks need a look`,
      detail: warned.map((name) => checkLabel(name, CHECK_LABELS)).join(", "),
    };
  }
  const mcp = overview.mcp.data;
  const facts = status
    ? `${plural(status.serverCount, "server")}, ${plural(status.accessSetCount, "access set")}`
    : "";
  if (mcp && mcp.state !== "enabled") {
    return {
      variant: "info",
      title: "Self-management is not connected",
      detail: `Everything else is healthy${facts ? ` (${facts})` : ""}. Connecting it is optional.`,
    };
  }
  return {
    variant: "success",
    title: "Toolport+ is working",
    detail: facts
      ? `${facts}. Self-management is connected.`
      : "Self-management is connected.",
  };
}

export function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

export function accessSetsLine(mcp: McpDoctorData): string {
  const parts: string[] = [];
  if (mcp.defaultAccess === null) parts.push("Default access: every enabled server");
  else
    parts.push(
      `Default access ${mcp.defaultAccess.id}: ${standing(mcp.defaultAccess.member, mcp.defaultAccess.optedOut)}`,
    );
  for (const set of mcp.clientAccessSets) {
    parts.push(`${set.id}: ${standing(set.member, set.optedOut)}`);
  }
  return parts.join(" · ");
}

function standing(member: boolean, optedOut: boolean): string {
  if (member) return "listed";
  return optedOut ? "taken out on purpose" : "not listed";
}

const NEEDS_A_PERSON = ["stdin", "browser", "terminal-only"];

/** A command the page can run for the user: read-only, nothing to fill in, nothing that waits for
 * a browser, a terminal or input. */
export function canRunCommand(row: CommandRow): boolean {
  return (
    row.kind === "command" &&
    row.tier === "read" &&
    !row.planned &&
    row.surface !== "terminal" &&
    row.operands.every((o) => !o.required) &&
    row.flags.every((f) => !f.required) &&
    row.oneOf.length === 0 &&
    row.needs.every((n) => !NEEDS_A_PERSON.includes(n))
  );
}

export function commandArgv(row: CommandRow): string[] {
  return row.path;
}

export function commandLine(row: CommandRow): string {
  return `toolportctl ${row.path.join(" ")}`;
}

export const TIER_BADGE: Record<
  Tier,
  { variant: "off" | "warning" | "destructive"; text: string }
> = {
  read: { variant: "off", text: "Read only" },
  write: { variant: "warning", text: "Changes things" },
  destructive: { variant: "destructive", text: "Removes things" },
};

export interface ToolEntry {
  name: string;
  description: string;
  tier: Tier;
  gated: boolean;
  previews: boolean;
  params: McpToolRow["params"];
}

/** The self-management tools with the tier of the registry and the description of the server. */
export function toolEntries(
  rows: CommandToolRow[],
  tools: McpToolRow[] | undefined,
): ToolEntry[] {
  return rows.map((row) => {
    const tool = tools?.find((t) => t.name === row.name);
    return {
      name: row.name,
      description: tool?.description ?? "",
      tier: row.tier,
      gated: (tool?.gate ?? "none") !== "none" || row.tier !== "read",
      previews: tool?.params.some((p) => p.name === "dry_run") ?? row.dryRun !== "none",
      params: tool?.params ?? [],
    };
  });
}
