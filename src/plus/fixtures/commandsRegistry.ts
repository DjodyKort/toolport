import type { CommandFlag, CommandRow, CommandsData } from "../bridge/data";

type Tier = "read" | "write" | "destructive";
type RowInit = Partial<CommandRow> & Pick<CommandRow, "id" | "summary">;

const NO_PREVIEW = { mode: "none", flag: null } as const;
export const DRY_RUN = { mode: "flag", flag: "--dry-run" } as const;

export function flag(name: string, init: Partial<CommandFlag> = {}): CommandFlag {
  return {
    name,
    aliases: [],
    valueType: "bool",
    required: false,
    repeatable: false,
    escalates: false,
    hidden: false,
    sensitive: false,
    effect: "",
    ...init,
  };
}

const dryRunFlag = flag("--dry-run", { effect: "Preview the change and write nothing" });

export function command(init: RowInit): CommandRow {
  const path = init.id.split(" ");
  const tier: Tier | null = init.tier === undefined ? "read" : init.tier;
  return {
    path,
    group: path.length > 1 ? path.slice(0, -1).join(" ") : path[0],
    kind: "command",
    parent: path.length > 1 ? path.slice(0, -1).join(" ") : null,
    planned: false,
    tier,
    baseTier: tier,
    dryRun: (init.preview ?? NO_PREVIEW).mode !== "none",
    preview: NO_PREVIEW,
    needs: [],
    cost: false,
    surface: "screen",
    operands: [],
    maxOperands: null,
    flags: [],
    operandEscalates: false,
    oneOf: [],
    tools: [],
    ...init,
  };
}

function group(id: string, summary: string, planned = false): CommandRow {
  return {
    id,
    path: id.split(" "),
    group: id,
    kind: "group",
    parent: null,
    summary,
    planned,
    tier: null,
    baseTier: null,
    dryRun: false,
    preview: null,
    needs: [],
    cost: false,
    surface: null,
    operands: [],
    maxOperands: null,
    flags: [],
    operandEscalates: false,
    oneOf: [],
    tools: [],
  };
}

