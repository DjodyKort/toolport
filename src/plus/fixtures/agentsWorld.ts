/** A stateful version of `agentsCtlFixtures`: the same argv rows, but an applied write changes
 * what the next read answers (a sync writes the outputs, a clean removes them but keeps the
 * lockfile, a created style shows up in the list). The dev browser fixture and the e2e tests
 * walk a screen through it; the first answer of every row equals the static map. */
import { agentWorld } from "./agents";

const repo = "/fixture/skills-repo";
const home = "/fixture/home";
const data = "/fixture/data";
const CLIENTS = agentWorld.outputs.map((o) => o.client);
const NATIVE = [
  { client: "claude-code", name: "Claude Code" },
  { client: "roomodes-style", name: "Roo Code modes" },
];
const ALWAYS_ON = [
  {
    client: "codex-cli",
    name: "Codex CLI",
    file: ".agents/skills/toolport-style/SKILL.md",
  },
  { client: "cursor", name: "Cursor", file: ".cursor/rules/toolport-style/RULE.md" },
  {
    client: "gemini-cli",
    name: "Gemini CLI",
    file: ".gemini/skills/toolport-style/SKILL.md",
  },
  { client: "windsurf", name: "Windsurf", file: ".windsurf/rules/toolport-style.md" },
];
const STYLE_DESCRIPTION = "Short answers, no preamble";

interface State {
  agents: string[];
  synced: string[] | null;
  present: boolean;
  styles: string[];
  stylesSynced: boolean;
  styleActive: string | null;
}

const fresh = (): State => ({
  agents: [agentWorld.name],
  synced: null,
  present: false,
  styles: [],
  stylesSynced: false,
  styleActive: null,
});

const agentPath = (name: string) => `${repo}/agents/${name}/AGENT.md`;
const outputFiles = (name: string) =>
  agentWorld.outputs.map(({ client, files }) => ({
    client,
    files: files.map((file) => file.replace(agentWorld.name, name)),
  }));
const warningsOf = (name: string) =>
  name === agentWorld.name ? agentWorld.outputs.flatMap((o) => o.warnings) : [];

function agentRow(name: string) {
  if (name === agentWorld.name) {
    const { description, model, tools, path } = agentWorld;
    return { description, model, name, path, tools };
  }
  return {
    description: "Describe what this agent is for",
    model: "",
    name,
    path: agentPath(name),
    tools: [] as string[],
  };
}

