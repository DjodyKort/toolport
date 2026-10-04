import type { SkillsLsData, SkillsSyncData, SourcesLsData } from "../bridge/data";
import type { SkillsAuditData, SkillsStatusData } from "../types/skills";
import { plusSourcesFixture } from "../fixtures/sources";

/** A synthetic skills library that reproduces the numbers of the acceptance line: 35 entries
 * (33 skills and 2 rules), 1 collision, 2 skills Claude Code does not accept, 1 drifted. */
export const REPO = "/fixture/skills-repo";
export const HOME = "/fixture/home";

const SKILL_NAMES = [
  "api-review", "build-triage", "changelog", "code-explain", "data-migration",
  "db-schema-review", "deploy-helper", "dep-audit", "doc-outline", "doc-review",
  "e2e-runner", "error-triage", "feature-spec", "flaky-tests", "incident-notes",
  "issue-triage", "lint-fixer", "log-analysis", "meeting-notes", "perf-profile",
  "pr-summary", "refactor-plan", "release-notes", "repo-tour", "security-review",
  "spec-batch", "sql-explain", "test-writer", "ticket-writer", "translate-ui",
  "upgrade-guide", "weekly-report", "write-docs",
]; // prettier-ignore
const RULE_NAMES = ["house-style", "commit-rules"];
const INVISIBLE = new Map([
  [
    "incident-notes",
    "the deployed copy is rejected: line 4 continues a quoted multi-line value without indentation",
  ],
  [
    "meeting-notes",
    "the deployed copy is rejected: line 3 continues a quoted multi-line value without indentation",
  ],
]);

export const CLIENTS = ["claude-code", "cursor"];
export const ALL_CLIENTS = [
  "agents-md", "aider", "amazon-q", "claude-code", "cline", "codex-cli", "continue",
  "cursor", "gemini-cli", "goose-cli", "jetbrains", "roo-code", "trae", "windsurf", "zed",
]; // prettier-ignore

const library = { kind: "library", name: "skills-repo" } as const;

export const libraryRows: SkillsLsData["skills"] = [
  ...SKILL_NAMES.map((name) => ({ name, type: "skill", activation: "auto" })),
  ...RULE_NAMES.map((name) => ({ name, type: "rule", activation: "always" })),
]
  .sort((a, b) => a.name.localeCompare(b.name))
  .map((row) => ({
    ...row,
    description: `Use when you need to ${row.name.replace(/-/g, " ")}`,
    invisibleReason: INVISIBLE.get(row.name) ?? null,
    origin: library,
    path: `${REPO}/${row.type === "rule" ? "rules" : "skills"}/${row.name}/SKILL.md`,
    visible: !INVISIBLE.has(row.name),
    writable: true,
  }));

export const libraryLs: SkillsLsData = { repo: REPO, skills: libraryRows };

export const sourcesFixture: SourcesLsData = plusSourcesFixture;

/** The rows of `skills ls --source <id>` for every source of the fixture but the library. */
export function sourceLs(id: string): SkillsLsData | null {
  const source = sourcesFixture.sources.find((row) => row.id === id);
  if (!source) return null;
  const slug = id.replace(/[^a-z0-9]+/g, "-");
  return {
    repo: source.root,
    partial: id.startsWith("vendored"),
    skipped: id.startsWith("vendored")
      ? [{ detector: "vendored", reason: "stopped at the 2 s budget" }]
      : [],
    skills: Array.from({ length: source.counts.skill }, (_, i) => ({
      activation: "auto",
      description: `A ${source.origin.kind} skill number ${i + 1}`,
      invisibleReason: null,
      name: `${slug}-skill-${i + 1}`,
      origin: source.origin,
      path: `${source.root ?? "origin/main"}/${slug}-skill-${i + 1}/SKILL.md`,
      type: "skill",
      visible: true,
      writable: false,
    })),
  };
}

export const collision = {
  skill: "release-notes",
  client: "claude-code",
  action: "kept",
  collisionPath: `${HOME}/.claude/commands/release-notes.md`,
  backupPath: null,
};

export function syncData(clients: string[], dryRun: boolean): SkillsSyncData {
  return {
    backupRoot: `${HOME}/.mcpm-backups`,
    cleaned: [],
    clientCount: clients.length,
    clientSource: "lock",
    collisions: [collision],
    dryRun,
    entries: libraryRows.map((row) => ({
      clientsSynced: clients,
      name: row.name,
      type: row.type,
      warnings:
        row.name === "deploy-helper" && clients.includes("cursor")
          ? ["cursor: 'allowed-tools' field not supported, dropped"]
          : [],
    })),
    globalMode: true,
    kept: 1,
    outputRoot: HOME,
    replaced: 0,
    repo: REPO,
    ruleCount: RULE_NAMES.length,
    skillCount: SKILL_NAMES.length,
    syncedAt: "2026-10-04T10:00:00Z",
    targetedClients: clients,
  };
}