const commands: CommandRow[] = [
  command({
    id: "status",
    summary: "Show registry, profile, secrets backend and gateway state",
  }),
  group("server", "Manage servers"),
  command({
    id: "server ls",
    summary: "List servers and whether the active profile enables them",
    tools: ["servers_list"],
  }),
  command({
    id: "server new",
    summary: "Add a server from a command or a URL",
    tier: "write",
    operands: [{ name: "name", required: true, variadic: false }],
    oneOf: [["--command", "--url"]],
    flags: [
      flag("--command", {
        valueType: "string",
        effect: "Program that starts the server",
      }),
      flag("--url", { valueType: "string", effect: "Address of a remote server" }),
      flag("--arg", { valueType: "list", effect: "Arguments (comma separated)" }),
      flag("--env", {
        valueType: "string",
        repeatable: true,
        effect: "Environment variable as KEY=value (repeatable)",
      }),
      flag("--transport", {
        valueType: "choice",
        choices: ["http", "sse"],
        effect: "Transport of a remote server",
      }),
    ],
  }),
  command({
    id: "server uninstall",
    summary: "Remove a server and its client entries",
    tier: "destructive",
    preview: DRY_RUN,
    operands: [{ name: "server", required: true, variadic: false }],
    flags: [
      dryRunFlag,
      flag("--keep-clients", {
        effect: "Keep the client entries that point at the server",
      }),
      flag("--keep-secrets", { effect: "Keep the server's secrets in the vault" }),
    ],
    tools: ["servers_uninstall"],
  }),
  group("profile", "Manage profiles"),
  command({
    id: "profile edit",
    summary: "Rename a profile or change its servers",
    tier: "write",
    preview: DRY_RUN,
    operands: [{ name: "profile", required: true, variadic: false }],
    flags: [
      flag("--name", { valueType: "string", effect: "New name for the profile" }),
      flag("--add-server", {
        valueType: "list",
        effect: "Add these servers (comma separated names or ids)",
      }),
      flag("--remove-server", {
        valueType: "list",
        effect: "Remove these servers (comma separated names or ids)",
      }),
      flag("--force", {
        hidden: true,
        effect: "Accepted for compatibility; has no effect",
      }),
      dryRunFlag,
    ],
  }),
  group("secret", "Manage secrets"),
  command({
    id: "secret set",
    summary: "Store a secret read from stdin",
    tier: "write",
    needs: ["stdin"],
    operands: [
      { name: "server", required: true, variadic: false },
      { name: "key", required: true, variadic: false },
    ],
    flags: [
      flag("--value-env", {
        valueType: "string",
        hidden: true,
        sensitive: true,
        effect: "Name of an environment variable holding the value",
      }),
    ],
  }),
  group("skills", "Manage skills"),
  command({
    id: "skills sync",
    summary: "Transpile skills to client outputs",
    tier: "write",
    preview: DRY_RUN,
    maxOperands: 0,
    flags: [
      flag("--repo", {
        valueType: "path",
        effect: "Skills repository (default: the configured one)",
      }),
      flag("--client", {
        valueType: "string",
        effect: "Limit to one client (id or key)",
      }),
      flag("--project", {
        effect: "Use the project-level location instead of the user level",
      }),
      dryRunFlag,
    ],
    tools: ["skills_sync"],
  }),
  group("client", "Manage AI clients"),
  command({
    id: "client import",
    summary: "Read the servers a client already has and import them",
    tier: "write",
    baseTier: "read",
    preview: DRY_RUN,
    operands: [{ name: "client", required: true, variadic: false }],
    flags: [
      flag("--select", {
        valueType: "list",
        escalates: true,
        effect: "Import only these servers",
      }),
      flag("--all", { escalates: true, effect: "Import every server found" }),
      dryRunFlag,
    ],
  }),
  command({ id: "client ls", summary: "List the AI clients Toolport can configure" }),
  group("compression", "Token compression"),
  command({
    id: "compression update",
    summary: "Check for a newer compression release",
    tier: "write",
    needs: ["network", "long-running"],
    preview: { mode: "unless-applied", flag: "--accept" },
    flags: [flag("--accept", { escalates: true, effect: "Install the release" })],
  }),
  command({
    id: "compression run",
    summary: "Start the coding assistant behind the compression proxy",
    tier: "write",
    surface: "terminal",
    needs: ["terminal-only", "long-running"],
    preview: { mode: "flag", flag: "--plan" },
    operands: [{ name: "assistant-args", required: false, variadic: true }],
  }),
  group("sync", "Encrypted sync"),
  command({
    id: "sync init",
    summary: "Create the encrypted sync bundle",
    tier: "write",
    needs: ["network", "stdin"],
    oneOf: [["--passphrase-stdin", "--passphrase-env"]],
    flags: [
      flag("--passphrase-stdin", {
        sensitive: true,
        effect: "Read the passphrase from stdin",
      }),
      flag("--passphrase-env", {
        valueType: "string",
        hidden: true,
        sensitive: true,
        effect: "Name of an environment variable holding the passphrase",
      }),
    ],
  }),
  command({
    id: "sync rotate-passphrase",
    summary: "Change the passphrase of the sync bundle",
    tier: "destructive",
    needs: ["network", "stdin"],
    oneOf: [["--passphrase-stdin", "--passphrase-env"]],
    flags: [
      flag("--passphrase-stdin", {
        sensitive: true,
        effect: "Read the passphrase from stdin",
      }),
      flag("--passphrase-env", {
        valueType: "string",
        hidden: true,
        sensitive: true,
        effect: "Name of an environment variable holding the passphrase",
      }),
    ],
  }),
  group("council", "Ask several models"),
  command({
    id: "council ask",
    summary: "Ask a question to several models (uses paid tokens)",
    needs: ["network"],
    cost: true,
    operands: [{ name: "question", required: true, variadic: false }],
  }),
  command({
    id: "attention ls",
    summary: "List what needs the user",
    flags: [flag("--all", { effect: "Include items that are only worth a look" })],
  }),
  group("report", "Reports", true),
];

const tools: CommandsData["tools"] = [
  {
    name: "servers_list",
    tier: "read",
    toolTier: 1,
    dryRun: "none",
    command: "server ls",
  },
  {
    name: "servers_uninstall",
    tier: "destructive",
    toolTier: 4,
    dryRun: "param",
    command: "server uninstall",
  },
  {
    name: "skills_sync",
    tier: "write",
    toolTier: 3,
    dryRun: "default-on",
    command: "skills sync",
  },
  { name: "skills_get", tier: "read", toolTier: 1, dryRun: "none", command: null },
  { name: "skills_edit_body", tier: "write", toolTier: 3, dryRun: "none", command: null },
  {
    name: "styles_apply_note",
    tier: "write",
    toolTier: 2,
    dryRun: "param",
    command: null,
  },
  {
    name: "skills_git_push",
    tier: "destructive",
    toolTier: 4,
    dryRun: "none",
    command: null,
  },
];

function registry(all: CommandRow[]): CommandsData {
  const runnable = all.filter((row) => row.kind === "command");
  return {
    tiers: ["read", "write", "destructive"],
    needs: ["stdin", "browser", "long-running", "network", "terminal-only"],
    counts: {
      rows: all.length,
      commands: runnable.length,
      groups: all.length - runnable.length,
      tools: tools.length,
    },
    commands: all,
    tools,
  };
}

/** A small, hand-written registry in the shape of `toolportctl commands --json`, with names
 * that belong to no one. It is validated against the shape by `plusCtl.test.ts`. */
export const commandsFixture: CommandsData = registry(commands);

/** The same registry once `mcp call` exists (contract section 15). */
export const commandsFixtureWithMcpCall: CommandsData = registry([
  ...commands,
  command({
    id: "mcp call",
    summary: "Call a self-management tool",
    tier: "write",
    needs: ["stdin"],
    operands: [{ name: "tool", required: true, variadic: false }],
    flags: [flag("--args-stdin", { effect: "Read the arguments as JSON from stdin" })],
  }),
]);
