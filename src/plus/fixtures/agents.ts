/** Envelope `data` of the agents and styles commands for the dev browser fixture: one agent
 * (`scout`) that reaches four clients with the `tools` field dropped by two of them, and no
 * output styles yet. Shapes follow `src-tauri/tests/fixtures/ctl-envelopes/agents-*.json`. */
import type { CommandFlag, CommandRow } from "../bridge/data";

const repo = "/fixture/skills-repo";
const home = "/fixture/home";
const data = "/fixture/data";

export const agentWorld = {
  name: "scout",
  description: "Reads a repository and reports what it finds",
  model: "sonnet",
  tools: ["Read", "Grep"],
  path: `${repo}/agents/scout/AGENT.md`,
  outputs: [
    {
      client: "claude-code",
      files: [".claude/agents/scout.md"],
      warnings: [] as string[],
    },
    {
      client: "codex-cli",
      files: [".codex/agents/scout.toml"],
      warnings: ["codex-cli: 'tools' field not supported in agent TOML, dropped"],
    },
    {
      client: "cursor",
      files: [".cursor/agents/scout.md"],
      warnings: ["cursor: 'tools' field not supported, dropped"],
    },
    {
      client: "gemini-cli",
      files: [".gemini/agents/scout.md"],
      warnings: [] as string[],
    },
  ],
};

export function agentsSyncData(dryRun: boolean) {
  const { name, model, outputs } = agentWorld;
  return {
    agentCount: 1,
    agents: [
      {
        clientsSynced: outputs.map((o) => o.client),
        found: true,
        model,
        name,
        outputFiles: outputs.map(({ client, files }) => ({ client, files })),
        warnings: outputs.flatMap((o) => o.warnings),
      },
    ],
    clientCount: outputs.length,
    discoveryWarnings: [],
    dryRun,
    foundCount: 1,
    lockDir: data,
    outputRoot: home,
    repo,
    scope: "global",
    syncedAt: "2026-10-04T08:00:00Z",
  };
}

const outputPaths = agentWorld.outputs.map((o) => `${home}/${o.files[0]}`);

const agentsClean = (dryRun: boolean) => ({
  cleanRoot: home,
  dryRun,
  ignored: [],
  lockDir: data,
  lockfilePresent: true,
  managed: ["scout"],
  removed: outputPaths,
  scope: "global",
  skipped: [],
});

const agentsUninstall = (dryRun: boolean) => ({
  dryRun,
  lockDir: data,
  lockUpdated: true,
  name: "scout",
  outputRoot: home,
  outputs: outputPaths,
  repo,
  scope: "global",
  sourcePath: `${repo}/agents/scout`,
});

const added = (kind: "agents" | "styles", name: string, dryRun: boolean) => ({
  dryRun,
  name,
  path: `${repo}/${kind}/${name}/${kind === "agents" ? "AGENT" : "STYLE"}.md`,
  repo,
});

const stylesSync = (dryRun: boolean) => ({
  clientCount: 2,
  discoveryWarnings: [],
  dryRun,
  foundCount: 1,
  lockDir: data,
  outputRoot: home,
  outputs: [`${home}/.claude/output-styles/terse.md`, `${home}/.roomodes`],
  repo,
  scope: "global",
  styleCount: 1,
  styles: [
    {
      clientsSynced: ["claude-code", "roomodes-style"],
      description: "Short answers, no preamble",
      name: "terse",
      warnings: [],
    },
  ],
  syncedAt: "2026-10-04T08:00:00Z",
});