export const statusData: SkillsStatusData = {
  drift: true,
  entries: libraryRows.map((row) => ({
    clientsSynced: CLIENTS,
    currentHash: `sha256:${row.name}`,
    drifted: row.name === "deploy-helper",
    knownToLockfile: true,
    lockfileHash: row.name === "deploy-helper" ? "sha256:old" : `sha256:${row.name}`,
    name: row.name,
    type: row.type,
  })),
  lockedCount: libraryRows.length,
  lockfilePresent: true,
  lockfileSyncedAt: "2026-10-03T09:00:00Z",
  outputRoot: HOME,
  outputs: libraryRows.flatMap((row) =>
    CLIENTS.map((client) => ({
      client,
      name: row.name,
      present: !(row.name === "api-review" && client === "cursor"),
    })),
  ),
  rejected: [...INVISIBLE].map(([name, reason]) => ({
    name,
    client: "claude-code",
    code: "unindented-continuation",
    path: `${HOME}/.claude/skills/${name}/SKILL.md`,
    reason,
  })),
  repo: REPO,
  targetedClients: CLIENTS,
};

export const lintData = {
  errors: 0,
  warnings: 2,
  messages: [
    { level: "warning", name: "deploy-helper", message: "Description is longer than 200 characters" },
    { level: "warning", name: "deploy-helper", message: "No 'when to use' guidance" },
    { level: "info", name: "repo-tour", message: "Consider adding a progressive reference file" },
  ],
}; // prettier-ignore

export const auditData: SkillsAuditData = {
  clean: false,
  findings: [
    {
      severity: "medium",
      skill: "deploy-helper",
      line: 3,
      message: "Suspicious: sudo usage in skill instructions",
    },
  ],
  high: 0,
  low: 0,
  medium: 1,
  repo: REPO,
  skillCount: SKILL_NAMES.length,
};

export const diffData = {
  clean: false,
  modified: ["deploy-helper"],
  new: ["feature-spec"],
  noLockfile: false,
  removed: [],
  repo: REPO,
  unchanged: 33,
};

export const resolveData = (migrate: boolean, dryRun: boolean) => ({
  backupRoot: `${HOME}/.mcpm-backups`,
  collisions: [
    migrate
      ? {
          ...collision,
          action: "replaced",
          backupPath: `${HOME}/.mcpm-backups/.claude/commands/release-notes.md.1`,
        }
      : collision,
  ],
  dryRun,
  kept: migrate ? 0 : 1,
  migrate: migrate ? true : null,
  outputRoot: HOME,
  replaced: migrate ? 1 : 0,
  repo: REPO,
  scope: "global",
  skillCount: SKILL_NAMES.length,
});

export const uninstallData = (name: string, dryRun: boolean) => ({
  dryRun,
  lockDir: "/fixture/data",
  lockUpdated: true,
  name,
  outputRoot: HOME,
  outputs: CLIENTS.map((client) => `${HOME}/.${client}/skills/${name}/SKILL.md`),
  repo: REPO,
  scope: "global",
  sourcePath: `${REPO}/skills/${name}`,
});

export const cleanData = (dryRun: boolean) => ({
  cleanRoot: HOME,
  dryRun,
  ignored: [],
  lockDir: "/fixture/data",
  lockfilePresent: true,
  lockfileRemoved: true,
  managed: SKILL_NAMES,
  removed: [`${HOME}/.claude/skills/api-review/SKILL.md`],
  scope: "global",
  skipped: [],
});

export const addData = (name: string, type: string, dryRun: boolean) => ({
  dryRun,
  files: [`${REPO}/${type === "rule" ? "rules" : "skills"}/${name}/SKILL.md`],
  name,
  path: `${REPO}/${type === "rule" ? "rules" : "skills"}/${name}/SKILL.md`,
  progressive: false,
  repo: REPO,
  type,
});

const sourceReplies = sourcesFixture.sources
  .filter((row) => row.id !== "library" && row.counts.skill > 0)
  .map((row): [string, unknown] => [`skills ls --source ${row.id}`, sourceLs(row.id)]);

/** The read replies of the fixture world, keyed by the argv joined with spaces. */
export const skillsCtlFixtures: Array<[string, unknown]> = [
  ["skills ls", libraryLs],
  ["sources ls", sourcesFixture],
  ["skills status", statusData],
  ["skills lint", lintData],
  ["skills audit", auditData],
  ["skills sync --dry-run", syncData(CLIENTS, true)],
  ["skills resolve --dry-run", resolveData(false, true)],
  ...sourceReplies,
];