export function createAgentsWorld(): Map<string, () => unknown> {
  const s = fresh();
  const rows = new Map<string, () => unknown>();
  const on = (argv: string, reply: () => unknown) => rows.set(argv, reply);

  const outputsOf = (names: string[]) =>
    names.flatMap((name) => outputFiles(name).map((o) => `${home}/${o.files[0]}`));
  const sync = (dryRun: boolean) => ({
    agentCount: s.agents.length,
    agents: s.agents.map((name) => ({
      clientsSynced: CLIENTS,
      found: true,
      model: agentRow(name).model || "inherit",
      name,
      outputFiles: outputFiles(name),
      warnings: warningsOf(name),
    })),
    clientCount: CLIENTS.length,
    discoveryWarnings: [],
    dryRun,
    foundCount: s.agents.length,
    lockDir: data,
    outputRoot: home,
    repo,
    scope: "global",
    syncedAt: "2026-10-04T08:00:00Z",
  });
  const clean = (dryRun: boolean) => ({
    cleanRoot: home,
    dryRun,
    ignored: [],
    lockDir: data,
    lockfilePresent: s.synced !== null,
    managed: s.synced ?? [],
    removed: s.present ? outputsOf(s.synced ?? []) : [],
    scope: "global",
    skipped: [],
  });
  const uninstall = (name: string, dryRun: boolean) => ({
    dryRun,
    lockDir: data,
    lockUpdated: s.synced?.includes(name) ?? false,
    name,
    outputRoot: home,
    outputs: s.present && s.synced?.includes(name) ? outputsOf([name]) : [],
    repo,
    scope: "global",
    sourcePath: `${repo}/agents/${name}`,
  });
  const added = (kind: "agents" | "styles", name: string, dryRun: boolean) => ({
    dryRun,
    name,
    path: `${repo}/${kind}/${name}/${kind === "agents" ? "AGENT" : "STYLE"}.md`,
    repo,
  });

  on("agents ls", () => ({
    agents: s.agents.map(agentRow),
    discoveryWarnings: [],
    repo,
  }));
  on("agents lint", () => ({
    agentCount: s.agents.length,
    discoveryWarnings: [],
    errors: 0,
    infos: 0,
    messages: [],
    repo,
    warnings: 0,
  }));
  on("agents audit", () => ({
    agentCount: s.agents.length,
    clean: true,
    discoveryWarnings: [],
    findings: [],
    high: 0,
    low: 0,
    medium: 0,
    repo,
  }));
  on("agents diff", () => {
    const fresher = s.agents.filter((name) => !s.synced?.includes(name));
    return {
      clean: fresher.length === 0,
      discoveryWarnings: [],
      modified: [],
      new: fresher,
      noLockfile: s.synced === null,
      removed: [],
      repo,
      unchanged: s.agents.length - fresher.length,
    };
  });
  on("agents status", () => ({
    drift: s.synced !== null && !s.present,
    lockedCount: s.synced?.length ?? 0,
    lockfilePresent: s.synced !== null,
    outputRoot: s.synced ? home : null,
    outputs: (s.synced ?? []).flatMap((name) =>
      CLIENTS.map((client) => ({ name, client, present: s.present })),
    ),
    repo,
  }));
  on("agents sync --dry-run", () => sync(true));
  on("agents sync", () => {
    s.synced = [...s.agents];
    s.present = true;
    return sync(false);
  });
  on("agents clean --dry-run", () => clean(true));
  on("agents clean", () => {
    const result = clean(false);
    s.present = false;
    return result;
  });
  on("agents uninstall scout --dry-run", () => uninstall("scout", true));
  on("agents uninstall scout", () => {
    const result = uninstall("scout", false);
    s.agents = s.agents.filter((name) => name !== "scout");
    if (s.synced) s.synced = s.synced.filter((name) => name !== "scout");
    return result;
  });
  on("agents add reviewer --dry-run", () => added("agents", "reviewer", true));
  on("agents add reviewer", () => {
    s.agents = [...s.agents, "reviewer"];
    return added("agents", "reviewer", false);
  });

  const styleRows = () =>
    s.styles.map((name) => ({
      clientsSynced: s.stylesSynced ? NATIVE.map((n) => n.client) : [],
      description: STYLE_DESCRIPTION,
      keepCodingInstructions: false,
      name,
      path: `${repo}/styles/${name}/STYLE.md`,
      synced: s.stylesSynced,
    }));
  const activePairs = () =>
    s.styleActive
      ? ALWAYS_ON.map(({ client }) => ({ client, style: s.styleActive as string }))
      : [];
  const styleFiles = () => [
    ...(s.stylesSynced
      ? [
          `${home}/.claude/output-styles/${s.styles[0] ?? "style"}.md`,
          `${home}/.roomodes`,
        ]
      : []),
    ...(s.styleActive ? ALWAYS_ON.map(({ file }) => `${home}/${file}`) : []),
  ];
  const stylesSync = (dryRun: boolean) => ({
    clientCount: NATIVE.length,
    discoveryWarnings: [],
    dryRun,
    foundCount: s.styles.length,
    lockDir: data,
    outputRoot: home,
    outputs: [
      `${home}/.claude/output-styles/${s.styles[0] ?? "style"}.md`,
      `${home}/.roomodes`,
    ],
    repo,
    scope: "global",
    styleCount: s.styles.length,
    styles: s.styles.map((name) => ({
      clientsSynced: NATIVE.map((n) => n.client),
      description: STYLE_DESCRIPTION,
      name,
      warnings: [],
    })),
    syncedAt: "2026-10-04T08:00:00Z",
  });
  const apply = (name: string, dryRun: boolean) => ({
    active: ALWAYS_ON.map(({ client }) => ({ client, style: name })),
    applied: ALWAYS_ON.map(({ client, file }) => ({ client, path: `${home}/${file}` })),
    appliedCount: ALWAYS_ON.length,
    discoveryWarnings: [],
    dryRun,
    lockDir: data,
    name,
    nativeClients: [],
    outputRoot: home,
    replaced: [],
    repo,
    scope: "global",
  });
  const remove = (dryRun: boolean) => ({
    active: [],
    clientKeys: [],
    dryRun,
    hadActive: s.styleActive !== null,
    lockDir: data,
    outputRoot: home,
    removed: s.styleActive
      ? ALWAYS_ON.map(({ client, file }) => ({
          client,
          path: `${home}/${file}`,
          style: s.styleActive as string,
        }))
      : [],
    repo,
    scope: "global",
  });
  const cleanStyles = (dryRun: boolean) => ({
    cleanRoot: home,
    dryRun,
    ignored: [],
    lockDir: data,
    lockUpdated: !dryRun,
    lockfilePresent: s.stylesSynced || s.styleActive !== null,
    managed: s.styles,
    removed: styleFiles(),
    scope: "global",
    skipped: [],
  });

  on("styles ls", () => ({
    active: activePairs(),
    discoveryWarnings: [],
    lockfilePresent: s.stylesSynced || s.styleActive !== null,
    repo,
    styles: styleRows(),
  }));
  on("styles lint", () => ({
    discoveryWarnings: [],
    errors: 0,
    infos: 0,
    messages: [],
    repo,
    styleCount: s.styles.length,
    warnings: 0,
  }));
  on("styles diff", () => {
    const fresher = s.stylesSynced ? [] : s.styles;
    return {
      clean: fresher.length === 0,
      discoveryWarnings: [],
      modified: [],
      new: fresher,
      noLockfile: !s.stylesSynced && s.styleActive === null,
      removed: [],
      repo,
      unchanged: s.styles.length - fresher.length,
    };
  });
  on("styles status", () => ({
    applyRemove: s.styleActive
      ? ALWAYS_ON.map(({ client, name }) => ({ client, name, active: s.styleActive }))
      : [],
    lockfilePresent: s.stylesSynced || s.styleActive !== null,
    native: s.stylesSynced
      ? NATIVE.map(({ client, name }) => ({ client, name, styles: s.styles }))
      : [],
    repo,
  }));
  on("styles add terse --dry-run", () => added("styles", "terse", true));
  on("styles add terse", () => {
    s.styles = [...s.styles, "terse"];
    return added("styles", "terse", false);
  });
  on("styles sync --dry-run", () => stylesSync(true));
  on("styles sync", () => {
    s.stylesSynced = true;
    return stylesSync(false);
  });
  on("styles apply terse --dry-run", () => apply("terse", true));
  on("styles apply terse", () => {
    s.styleActive = "terse";
    return apply("terse", false);
  });
  on("styles remove --dry-run", () => remove(true));
  on("styles remove", () => {
    const result = remove(false);
    s.styleActive = null;
    return result;
  });
  on("styles clean --dry-run", () => cleanStyles(true));
  on("styles clean", () => {
    const result = cleanStyles(false);
    s.stylesSynced = false;
    s.styleActive = null;
    return result;
  });
  return rows;
}