export const agentsCtlFixtures = new Map<string, unknown>([
  [
    "agents ls",
    {
      agents: [
        {
          description: agentWorld.description,
          model: agentWorld.model,
          name: agentWorld.name,
          path: agentWorld.path,
          tools: agentWorld.tools,
        },
      ],
      discoveryWarnings: [],
      repo,
    },
  ],
  [
    "agents lint",
    {
      agentCount: 1,
      discoveryWarnings: [],
      errors: 0,
      infos: 0,
      messages: [],
      repo,
      warnings: 0,
    },
  ],
  [
    "agents audit",
    {
      agentCount: 1,
      clean: true,
      discoveryWarnings: [],
      findings: [],
      high: 0,
      low: 0,
      medium: 0,
      repo,
    },
  ],
  [
    "agents diff",
    {
      clean: false,
      discoveryWarnings: [],
      modified: [],
      new: ["scout"],
      noLockfile: true,
      removed: [],
      repo,
      unchanged: 0,
    },
  ],
  [
    "agents status",
    {
      drift: false,
      lockedCount: 0,
      lockfilePresent: false,
      outputRoot: null,
      outputs: [],
      repo,
    },
  ],
  ["agents sync --dry-run", agentsSyncData(true)],
  ["agents sync", agentsSyncData(false)],
  ["agents clean --dry-run", agentsClean(true)],
  ["agents clean", agentsClean(false)],
  ["agents uninstall scout --dry-run", agentsUninstall(true)],
  ["agents uninstall scout", agentsUninstall(false)],
  ["agents add reviewer --dry-run", added("agents", "reviewer", true)],
  ["agents add reviewer", added("agents", "reviewer", false)],
  [
    "styles ls",
    { active: [], discoveryWarnings: [], lockfilePresent: false, repo, styles: [] },
  ],
  [
    "styles lint",
    {
      discoveryWarnings: [],
      errors: 0,
      infos: 0,
      messages: [],
      repo,
      styleCount: 0,
      warnings: 0,
    },
  ],
  [
    "styles diff",
    {
      clean: true,
      discoveryWarnings: [],
      modified: [],
      new: [],
      noLockfile: true,
      removed: [],
      repo,
      unchanged: 0,
    },
  ],
  ["styles status", { applyRemove: [], lockfilePresent: false, native: [], repo }],
  ["styles add terse --dry-run", added("styles", "terse", true)],
  ["styles add terse", added("styles", "terse", false)],
  ["styles sync --dry-run", stylesSync(true)],
]);

function flag(name: string, valueType: CommandFlag["valueType"], repeatable = false) {
  return {
    name,
    aliases: [],
    valueType,
    required: false,
    repeatable,
    escalates: false,
    hidden: false,
    sensitive: false,
    effect: "",
  } satisfies CommandFlag;
}

function row(
  id: string,
  tier: "read" | "write" | "destructive",
  options: { dry?: boolean; name?: boolean; client?: boolean; project?: boolean } = {},
): CommandRow {
  const path = id.split(" ");
  return {
    id,
    path,
    group: path[0],
    kind: "command",
    parent: path[0],
    summary: id,
    planned: false,
    tier,
    baseTier: tier,
    dryRun: !!options.dry,
    preview: options.dry
      ? { mode: "flag", flag: "--dry-run" }
      : { mode: "none", flag: null },
    needs: [],
    cost: false,
    surface: "screen",
    operands: options.name ? [{ name: "name", required: true, variadic: false }] : [],
    maxOperands: null,
    flags: [
      flag("--path", "path"),
      ...(options.client ? [flag("--client", "string", true)] : []),
      ...(options.project ? [flag("--project", "bool")] : []),
      ...(options.dry ? [flag("--dry-run", "bool")] : []),
    ],
    operandEscalates: false,
    oneOf: [],
    tools: [],
  };
}

/** The `agents` and `styles` rows of the registry, with the tier and preview flag of the
 * policy table (`src-tauri/tests/fixtures/ctl-envelopes/commands.json`). */
export const agentsCommandRows: CommandRow[] = [
  row("agents add", "write", { dry: true, name: true }),
  row("agents ls", "read"),
  row("agents lint", "read"),
  row("agents audit", "read"),
  row("agents diff", "read"),
  row("agents status", "read"),
  row("agents clean", "destructive", { dry: true, client: true, project: true }),
  row("agents uninstall", "destructive", { dry: true, name: true, project: true }),
  row("agents sync", "write", { dry: true, client: true, project: true }),
  row("styles add", "write", { dry: true, name: true }),
  row("styles ls", "read"),
  row("styles lint", "read"),
  row("styles diff", "read"),
  row("styles status", "read"),
  row("styles sync", "write", { dry: true, client: true, project: true }),
  row("styles apply", "write", { dry: true, name: true, client: true, project: true }),
  row("styles remove", "destructive", { dry: true, client: true, project: true }),
  row("styles clean", "destructive", { dry: true, project: true }),
];
