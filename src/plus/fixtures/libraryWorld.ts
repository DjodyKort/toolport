import type { SkillEntry, SkillsInventory } from "@/lib/types";
import type { CtlReply } from "./ctlWorld";
import { createSelectWorld, type SelectScenario } from "./selectWorld";
import type {
  AuditFinding,
  AutoLastRun,
  AutoSchedule,
  LibrarySkill,
  LintRow,
  PlanChange,
  SkillFile,
  SkillLink,
  SourceRow,
  SyncData,
  TapRow,
} from "../library/data";

export const LIBRARY_HOME = "/Users/example";
export const LIBRARY_REPO = `${LIBRARY_HOME}/work/skills-library`;
const OUT = LIBRARY_HOME;
const TOTAL_FILES = 183;
const SNAPSHOT_AT = "2026-11-16T17:42:00.000Z";

export type LibraryScenario =
  "plan" | "done" | "held" | "blocked" | "behind" | "empty" | "catchup" | "busy";

/** Where auto-sync stands: the shipped task is off (the default), on with a clean last run, on
 * with a last run that held changes, on with a last run that failed, or paused after an Undo. */
export type LibraryAuto = "off" | "on" | "held" | "failed" | "paused";

export interface LibraryWorldOptions {
  scenario?: LibraryScenario;
  auto?: LibraryAuto;
  /** A hook script that is not executable: an error row with the chmod fix. */
  chmod?: boolean;
  /** The verification after Apply finds this many files that differ. */
  drift?: number;
  /** The first Apply leaves one agent step failed; an Apply limited with --only runs it again. */
  failedStep?: boolean;
  /** The push is refused by the secret scan. */
  secret?: boolean;
  /** The remote has uncommitted work as well as new commits. */
  dirty?: boolean;
  /** The library holds one output style. */
  styles?: boolean;
  /** `styles status` cannot read the lock. */
  styleStatusFails?: boolean;
  /** An agent file whose description is over the limit: it does not parse and lint says so. */
  longAgent?: boolean;
  /** The output style plain is on for Cursor and Zed (it also puts the style in the library). */
  applied?: boolean;
  /** The plan also writes the agent code-scout and the output style plain. */
  extras?: boolean;
  /** Where the selective-loading source `acme-skills` stands. */
  select?: SelectScenario;
}

const SKILLS: Array<[string, string]> = [
  ["api-review", "Review an API change for naming and compatibility"],
  ["build-triage", "Sort failing builds by cause"],
  ["changelog", "Draft a changelog from merged pull requests"],
  ["code-explain", "Explain unfamiliar code with a short walkthrough"],
  ["db-migrate-check", "Check a database migration for locks and rollbacks"],
  ["deploy-notes", "Write deployment notes from a release diff"],
  ["doc-outline", "Outline a long document before writing it"],
  ["env-audit", "List environment variables a project reads"],
  ["flaky-test-hunt", "Find tests that fail intermittently"],
  ["git-tidy", "Clean up local branches that are already merged"],
  ["incident-notes", "Keep a timeline while an incident is running"],
  ["issue-groomer", "Sort new issues by area and severity"],
  ["log-reader", "Read a log file and report what stands out"],
  ["old-notes", "Notes from an earlier project"],
  ["onboarding-tour", "Walk a new teammate through a repository"],
  ["pr-writer", "Write a pull request description from a diff"],
  ["release-notes", "Write release notes from merged pull requests since the last tag."],
  ["review-round", "Handle one round of review comments on a pull request"],
  ["runbook-draft", "Turn a repeated task into a runbook"],
  ["schema-diff", "Compare two schemas and list what changed"],
  ["screenshot", "Take and annotate screenshots of the running app for bug reports."],
  ["security-skim", "Skim a change for risky patterns"],
  ["spec-writer", "Score a written spec against a rubric and list what is missing."],
  ["task-runner", "Run a named task and report each step"],
  ["test-plan", "Draft a test plan from a change"],
  ["ticket-intake", "Turn a customer message into a ticket"],
  ["triage", "Sort new issues by area and severity."],
  ["ui-polish", "Check a screen for spacing and contrast"],
  ["upgrade-guide", "Write an upgrade guide between two versions"],
  ["vendor-scan", "List third-party code a project vendors"],
  ["weekly-digest", "Summarise a week of commits"],
  ["yaml-lint", "Find mistakes in a YAML file"],
];
const RULES: Array<[string, string]> = [
  ["code-style", "House style for code"],
  ["house-voice", "How replies are worded"],
];

const HELD_BY_LINT = ["ticket-intake", "release-notes", "review-round"];
const TAP_SKILLS = ["quick-deploy"];
/** The one tap that is also a Claude Code plugin marketplace. */
const MARKET_TAP = "acme-skills";

/** The clients that take an output style as an always-on rule, with the file each one gets. */
const STYLE_CLIENTS: Array<{ key: string; name: string; path: string }> = [
  { key: "aider", name: "Aider", path: ".mcpm/skills/mcpm-output-style/SKILL.md" },
  { key: "amazon-q", name: "Amazon Q", path: ".amazonq/rules/mcpm-output-style.md" },
  { key: "cline", name: "Cline", path: ".clinerules/mcpm-output-style.md" },
  {
    key: "codex-cli",
    name: "Codex CLI",
    path: ".agents/skills/mcpm-output-style/SKILL.md",
  },
  { key: "continue", name: "Continue.dev", path: ".continue/rules/mcpm-output-style.md" },
  { key: "cursor", name: "Cursor", path: ".cursor/rules/mcpm-output-style/RULE.md" },
  {
    key: "gemini-cli",
    name: "Gemini CLI",
    path: ".gemini/skills/mcpm-output-style/SKILL.md",
  },
  { key: "goose", name: "Goose", path: ".goose/rules/mcpm-output-style.md" },
  {
    key: "jetbrains",
    name: "JetBrains AI",
    path: ".aiassistant/rules/mcpm-output-style.md",
  },
  { key: "trae", name: "Trae", path: ".trae/rules/mcpm-output-style.md" },
  {
    key: "vscode-copilot",
    name: "VS Code Copilot",
    path: ".github/instructions/mcpm-output-style.instructions.md",
  },
  { key: "windsurf", name: "Windsurf", path: ".windsurf/rules/mcpm-output-style.md" },
  { key: "zed", name: "Zed", path: ".rules" },
];

const folder = (skill: string) => `${OUT}/.claude/skills/${skill}`;

function change(
  skill: string,
  path: string,
  op: string,
  held = false,
  client = "claude-code",
): PlanChange {
  return { skill, client, path, op, held };
}

const path = (skill: string, file: string) => `.claude/skills/${skill}/${file}`;

function planScenario(): PlanChange[] {
  return [
    ...[
      "SKILL.md",
      "SETUP.md",
      "references/causes.md",
      "references/examples.md",
      "scripts/triage.py",
      "templates/report/.gitignore",
      "templates/report/report.md",
    ].map((file) => change("build-triage", path("build-triage", file), "add")),
    change("api-review", path("api-review", "SKILL.md"), "update"),
    change("api-review", path("api-review", "references/checklist.md"), "update"),
    change("spec-writer", path("spec-writer", "SKILL.md"), "update"),
    change("spec-writer", path("spec-writer", "references/rubric.md"), "add"),
    change("task-runner", path("task-runner", "SKILL.md"), "update"),
    change("task-runner", path("task-runner", "scripts/on-stop.sh"), "update"),
    change("task-runner", ".claude/settings.json#Stop", "hook", true),
    change("changelog", path("changelog", "SKILL.md"), "edited", true),
    change("log-reader", path("log-reader", "references/notes.md"), "edited", true),
    ...["SKILL.md", "references/a.md", "references/b.md", "notes/summary.md"].map(
      (file) => change("old-notes", path("old-notes", file), "remove", true),
    ),
    change("code-style", ".claude/rules/code-style.md", "update"),
  ];
}

function heldScenario(): PlanChange[] {
  return [
    change("api-review", path("api-review", "SKILL.md"), "update"),
    ...["SKILL.md", "references/a.md", "references/b.md", "notes/summary.md"].map(
      (file) => change("old-notes", path("old-notes", file), "remove", true),
    ),
    change("task-runner", ".claude/settings.json#Stop", "hook", true),
  ];
}

function behindScenario(): PlanChange[] {
  return [change("weekly-digest", path("weekly-digest", "SKILL.md"), "update")];
}

function catchupScenario(): PlanChange[] {
  const adds = ["spec-writer", "api-review", "security-skim", "runbook-draft"].flatMap(
    (skill, index) =>
      Array.from({ length: 5 + index }, (_, n) =>
        change(
          skill,
          path(skill, n === 0 ? "SKILL.md" : `references/file-${n}.md`),
          "add",
        ),
      ),
  );
  return [
    ...adds,
    ...["SETUP.md", "references/env-list.md", "references/ci.md", "scripts/list.sh"].map(
      (file) =>
        change(
          "env-audit",
          path("env-audit", file),
          file === "SETUP.md" ? "differs" : "add",
          file === "SETUP.md",
        ),
    ),
    ...["a", "b", "c", "d", "e"].map((n) =>
      change("schema-diff", path("schema-diff", `references/${n}.md`), "adopt"),
    ),
  ];
}

const lintRow = (
  row: Partial<LintRow> & Pick<LintRow, "code" | "name" | "message">,
): LintRow => ({
  level: "warning",
  consequence: "other",
  ...row,
});

function notCopied(): LintRow[] {
  const owners = SKILLS.slice(0, 12).map(([name]) => name);
  return Array.from({ length: 31 }, (_, index) =>
    lintRow({
      code: "file-not-copied",
      consequence: "not-copied",
      level: "info",
      name: owners[index % owners.length],
      file: index % 3 === 0 ? ".DS_Store" : `__pycache__/file${index}.pyc`,
      message: "not installed by a sync: on the deny-list",
      reason: "on the deny-list",
    }),
  );
}

function fullLint(chmod: boolean): LintRow[] {
  return [
    lintRow({
      code: "description-too-long",
      consequence: "cut",
      level: "error",
      name: "ticket-intake",
      file: "SKILL.md",
      message:
        "Description (1,200) and when_to_use (402) add up to 1,602 characters. Claude Code keeps 1,536, so the last 66 are never read.",
      meter: {
        length: 1602,
        description: 1200,
        whenToUse: 402,
        limit: 1536,
        specLimit: 1024,
      },
    }),
    lintRow({
      code: "name-missing",
      consequence: "hidden",
      level: "error",
      name: "release-notes",
      file: "SKILL.md",
      message: "No name in the frontmatter. Claude Code hides the skill from the model.",
    }),
    lintRow({
      code: "frontmatter-invalid-yaml",
      consequence: "hidden",
      level: "error",
      name: "review-round",
      file: "SKILL.md",
      line: 4,
      message:
        "A quoted multi-line value starts in column 0 (line 4). Claude Code cannot read the file and hides the skill without a message.",
    }),
    ...(chmod
      ? [
          lintRow({
            code: "hook-not-executable",
            consequence: "hook-wont-run",
            level: "error",
            name: "task-runner",
            file: "scripts/on-stop.sh",
            fixable: "chmod",
            message:
              "Hook script 'scripts/on-stop.sh' of the Stop hook is not executable.",
          }),
        ]
      : []),
    lintRow({
      code: "description-over-spec",
      consequence: "over-spec",
      name: "build-triage",
      file: "SKILL.md",
      message:
        "Description is 1,310 characters, above the Agent Skills limit of 1,024. Claude Code reads up to 1,536 and still sees all of it. Other clients may cut it.",
      meter: {
        length: 1310,
        description: 1310,
        whenToUse: 0,
        limit: 1536,
        specLimit: 1024,
      },
    }),
    lintRow({
      code: "link-leaves-folder",
      consequence: "dead-link",
      name: "spec-writer",
      file: "SKILL.md",
      line: 31,
      target: "../shared/glossary.md",
      fixable: "move-link",
      message:
        "SKILL.md links ../shared/glossary.md, outside the skill folder. That file will not exist in ~/.claude/skills.",
    }),
    lintRow({
      code: "body-large",
      consequence: "context-cost",
      name: "pr-writer",
      file: "SKILL.md",
      message:
        "Body is 612 lines. The whole body loads when the skill triggers. Consider moving detail into references/.",
    }),
    lintRow({
      code: "description-no-when",
      consequence: "weak-trigger",
      level: "info",
      name: "doc-outline",
      file: "SKILL.md",
      message:
        "Description lacks 'when to use' guidance. Claude decides from the description alone.",
    }),
    ...notCopied(),
  ];
}

function calmLint(): LintRow[] {
  return notCopied().slice(0, 4);
}

const consequences = (rows: LintRow[]) => {
  const out: Record<string, number> = {};
  for (const row of rows) out[row.consequence] = (out[row.consequence] ?? 0) + 1;
  return out;
};

function filesOf(name: string, changes: PlanChange[]): SkillFile[] {
  const own = changes.filter((c) => c.skill === name);
  const prefix = `.claude/skills/${name}/`;
  const status = (op: string) =>
    op === "remove"
      ? "removed-on-next-sync"
      : op === "edited"
        ? "hand-edited"
        : op === "differs"
          ? "differs"
          : op === "adopt"
            ? "copied"
            : "would-copy";
  const planned: SkillFile[] = own
    .filter((c) => c.op !== "hook")
    .map((c) => ({
      client: c.client,
      op: c.op,
      path: c.path.startsWith(prefix) ? c.path.slice(prefix.length) : c.path,
      reason: null,
      size: 600 + ((c.path.length * 37) % 4200),
      status: status(c.op),
    }));
  if (name === "spec-writer") {
    return [
      ...planned,
      {
        client: "claude-code",
        op: "unchanged",
        path: "SETUP.md",
        reason: null,
        size: 1433,
        status: "copied",
      },
      {
        client: "claude-code",
        op: "unchanged",
        path: "references/examples/good.md",
        reason: null,
        size: 2150,
        status: "copied",
      },
      {
        client: "claude-code",
        op: "unchanged",
        path: "references/examples/weak.md",
        reason: null,
        size: 1843,
        status: "copied",
      },
      {
        client: "claude-code",
        op: "unchanged",
        path: "templates/report/report.md",
        reason: null,
        size: 1126,
        status: "copied",
      },
      {
        client: "claude-code",
        op: "unchanged",
        path: "scripts/render.py",
        reason: null,
        size: 2662,
        status: "copied",
      },
      {
        client: "claude-code",
        op: "unchanged",
        path: "assets/flow.png",
        reason: null,
        size: 38912,
        status: "copied",
      },
      {
        client: null,
        op: null,
        path: ".DS_Store",
        reason: "on the deny-list (.DS_Store)",
        size: 6148,
        status: "not-copied",
      },
      {
        client: null,
        op: null,
        path: "__pycache__/render.cpython-313.pyc",
        reason: "on the deny-list (__pycache__/)",
        size: 3072,
        status: "not-copied",
      },
    ];
  }
  if (planned.some((f) => f.path === "SKILL.md")) return planned;
  return [
    {
      client: "claude-code",
      op: "unchanged",
      path: "SKILL.md",
      reason: null,
      size: 3174,
      status: "copied",
    },
    ...planned,
  ];
}

function linksOf(name: string): SkillLink[] {
  if (name !== "spec-writer") return [];
  return [
    {
      line: 31,
      state: "outside-folder",
      target: "../shared/glossary.md",
      reason: "outside the skill folder",
      fixable: "move-link",
    },
    {
      line: 44,
      state: "missing",
      target: "references/old-rubric.md",
      reason: "the file does not exist",
    },
    { line: 12, state: "installed", target: "references/rubric.md", reason: null },
  ];
}

const DENY = [
  ".git/",
  "__pycache__/",
  "node_modules/",
  ".venv/",
  ".pytest_cache/",
  "dist/",
  "build/",
  "target/",
  ".DS_Store",
  "*.pyc",
  "*.log",
];

function inventoryOf(): SkillsInventory {
  const entry = (name: string, state: "on" | "name-only" | "off"): SkillEntry => ({
    key: `${LIBRARY_HOME}/.claude/skills/${name}/SKILL.md`,
    name,
    description: "",
    descriptionChars: 0,
    path: `~/.claude/skills/${name}/SKILL.md`,
    folder: folder(name),
    source: "library",
    sourceNote: "Library",
    editable: true,
    removable: false,
    trashOnRemove: false,
    sizeBytes: 3000,
    lines: 80,
    modifiedSecs: null,
    gitRepo: null,
    links: [],
    states: { "claude-code": { state, via: "~/.claude/skills" } },
    preview: "",
  });
  const names = SKILLS.map(([name]) => name).filter(
    (name) => !HELD_BY_LINT.includes(name) && name !== "build-triage",
  );
  return {
    agents: [],
    project: null,
    skills: names.map((name) =>
      entry(name, name === "screenshot" ? "off" : name === "triage" ? "name-only" : "on"),
    ),
    rules: [],
    issues: [],
  };
}

const SOURCES: SourceRow[] = [
  src(
    "library:skills-library",
    "library",
    "skills-library",
    "library",
    LIBRARY_REPO,
    true,
    { skill: 32, rule: 2 },
    "ok",
    "34 items · in sync with origin",
  ),
  src(
    "repo:project-a",
    "repo",
    "~/work/project-a",
    "project",
    `${LIBRARY_HOME}/work/project-a`,
    false,
    { skill: 3, memory: 1 },
    "behind",
    "114 commits behind origin/main",
  ),
  src(
    "client:project-b",
    "client",
    "~/work/project-b",
    "project",
    `${LIBRARY_HOME}/work/project-b`,
    false,
    { skill: 2, command: 1 },
    "ok",
    "2 skills, 1 command",
  ),
  src(
    "duplicate:old-copy",
    "duplicate",
    "~/work/old-library-copy",
    "project",
    `${LIBRARY_HOME}/work/old-library-copy`,
    false,
    { skill: 34 },
    "duplicate",
    "same remote as your library",
  ),
  src(
    "plugin:kit@acme",
    "plugin",
    "kit@acme",
    "plugin",
    "",
    false,
    { skill: 12, agent: 2 },
    "ok",
    "12 skills, 2 agents",
  ),
  src(
    "plugin:toolkit-pro@community",
    "plugin",
    "toolkit-pro@community",
    "plugin",
    "",
    false,
    { skill: 290 },
    "ok",
    "290 skills, listed in Skills only",
  ),
  src(
    "tap:acme-skills",
    "tap",
    "acme-skills",
    "third-party",
    "",
    false,
    { skill: 14 },
    "ok",
    "14 skills",
  ),
  src(
    "tap:community-tools",
    "tap",
    "community-tools",
    "third-party",
    "",
    false,
    { skill: 3 },
    "failed",
    "Could not update: git asked for a password. Run `gh auth login`, then retry.",
  ),
  src(
    "account:claude",
    "account",
    "claude.ai account",
    "account",
    "",
    false,
    { skill: 2 },
    "ok",
    "2 skills",
  ),
  src(
    "loose:claude",
    "loose",
    "~/.claude",
    "user",
    `${LIBRARY_HOME}/.claude`,
    false,
    { skill: 2 },
    "ok",
    "2 skills (hand-written)",
  ),
];

function src(
  id: string,
  kind: string,
  name: string,
  owner: string,
  root: string,
  writable: boolean,
  counts: Record<string, number>,
  state: string,
  detail: string,
): SourceRow {
  return {
    id,
    origin: { kind, name },
    detector: kind,
    owner,
    root,
    writable,
    counts,
    status: { state, detail },
    freshness: null,
    warnings: [],
    tokens: { basis: "estimate", value: 10 * (counts.skill ?? 1) },
  };
}

/** What the two copies of a held file hold, as the Compare dialog reads them. */
const COMPARE_TEXT: Record<string, { library: string; installed: string; diff: string }> =
  {
    "changelog/SKILL.md": {
      library:
        "---\nname: changelog\n---\n\nWrite one line per change.\nGroup by area.\n",
      installed:
        "---\nname: changelog\n---\n\nWrite one line per change.\nGroup by area.\nMention the ticket id.\n",
      diff: "--- library\n+++ installed\n@@ -3,3 +3,4 @@\n \n Write one line per change.\n Group by area.\n+Mention the ticket id.\n",
    },
    "log-reader/references/notes.md": {
      library: "# Notes\nRead the newest log first.\n",
      installed: "# Notes\nRead the newest log first, then the one before.\n",
      diff: "--- library\n+++ installed\n@@ -1,2 +1,2 @@\n # Notes\n-Read the newest log first.\n+Read the newest log first, then the one before.\n",
    },
  };

/** The part of a change path below the skill folder, as the files list names it. */
const inSkill = (c: PlanChange) => {
  const prefix = `.claude/skills/${c.skill}/`;
  return c.path.startsWith(prefix) ? c.path.slice(prefix.length) : c.path;
};

const ok = (data: unknown): CtlReply => ({ ok: true, data });
const fail = (code: string, message: string, data?: unknown): CtlReply => ({
  ok: false,
  error: { code, message },
  data,
});

const flag = (argv: string[], name: string) => {
  const at = argv.indexOf(name);
  return at >= 0 ? (argv[at + 1] ?? null) : null;
};
const operand = (argv: string[], from: number) =>
  argv
    .slice(from)
    .find((a, i, all) => !a.startsWith("--") && !(all[i - 1] ?? "").startsWith("--")) ??
  null;

function plan(
  summary: string,
  steps: Array<{ op: string; detail: string; path?: string }>,
  undo: string,
) {
  return { summary, steps, warnings: [] as string[], undo, effects: {} };
}

/** A small in-memory toolportctl for the Library screen: the library, the lock, the plan and the
 * snapshot of the last sync, the remote, taps, sources, agents and styles. A second read sees the
 * first write. */
export function createLibraryWorld(options: LibraryWorldOptions = {}) {
  const scenario = options.scenario ?? "plan";
  const selectWorld = createSelectWorld(options.select);
  let items: LibrarySkill[] = [
    ...SKILLS.map(([name, description]) => item(name, "skill", description)),
    ...RULES.map(([name, description]) => item(name, "rule", description)),
  ];
  let changes: PlanChange[] =
    scenario === "plan" || scenario === "blocked" || scenario === "busy"
      ? planScenario()
      : scenario === "held"
        ? heldScenario()
        : scenario === "behind"
          ? behindScenario()
          : scenario === "catchup"
            ? catchupScenario()
            : [];
  if (options.extras)
    changes = [
      ...changes,
      {
        kind: "agent",
        skill: "code-scout",
        client: "claude-code",
        path: ".claude/agents/code-scout.md",
        op: "add",
        held: false,
      },
      {
        kind: "style",
        skill: "plain",
        client: "claude-code",
        path: ".claude/output-styles/plain.md",
        op: "add",
        held: false,
      },
    ];
  const fresh = new Set(changes.filter((c) => c.op === "add").map((c) => c.skill));
  const locked = new Set(
    scenario === "catchup"
      ? []
      : items
          .map((i) => i.name)
          .filter((n) => !HELD_BY_LINT.includes(n) && !fresh.has(n)),
  );
  let lockfile = scenario !== "catchup";
  let snapshot: {
    id: string;
    files: number;
    hooks: number;
    changes: PlanChange[];
  } | null =
    scenario === "done" || scenario === "held"
      ? {
          id: "20261116T174200",
          files: 12,
          hooks: 0,
          changes: [
            change("weekly-digest", path("weekly-digest", "SKILL.md"), "update"),
            change("env-audit", path("env-audit", "references/env-list.md"), "add"),
          ],
        }
      : null;
  const autoState = options.auto ?? "off";
  let pause: { reason: string; since: string } | null =
    autoState === "paused" ? { reason: "undo", since: "2026-11-17T09:00:00.000Z" } : null;
  let autoOn = autoState !== "off";
  let lastRun: AutoLastRun | null = null;
  let taskRun: AutoSchedule["lastRun"] = null;
  let connected = scenario !== "empty";
  let published = true;
  let remote = {
    ahead: 0,
    behind: scenario === "behind" ? 3 : 0,
    uncommitted: options.dirty ? 2 : scenario === "plan" ? 2 : 0,
    hasRemote: scenario !== "empty",
  };
  let clients = ["claude-code"];
  const kept = new Set<string>();
  let taps: TapRow[] = [
    tap("acme-skills", "acme-co/agent-skills"),
    tap("community-tools", "community/toolbox"),
  ];
  let tapFails = true;
  const linkedTaps = new Set<string>();
  let roots = [
    {
      path: `${LIBRARY_HOME}/work/project-a`,
      exists: true,
      origin: "default",
      repo: true,
    },
    {
      path: `${LIBRARY_HOME}/work/clients`,
      exists: true,
      origin: "default",
      repo: false,
    },
    {
      path: `${LIBRARY_HOME}/work/old-library-copy`,
      exists: true,
      origin: "config",
      repo: false,
    },
  ];
  let agents = [
    {
      name: "code-scout",
      description: "Reads a repository and reports what it finds.",
      model: "sonnet",
      path: `${LIBRARY_REPO}/agents/code-scout/AGENT.md`,
      tools: ["Read", "Grep"],
    },
  ];
  let agentSynced = true;
  let styles =
    options.styles || options.extras || options.applied
      ? [
          {
            name: "plain",
            description: "Short answers without preamble",
            path: `${LIBRARY_REPO}/styles/plain/STYLE.md`,
            synced: false,
            clientsSynced: [] as string[],
          },
        ]
      : [];
  const bodies = new Map<string, string>([
    ["agents:code-scout", "You read a repository and report what you find.\n"],
    ["styles:plain", "Answer in two short sentences.\n"],
  ]);
  const edited = new Set<string>();
  const activeStyles = new Map<string, string>(
    options.applied
      ? [
          ["cursor", "plain"],
          ["zed", "plain"],
        ]
      : [],
  );
  const drift = options.drift ?? 0;

  function item(name: string, type: string, description: string): LibrarySkill {
    return {
      name,
      type,
      description,
      activation: type === "rule" ? "always" : "auto",
      visible: !HELD_BY_LINT.includes(name),
      invisibleReason: HELD_BY_LINT.includes(name) ? "held back by lint" : null,
      writable: true,
      path: `${LIBRARY_REPO}/${type === "rule" ? "rules" : "skills"}/${name}/SKILL.md`,
      origin: {
        kind: TAP_SKILLS.includes(name) ? "tap" : "library",
        name: "skills-library",
      },
    };
  }
  function tap(name: string, repo: string): TapRow {
    return {
      name,
      repo,
      url: `https://github.com/${repo}.git`,
      path: `${LIBRARY_HOME}/Library/Application Support/Toolport/taps/${name}`,
      cloned: true,
    };
  }

  let moved = false;
  const lintRows = () =>
    scenario === "plan" || scenario === "blocked" || scenario === "busy"
      ? fullLint(options.chmod === true).filter(
          (r) => !(moved && r.code === "link-leaves-folder"),
        )
      : options.chmod
        ? [
            ...calmLint(),
            ...fullLint(true).filter((r) => r.code === "hook-not-executable"),
          ]
        : calmLint();

  const rowsHeld = (list: PlanChange[]) =>
    list
      .filter((c) => c.held)
      .map((c) => ({
        skill: c.skill,
        client: c.client,
        path: c.path,
        op: c.op,
        reason: c.op === "remove" ? "removal" : c.op === "hook" ? "new-hook" : c.op,
      }));

  function visibleChanges(): PlanChange[] {
    const extra = clients.includes("agents-md")
      ? [change("code-style", "AGENTS.md", "add", false, "agents-md")]
      : [];
    return [...changes, ...extra].filter((c) => !kept.has(`${c.skill}/${inSkill(c)}`));
  }

  const counts = (list: PlanChange[]) => {
    const n = (op: string) => list.filter((c) => c.op === op).length;
    return {
      add: n("add"),
      update: n("update"),
      adopt: n("adopt"),
      differs: n("differs"),
      edited: n("edited"),
      remove: n("remove"),
      hook: n("hook"),
      kept: kept.size,
      unchanged: Math.max(0, items.length - new Set(list.map((c) => c.skill)).size),
    };
  };

  function syncData(dryRun: boolean, extra: Partial<SyncData> = {}): SyncData {
    const list = visibleChanges();
    return {
      dryRun,
      unattended: false,
      paused: false,
      repo: LIBRARY_REPO,
      skillCount: items.filter((i) => i.type === "skill").length,
      ruleCount: items.filter((i) => i.type === "rule").length,
      targetedClients: clients,
      entries: items.map((i) => ({
        name: i.name,
        type: i.type,
        clientsSynced: locked.has(i.name) ? clients : [],
        warnings: [],
      })),
      collisions: [],
      replaced: 0,
      kept: 0,
      plan: {
        counts: counts(list),
        held: list.filter((c) => c.held).length,
        changes: list,
        skipped: [],
      },
      held: rowsHeld(list),
      snapshot: null,
      verify: null,
      syncedAt: "2026-11-17T08:58:00.000Z",
      outputRoot: OUT,
      backupRoot: `${OUT}/.mcpm-backups`,
      ...extra,
    };
  }

  function sync(argv: string[]): CtlReply {
    if (!connected) return fail("not_found", "no skills repository found");
    if (scenario === "busy")
      return fail("conflict", "another sync holds the lock (started 07:00:12)");
    if (scenario === "blocked")
      return fail("skills", `Permission denied (os error 13): ${OUT}/.claude/skills`);
    const dryRun = argv.includes("--dry-run");
    const unattended = argv.includes("--unattended");
    const applyHeld = argv.includes("--apply-held");
    const allowTap = argv.includes("--allow-tap-hooks");
    const values = (name: string) =>
      argv.flatMap((a, i) => (a === name ? [argv[i + 1]] : []));
    const only = values("--only");
    const take = values("--take");
    const asked = values("--client");
    if (asked.length) clients = asked;
    if (unattended && pause)
      return ok({
        dryRun,
        held: [],
        paused: true,
        skipped: true,
        message: `auto-sync is paused (reason: ${pause.reason}); resume it with \`skills auto resume\``,
        reason: pause.reason,
        since: pause.since,
        unattended: true,
        snapshot: null,
        verify: null,
      });
    if (unattended && applyHeld)
      return fail("usage", "an unattended sync never applies held changes");
    if (dryRun)
      return ok(
        syncData(true, {
          unattended,
          held: unattended && scenario !== "held" ? [] : rowsHeld(visibleChanges()),
        }),
      );
    const list = visibleChanges();
    const apply = list.filter(
      (c) =>
        (only.length === 0 || only.includes(c.skill)) &&
        (!c.held ||
          take.includes(`${c.skill}/${inSkill(c)}`) ||
          (!unattended &&
            applyHeld &&
            (c.op !== "hook" || !TAP_SKILLS.includes(c.skill) || allowTap))),
    );
    const files = apply.filter((c) => c.op !== "hook").length;
    const hooks = apply.filter((c) => c.op === "hook").length;
    if (apply.length) {
      snapshot = {
        id: "20261117T085800",
        files: files + 4,
        hooks,
        changes: apply,
      };
      for (const c of apply) {
        if (c.kind) continue;
        locked.add(c.skill);
        if (c.op === "remove") {
          locked.delete(c.skill);
          items = items.filter((i) => i.name !== c.skill);
        }
      }
      changes = changes.filter((c) => !apply.includes(c));
      lockfile = true;
    }
    const stillHeld = rowsHeld(visibleChanges());
    if (unattended)
      recordRun({
        at: new Date().toISOString(),
        applied: apply.length,
        held: stillHeld,
        skipped: 0,
      });
    else if (lastRun)
      lastRun = {
        ...lastRun,
        held: lastRun.held.filter((h) =>
          stillHeld.some(
            (s) =>
              s.skill === h.skill &&
              s.client === h.client &&
              s.path === h.path &&
              s.op === h.op,
          ),
        ),
      };
    const total = TOTAL_FILES;
    const mismatched = Array.from({ length: drift }, (_, n) => ({
      client: "claude-code",
      path: n === 0 ? "references/checklist.md" : "references/notes.md",
      reason: "differs",
      skill: n === 0 ? "api-review" : "log-reader",
    }));
    const done = syncData(false, {
      unattended,
      snapshot:
        snapshot && apply.length
          ? { id: snapshot.id, files: snapshot.files, hooks: snapshot.hooks }
          : null,
      verify: {
        clean: drift === 0,
        matched: total - drift,
        total,
        mismatched,
      },
    });
    done.plan = {
      ...done.plan,
      changes: apply,
      held: apply.filter((c) => c.held).length,
      counts: counts(apply),
    };
    done.held = rowsHeld(visibleChanges());
    if (options.failedStep && only.length === 0)
      done.plan.failed = [
        {
          id: "agent:code-scout",
          kind: "agent",
          name: "code-scout",
          client: "claude-code",
          step: "files",
          message: "the agents folder is not writable",
        },
      ];
    return ok(done);
  }

  function undo(argv: string[]): CtlReply {
    if (!snapshot) return fail("not_found", "no snapshot to undo: run skills sync first");
    const dryRun = argv.includes("--dry-run");
    const files = snapshot.changes
      .filter((c) => c.op !== "hook")
      .map((c) => ({
        action: c.op === "add" ? "remove" : "restore",
        path: c.path,
        reason: null as string | null,
      }));
    const data = {
      dryRun,
      files,
      hooks: [] as unknown[],
      removed: files.filter((f) => f.action === "remove").length,
      restored: files.filter((f) => f.action === "restore").length,
      conflicts: 0,
      snapshot: {
        id: snapshot.id,
        files: snapshot.files,
        hooks: snapshot.hooks,
        createdAt: SNAPSHOT_AT,
      },
      autoSync: dryRun
        ? { willPause: true }
        : { paused: true, reason: "undo", since: "2026-11-17T09:00:00.000Z" },
    };
    if (!dryRun) {
      changes = [...snapshot.changes, ...changes];
      for (const c of snapshot.changes) if (c.op === "add") locked.delete(c.skill);
      snapshot = null;
      pause = { reason: "undo", since: "2026-11-17T09:00:00.000Z" };
    }
    return ok(data);
  }

  function verify(): CtlReply {
    if (!lockfile)
      return ok({
        clean: true,
        matched: 0,
        total: 0,
        mismatched: [],
        lockfilePresent: false,
        warnings: [],
      });
    const mismatched = Array.from({ length: drift }, (_, n) => ({
      client: "claude-code",
      path: n === 0 ? "references/checklist.md" : "references/notes.md",
      reason: "differs",
      skill: n === 0 ? "api-review" : "log-reader",
    }));
    const data = {
      clean: drift === 0,
      matched: TOTAL_FILES - drift,
      total: TOTAL_FILES,
      mismatched,
      lockfilePresent: true,
      warnings: [],
    };
    return drift === 0 ? ok(data) : fail("unhealthy", "one or more checks failed", data);
  }

  const HOUR_MS = 3_600_000;
  const AUTO_SLOT = "30 6 * * *";

  /** The next 06:30 UTC after now, which is what the shipped cron fires on. */
  function nextSlot(): string {
    const at = new Date();
    at.setUTCHours(6, 30, 0, 0);
    if (at.getTime() <= Date.now()) at.setUTCDate(at.getUTCDate() + 1);
    return at.toISOString();
  }

  function schedule(): AutoSchedule {
    return {
      task: "skills-auto-sync",
      present: true,
      enabled: autoOn,
      cron: AUTO_SLOT,
      when: "daily 06:30 UTC",
      autoRun: true,
      nextRun: autoOn ? nextSlot() : null,
      lastRun: taskRun,
    };
  }

  function recordRun(run: AutoLastRun) {
    const at = Date.parse(run.at);
    lastRun = run;
    taskRun = {
      runId: "run-1794932392-bf0eb57a",
      status: run.error ? "failed" : "ok",
      trigger: "schedule",
      startedAt: new Date(at).toISOString(),
    };
  }

  function seedLastRun() {
    if (autoState === "off" || autoState === "paused") return;
    const at = new Date(Date.now() - 2.5 * HOUR_MS).toISOString();
    if (autoState === "failed")
      return recordRun({
        at,
        applied: 0,
        held: [],
        skipped: 0,
        error: "no skills repository found",
      });
    const held = autoState === "held" ? rowsHeld(visibleChanges()) : [];
    recordRun({ at, applied: autoState === "held" ? 6 : 3, held, skipped: 0 });
  }

  function autoData() {
    return {
      paused: pause !== null,
      reason: pause?.reason ?? null,
      since: pause?.since ?? null,
      lastRun,
      schedule: schedule(),
    };
  }

  function autoSwitch(on: boolean, dryRun: boolean): CtlReply {
    const changes = autoOn !== on;
    const word = on ? "on" : "off";
    if (!dryRun) autoOn = on;
    const undo = `toolportctl skills auto ${on ? "off" : "on"}`;
    return ok({
      dryRun,
      task: "skills-auto-sync",
      enabled: on,
      path: `${LIBRARY_HOME}/Library/Application Support/toolport/plus/tasks/skills-auto-sync.json`,
      plan: {
        summary: changes
          ? `Turn task skills-auto-sync ${word}`
          : `Task skills-auto-sync is already ${word}`,
        steps: [],
        warnings:
          on && changes ? ["the schedule runs the task on its own (autoRun)"] : [],
        undo,
        effects: {},
      },
      result: dryRun ? null : { applied: changes, changed: [], undo, backups: [] },
      schedule: schedule(),
    });
  }

  function auto(argv: string[]): CtlReply {
    const action = argv[2];
    if (action === "on" || action === "off")
      return autoSwitch(action === "on", argv.includes("--dry-run"));
    if (action === "pause" && !pause)
      pause = { reason: "manual", since: "2026-11-17T09:00:00.000Z" };
    if (action === "resume") pause = null;
    return ok(
      action === "status"
        ? autoData()
        : {
            paused: pause !== null,
            reason: pause?.reason ?? null,
            since: pause?.since ?? null,
            lastRun,
          },
    );
  }

  function status(): CtlReply {
    return ok({
      drift: false,
      entries: items.map((i) => ({
        name: i.name,
        type: i.type,
        clientsSynced: locked.has(i.name) ? ["claude-code"] : [],
        drifted: false,
        knownToLockfile: locked.has(i.name),
      })),
      lockedCount: locked.size,
      lockfilePresent: lockfile,
      lockfileSyncedAt: lockfile ? "2026-11-17T07:00:00.000Z" : null,
      outputs: items
        .filter((i) => locked.has(i.name))
        .flatMap((i) =>
          clients.map((client) => ({ name: i.name, client, present: true })),
        ),
      repo: LIBRARY_REPO,
      targetedClients: clients,
    });
  }

  function lint(): CtlReply {
    const rows = lintRows();
    const errors = rows.filter((r) => r.level === "error").length;
    const warnings = rows.filter((r) => r.level === "warning").length;
    const data = {
      errors,
      warnings,
      limits: { description: 1536, specDescription: 1024 },
      consequences: consequences(rows),
      messages: rows,
    };
    return errors > 0 ? fail("unhealthy", "one or more checks failed", data) : ok(data);
  }

  function audit(): CtlReply {
    return ok({
      clean: true,
      findings: [] as AuditFinding[],
      high: 0,
      medium: 0,
      low: 0,
      skillCount: items.length,
    });
  }

  function moveLink(argv: string[]): CtlReply {
    const [skill, link] = [argv[3], argv[4]];
    const dryRun = argv.includes("--dry-run");
    if (skill !== "spec-writer" || link !== "../shared/glossary.md" || moved)
      return fail(
        "refused",
        `${link ?? ""} is not a link of SKILL.md that leaves the skill folder`,
      );
    if (!dryRun) moved = true;
    return ok({
      dryRun,
      fix: "move-link",
      skill,
      link,
      from: `${LIBRARY_REPO}/shared/glossary.md`,
      to: "references/glossary.md",
      reused: false,
      size: 2048,
      count: 1,
      rewrites: [
        {
          line: 31,
          before: "Read the [glossary](../shared/glossary.md) before you start.",
          after: "Read the [glossary](references/glossary.md) before you start.",
        },
      ],
    });
  }

  function fix(argv: string[]): CtlReply {
    if (argv[2] === "move-link") return moveLink(argv);
    const dryRun = argv.includes("--dry-run");
    const has = options.chmod === true && !fixed;
    if (!dryRun && has) fixed = true;
    return ok({
      count: has ? 1 : 0,
      dryRun,
      fix: "chmod",
      fixed: has
        ? [
            {
              event: "Stop",
              file: "scripts/on-stop.sh",
              from: "0644",
              skill: "task-runner",
              to: "0755",
            },
          ]
        : [],
    });
  }
  let fixed = false;

  function files(argv: string[]): CtlReply {
    const name = operand(argv, 2);
    const found = items.find((i) => i.name === name);
    if (!found) return fail("not_found", `skill not found: ${name ?? ""}`);
    const list = [
      ...filesOf(found.name, visibleChanges()),
      ...[...kept]
        .filter((k) => k.startsWith(`${found.name}/`))
        .map((k) => ({
          client: "claude-code",
          op: "kept",
          path: k.slice(found.name.length + 1),
          reason: null,
          size: 1200,
          status: "kept",
        })),
    ];
    const tally: Record<string, number> = {};
    for (const file of list) tally[file.status] = (tally[file.status] ?? 0) + 1;
    return ok({
      skill: found.name,
      type: found.type,
      source: found.path,
      counts: tally,
      denyList: DENY,
      description: {
        length:
          found.name === "ticket-intake"
            ? 1602
            : found.name === "spec-writer"
              ? 912
              : found.description.length,
        description:
          found.name === "ticket-intake"
            ? 1200
            : found.name === "spec-writer"
              ? 912
              : found.description.length,
        whenToUse: found.name === "ticket-intake" ? 402 : 0,
        limit: 1536,
        specLimit: 1024,
      },
      files: list,
      installed: [
        {
          client: "claude-code",
          dir: folder(found.name),
          present: locked.has(found.name),
        },
      ],
      links: linksOf(found.name).filter(
        (link) => !(moved && link.target === "../shared/glossary.md"),
      ),
      lockfilePresent: lockfile,
      targetedClients: clients,
    });
  }

  function heldFiles(name: string): PlanChange[] {
    return visibleChanges().filter(
      (c) => c.skill === name && (c.op === "edited" || c.op === "differs"),
    );
  }

  function diff(argv: string[]): CtlReply {
    const name = operand(argv, 2);
    if (!name) return fail("unhealthy", "one or more checks failed", { clean: false });
    if (!items.some((i) => i.name === name))
      return fail("not_found", `skill not found: ${name}`);
    const only = flag(argv, "--file");
    const rows = visibleChanges().filter(
      (c) =>
        c.skill === name && (only ? inSkill(c) === only : heldFiles(name).includes(c)),
    );
    if (only && rows.length === 0)
      return fail("not_found", `${name} has no file ${only} to compare`);
    return ok({
      repo: LIBRARY_REPO,
      skill: name,
      type: "skill",
      lockfilePresent: lockfile,
      differing: rows.length,
      files: rows.map((c) => {
        const file = inSkill(c);
        const text = COMPARE_TEXT[`${name}/${file}`] ?? {
          library: "library text\n",
          installed: "installed text\n",
          diff: "--- library\n+++ installed\n@@ -1,1 +1,1 @@\n-library text\n+installed text\n",
        };
        return {
          path: file,
          client: c.client,
          status: c.op === "edited" ? "hand-edited" : c.op,
          installedPath: `${folder(name)}/${file}`,
          binary: false,
          truncated: false,
          identical: false,
          library: { present: true, size: text.library.length, text: text.library },
          installed: { present: true, size: text.installed.length, text: text.installed },
          diff: text.diff,
          added: 1,
          removed: text.diff.includes("\n-") ? 1 : 0,
        };
      }),
    });
  }

  function keep(argv: string[]): CtlReply {
    const name = operand(argv, 2);
    const asked = argv.slice(3).filter((a) => !a.startsWith("--"));
    if (!name || asked.length === 0)
      return fail("usage", "usage: skills keep <skill> <file>...");
    const held = heldFiles(name).map(inSkill);
    const missing = asked.find((file) => !held.includes(file));
    if (missing) return fail("refused", `${missing} is not held back for ${name}`);
    const dryRun = argv.includes("--dry-run");
    if (!dryRun) for (const file of asked) kept.add(`${name}/${file}`);
    return ok({
      skill: name,
      dryRun,
      count: asked.length,
      kept: asked.map((path) => ({ client: "claude-code", path, status: "differs" })),
    });
  }

  function ls(argv: string[]): CtlReply {
    const source = flag(argv, "--source");
    if (source && source !== "library")
      return ok({ repo: `${LIBRARY_HOME}/work/project-a`, skills: [], partial: false });
    return ok({ repo: LIBRARY_REPO, skills: items });
  }

  function remoteData(fetch: boolean) {
    return {
      ahead: remote.ahead,
      auth: { checked: fetch, method: "git-credential", ok: remote.hasRemote },
      behind: remote.behind,
      branch: "main",
      duplicateClones: [] as string[],
      fetch: { error: null, ok: fetch ? true : null, requested: fetch },
      lastFetch: "2026-11-17T08:54:00.000Z",
      remote: remote.hasRemote ? "github.com/acme-co/skills-library" : null,
      repo: LIBRARY_REPO,
      uncommitted: remote.uncommitted,
      upstream: remote.hasRemote && published ? "origin/main" : null,
    };
  }

  const pushChecks = () => ({
    audit: { findings: [] as AuditFinding[], high: 0, ran: true, skills: items.length },
    blocked: options.secret === true,
    builtinScan: {
      count: options.secret ? 1 : 0,
      findings: options.secret
        ? [
            {
              commit: "13e1696",
              file: "skills/deploy-notes/scripts/publish.sh",
              line: 14,
              rule: "github-token",
            },
          ]
        : [],
    },
    gitleaks: { available: false, count: 0, error: "not installed", ran: false },
  });

  function connect(argv: string[]): CtlReply {
    const url = operand(argv, 2) ?? "";
    const dryRun = argv.includes("--dry-run");
    if (url.includes("@") && url.includes("://") && url.split("//")[1].includes(":"))
      return fail("usage", "repository urls must not embed credentials");
    if (connected)
      return fail("refused", `a library is already connected at ${LIBRARY_REPO}`);
    const target = flag(argv, "--clone-to") ?? `${LIBRARY_HOME}/.config/mcpm/skills_repo`;
    if (dryRun)
      return ok({
        dryRun,
        repo: target,
        remote: url,
        plan: plan(
          `Clone ${url} into ${target}`,
          [
            { op: "exec", detail: "git clone (network)", path: target },
            { op: "note", detail: `point the library at ${target}` },
          ],
          `remove the folder ${target} and the local_path entry of skills_sync.json`,
        ),
      });
    connected = true;
    remote = { ...remote, hasRemote: true, behind: 0, ahead: 0, uncommitted: 0 };
    return ok({ dryRun, repo: target, remote: url, connected: true, empty: false });
  }

  function remoteAdd(argv: string[]): CtlReply {
    const url = operand(argv, 3) ?? "";
    const dryRun = argv.includes("--dry-run");
    if (remote.hasRemote)
      return fail(
        "refused",
        "the library already has a remote: github.com/acme-co/skills-library",
      );
    if (dryRun)
      return ok({
        dryRun,
        repo: LIBRARY_REPO,
        remote: url,
        plan: plan(
          `Add ${url} as origin`,
          [
            {
              op: "exec",
              detail: "git remote add origin (nothing is fetched or pushed)",
              path: LIBRARY_REPO,
            },
          ],
          `git -C '${LIBRARY_REPO}' remote remove origin`,
        ),
      });
    remote = { ...remote, hasRemote: true };
    published = false;
    return ok({ dryRun, repo: LIBRARY_REPO, remote: url });
  }

  function commit(argv: string[]): CtlReply {
    const message = flag(argv, "-m") ?? "";
    const dryRun = argv.includes("--dry-run");
    if (!message.trim()) return fail("usage", "the commit needs a message");
    const files = ["skills/api-review/SKILL.md", "skills/build-triage/SKILL.md"].slice(
      0,
      remote.uncommitted,
    );
    if (remote.uncommitted === 0)
      return ok({
        dryRun,
        repo: LIBRARY_REPO,
        committed: false,
        message: "working tree clean; nothing to commit",
        files: [],
      });
    const checks = pushChecks();
    if (dryRun)
      return ok({
        dryRun,
        repo: LIBRARY_REPO,
        committed: false,
        files,
        checks,
        plan: plan(
          `Commit ${files.length} changed file(s)`,
          [
            { op: "exec", detail: "git add -A", path: LIBRARY_REPO },
            { op: "exec", detail: `git commit -m "${message}"`, path: LIBRARY_REPO },
          ],
          `git -C '${LIBRARY_REPO}' reset --soft HEAD~1`,
        ),
      });
    if (checks.blocked)
      return fail(
        "refused",
        "secret scan blocked the commit: github-token in skills/deploy-notes/scripts/publish.sh:14 (nothing was committed)",
      );
    remote = { ...remote, uncommitted: 0, ahead: remote.ahead + 1 };
    return ok({
      dryRun,
      repo: LIBRARY_REPO,
      committed: true,
      commitSha: "f00d1e5",
      files,
      checks,
    });
  }

  function library(argv: string[]): CtlReply {
    if (argv[1] === "select") return selectWorld.run(argv);
    const dryRun = argv.includes("--dry-run");
    if (argv[1] === "status") return ok(remoteData(argv.includes("--fetch")));
    if (argv[1] === "connect") return connect(argv);
    if (argv[1] === "commit") return commit(argv);
    if (argv[1] === "remote" && argv[2] === "add") return remoteAdd(argv);
    if (argv[1] === "pull") {
      if (remote.uncommitted > 0)
        return fail(
          "conflict",
          `pull refused: ${remote.uncommitted} uncommitted changes`,
        );
      if (remote.behind === 0)
        return ok({ dryRun, repo: LIBRARY_REPO, pulled: false, commits: 0 });
      const commits = remote.behind;
      if (dryRun)
        return ok({
          dryRun,
          repo: LIBRARY_REPO,
          plan: plan(
            `Fast-forward ${commits} commit(s) from origin/main`,
            [
              {
                op: "exec",
                detail: "git fetch origin (refs only, network)",
                path: LIBRARY_REPO,
              },
              {
                op: "exec",
                detail: "git merge --ff-only origin/main",
                path: LIBRARY_REPO,
              },
              { op: "note", detail: "628cdc8 Add notes-two" },
              { op: "note", detail: "e3bf666 Add notes-one" },
            ],
            `git -C '${LIBRARY_REPO}' reset --hard 789e5c0`,
          ),
        });
      remote = { ...remote, behind: 0 };
      changes = [
        ...changes,
        change("onboarding-tour", path("onboarding-tour", "SKILL.md"), "update"),
        change("ui-polish", path("ui-polish", "references/format.md"), "add"),
      ];
      return ok({ dryRun, repo: LIBRARY_REPO, pulled: true, commits });
    }
    const checks = pushChecks();
    const commits = [
      ...(remote.uncommitted > 0
        ? [{ sha: "new", subject: "commit 2 changed files: api-review, build-triage" }]
        : []),
      ...(remote.ahead > 0
        ? [{ sha: "a1b2c3d", subject: "Tighten spec-writer description" }]
        : []),
    ];
    if (dryRun)
      return ok({
        branch: "main",
        checks,
        commits,
        dryRun,
        plan: {
          ...plan(
            `Push ${commits.length} commit(s) to origin/main`,
            [
              {
                op: "note",
                detail: `audit: ${items.length} skill(s), 0 finding(s), 0 high`,
              },
              { op: "note", detail: "gitleaks not installed; built-in scan" },
              { op: "exec", detail: "git push (never forced)", path: LIBRARY_REPO },
            ],
            `git -C '${LIBRARY_REPO}' revert --no-edit <remote head>..HEAD`,
          ),
          warnings: options.secret
            ? [
                "the push would be refused: github-token in skills/deploy-notes/scripts/publish.sh:14",
              ]
            : [],
        },
        pushed: false,
        repo: LIBRARY_REPO,
      });
    if (options.secret)
      return fail("conflict", "push refused: 1 possible secret", {
        checks,
        pushed: false,
        dryRun,
        repo: LIBRARY_REPO,
      });
    remote = { ...remote, ahead: 0, uncommitted: 0 };
    published = true;
    return ok({ checks, commitSha: "a1b2c3d", dryRun, pushed: true, repo: LIBRARY_REPO });
  }

  /** `skills tap link|unlink`: the marketplace of one tap declared in, or removed from, Claude
   * Code's settings. Only the tap `acme-skills` is a plugin marketplace. */
  function tapLink(argv: string[], link: boolean): CtlReply {
    const dryRun = argv.includes("--dry-run");
    const name = operand(argv, 3) ?? "";
    if (!taps.some((t) => t.name === name))
      return fail("not_found", `no tap named ${name}`);
    if (name !== MARKET_TAP)
      return fail(
        "skills",
        `tap '${name}' is not a plugin marketplace: it has no .claude-plugin/marketplace.json`,
      );
    const settings = `${LIBRARY_HOME}/.claude/settings.json`;
    const already = linkedTaps.has(name) === link;
    const changesOf = already
      ? []
      : [
          {
            action: link ? "declare" : "remove",
            key: "extraKnownMarketplaces.acme",
            path: settings,
            scope: "user",
          },
          {
            action: link ? "enable" : "remove",
            key: "enabledPlugins.core@acme",
            path: settings,
            scope: "user",
          },
        ];
    if (!dryRun) {
      if (link) linkedTaps.add(name);
      else linkedTaps.delete(name);
    }
    return ok({
      tap: name,
      dryRun,
      scope: "user",
      changes: changesOf,
      conflicts: [],
      plan: {
        summary: link
          ? `Link tap ${name} to Claude Code as marketplace acme`
          : `Unlink tap ${name} from Claude Code`,
        steps: already
          ? [
              {
                op: "note",
                detail: link
                  ? "already linked: acme is declared and its plugins are switched on"
                  : `tap ${name} is not linked to Claude Code; nothing to remove`,
              },
            ]
          : [
              {
                op: "merge",
                detail: `${link ? "declare" : "remove"} acme`,
                path: settings,
              },
              {
                op: "merge",
                detail: `${link ? "enable" : "remove"} core@acme`,
                path: settings,
              },
            ],
        warnings: [
          "Claude Code reads plugin settings when a session starts: sessions that are already running keep the old state until they restart",
        ],
        undo: `toolportctl skills tap ${link ? "unlink" : "link"} ${name}`,
        effects: {},
      },
      result: dryRun
        ? null
        : {
            applied: true,
            changed: changesOf.map((c) => `${c.key} in ${settings}`),
            undo: `toolportctl skills tap ${link ? "unlink" : "link"} ${name}`,
          },
    });
  }

  function tapCmd(argv: string[]): CtlReply {
    const dryRun = argv.includes("--dry-run");
    switch (argv[2]) {
      case "ls":
        return ok({
          tapsRoot: `${LIBRARY_HOME}/Library/Application Support/Toolport/taps`,
          taps: taps.map((t) =>
            t.name === MARKET_TAP
              ? {
                  ...t,
                  marketplace: {
                    name: "acme",
                    declared: linkedTaps.has(t.name) ? "here" : "no",
                    plugins: [
                      {
                        id: "core@acme",
                        name: "core",
                        linked: linkedTaps.has(t.name),
                        skills: 1,
                        agents: 0,
                        hooks: false,
                        version: "1.4.0",
                      },
                    ],
                    warnings: [],
                  },
                }
              : t,
          ),
        });
      case "link":
      case "unlink":
        return tapLink(argv, argv[2] === "link");
      case "add": {
        const repo = operand(argv, 3) ?? "";
        const name = repo.split("/").pop() ?? repo;
        if (!dryRun) taps = [...taps, tap(name, repo)];
        return ok({ name, repo, dryRun, cloned: !dryRun });
      }
      case "remove": {
        const name = operand(argv, 3) ?? "";
        if (!taps.some((t) => t.name === name))
          return fail("not_found", `no tap named ${name}`);
        if (linkedTaps.has(name))
          return fail(
            "skills",
            `Tap '${name}' is linked to Claude Code; run \`toolportctl skills tap unlink ${name}\` first`,
          );
        if (!dryRun) taps = taps.filter((t) => t.name !== name);
        return ok({ name, dryRun, removed: !dryRun });
      }
      default: {
        const only = operand(argv, 3);
        if (!dryRun && only === "community-tools") tapFails = false;
        const rows = taps
          .filter((t) => !only || t.name === only)
          .map((t) => {
            const bad = t.name === "community-tools" && tapFails;
            return {
              name: t.name,
              ok: !bad,
              head: bad ? null : "9f3e2ab",
              error: bad
                ? "git asked for a password. Run `gh auth login`, then retry."
                : null,
            };
          });
        return ok({ dryRun, failed: rows.filter((r) => !r.ok).length, results: rows });
      }
    }
  }

  const HITS = [
    {
      tap: "community-tools",
      repo: "community/toolbox",
      name: "quick-deploy",
      description: "Roll a service out in steps.",
      type: "skill",
    },
    {
      tap: "acme-skills",
      repo: "acme-co/agent-skills",
      name: "dep-check",
      description: "Compare dependency versions against a policy.",
      type: "skill",
    },
    {
      tap: "acme-skills",
      repo: "acme-co/agent-skills",
      name: "api-review",
      description: "Already in your library.",
      type: "skill",
    },
  ];

  function search(argv: string[]): CtlReply {
    const query = (operand(argv, 2) ?? "").toLowerCase();
    return ok({
      query,
      tapCount: taps.length,
      results: HITS.filter((h) => `${h.name} ${h.description}`.includes(query)),
      discoveryWarnings: [],
    });
  }

  function install(argv: string[]): CtlReply {
    const spec = operand(argv, 2) ?? "";
    const name = spec.split("/").pop() ?? "";
    const dryRun = argv.includes("--dry-run");
    const risky = name === "quick-deploy";
    if (!HITS.some((h) => h.name === name))
      return fail("not_found", `No skills found for '${spec}'`);
    if (!dryRun && !items.some((i) => i.name === name)) {
      items = [
        ...items,
        item(name, "skill", HITS.find((h) => h.name === name)!.description),
      ];
      changes = [...changes, change(name, path(name, "SKILL.md"), "add")];
    }
    return ok({
      spec,
      tap: HITS.find((h) => h.name === name)!.tap,
      dryRun,
      blocked: false,
      target: LIBRARY_REPO,
      audit: {
        ran: true,
        high: risky ? 1 : 0,
        medium: risky ? 1 : 0,
        low: 0,
        findings: risky
          ? [
              {
                severity: "high",
                skill: name,
                message: "Tells the model not to mention a step to the user.",
                line: 22,
              },
              {
                severity: "medium",
                skill: name,
                message: "Runs a command piped from the network.",
                line: 58,
              },
            ]
          : [],
      },
      skills: [
        {
          name,
          type: "skill",
          path: `${LIBRARY_REPO}/skills/${name}`,
          files: 5,
          status: "installed",
        },
      ],
      installedCount: 1,
      skippedCount: 0,
      discoveryWarnings: [],
    });
  }

  function sources(argv: string[]): CtlReply {
    if (argv[1] === "ls")
      return ok({
        generatedAt: "2026-11-17T08:52:00.000Z",
        partial: false,
        skipped: [],
        sources: SOURCES,
      });
    const dryRun = argv.includes("--dry-run");
    if (argv[2] === "ls") return ok({ roots });
    const target = operand(argv, 3) ?? "";
    if (argv[2] === "add") {
      if (!target.startsWith("/")) return fail("usage", "give an absolute folder");
      if (!dryRun)
        roots = [...roots, { path: target, exists: true, origin: "config", repo: false }];
      return ok({ dryRun, path: target, added: !dryRun });
    }
    if (!dryRun) roots = roots.filter((r) => r.path !== target);
    return ok({ dryRun, path: target, removed: !dryRun });
  }

  function clean(argv: string[]): CtlReply {
    const dryRun = argv.includes("--dry-run");
    return ok({
      dryRun,
      managed: [...locked],
      removed: [...locked].slice(0, 5).map((n) => `${folder(n)}/SKILL.md`),
      skipped: [],
      lockfileRemoved: true,
    });
  }

  function bundle(argv: string[]): CtlReply {
    const dryRun = argv.includes("--dry-run");
    return ok({
      dryRun,
      output: `${LIBRARY_REPO}/skills-library-bundle.zip`,
      fileCount: TOTAL_FILES,
      sourceBytes: 412_000,
      bundleBytes: dryRun ? null : 188_000,
      skills: items.slice(0, 3).map((i) => ({ name: i.name, type: i.type, files: 5 })),
    });
  }

  function unbundle(argv: string[]): CtlReply {
    const zip = operand(argv, 2);
    if (!zip) return fail("usage", "usage: skills unbundle <zip>");
    return ok({
      dryRun: argv.includes("--dry-run"),
      bundle: zip,
      names: ["imported-skill"],
      files: ["skills/imported-skill/SKILL.md"],
      overwritten: [],
      skipped: [],
      target: LIBRARY_REPO,
    });
  }

  function add(argv: string[]): CtlReply {
    const name = operand(argv, 2);
    if (!name) return fail("usage", "usage: skills add <name>");
    if (items.some((i) => i.name === name))
      return fail("conflict", `${name} already exists`);
    const dryRun = argv.includes("--dry-run");
    const type = flag(argv, "--type") === "rule" ? "rule" : "skill";
    if (!dryRun) {
      items = [...items, item(name, type, "")];
      changes = [...changes, change(name, path(name, "SKILL.md"), "add")];
    }
    return ok({
      dryRun,
      name,
      type,
      path: `${LIBRARY_REPO}/${type === "rule" ? "rules" : "skills"}/${name}/SKILL.md`,
      files: [`${LIBRARY_REPO}/skills/${name}/SKILL.md`],
    });
  }

  function init(argv: string[]): CtlReply {
    return ok({
      alreadyExists: false,
      created: [
        "mcpm-skills.yaml",
        "skills/",
        "rules/",
        "agents/",
        "styles/",
        "profiles/",
      ],
      dryRun: argv.includes("--dry-run"),
      name: flag(argv, "--name") ?? "skills-library",
      repo: flag(argv, "--path") ?? LIBRARY_REPO,
    });
  }

  function uninstall(argv: string[]): CtlReply {
    const name = operand(argv, 2) ?? "";
    if (!items.some((i) => i.name === name))
      return fail("not_found", `skill not found: ${name}`);
    const dryRun = argv.includes("--dry-run");
    if (!dryRun) {
      items = items.filter((i) => i.name !== name);
      changes = changes.filter((c) => c.skill !== name);
      locked.delete(name);
    }
    return ok({
      dryRun,
      name,
      outputs: locked.has(name) || dryRun ? [`${folder(name)}/SKILL.md`] : [],
      sourcePath: `${LIBRARY_REPO}/skills/${name}`,
    });
  }

  function resolve(argv: string[]): CtlReply {
    return ok({
      dryRun: argv.includes("--dry-run"),
      collisions: [],
      replaced: argv.includes("--migrate") ? 1 : 0,
      kept: argv.includes("--no-migrate") ? 1 : 0,
      repo: LIBRARY_REPO,
    });
  }

  /** What changed since the last sync: an item the lock does not know is new, one edited
   * through `edit_body` is modified. */
  function contentDiff(kind: "agents" | "styles", names: string[]) {
    const fresh = names.filter((n) => !["code-scout", "plain"].includes(n));
    const modified = names.filter(
      (n) => edited.has(`${kind}:${n}`) && !fresh.includes(n),
    );
    return {
      repo: LIBRARY_REPO,
      noLockfile: false,
      clean: fresh.length + modified.length === 0,
      new: fresh,
      modified,
      removed: [] as string[],
      unchanged: names.length - fresh.length - modified.length,
    };
  }

  /** The self-management tools the Library runs through `mcp call`: read and edit the body of a
   * skill, an agent or a style. */
  function tool(name: string, args: Record<string, unknown>): CtlReply | null {
    const [kind, action] = name.split(/_(.*)/s) as [
      "skills" | "agents" | "styles",
      string,
    ];
    if (
      !["skills", "agents", "styles"].includes(kind) ||
      !["get", "edit_body"].includes(action)
    )
      return null;
    const item =
      kind === "skills"
        ? items.find((i) => i.name === args.name)
        : kind === "agents"
          ? agents.find((a) => a.name === args.name)
          : styles.find((s) => s.name === args.name);
    if (!item)
      return {
        ok: false,
        error: {
          code: "not_found",
          message: `${kind.slice(0, -1)} not found: ${String(args.name)}`,
        },
      };
    const key = `${kind}:${item.name}`;
    if (action === "get")
      return ok({
        tool: name,
        tier: 1,
        isError: false,
        result: {
          name: item.name,
          description: item.description,
          path: item.path,
          body: bodies.get(key) ?? `# ${item.name}\n\nSteps for ${item.name}.\n`,
        },
      });
    if (args.confirm !== true)
      return {
        ok: false,
        error: { code: "refused", message: `${name} needs confirm=true` },
      };
    bodies.set(key, `${String(args.new_body ?? "").trimEnd()}\n`);
    edited.add(key);
    if (
      kind === "skills" &&
      !changes.some(
        (c) => c.skill === item.name && !c.kind && c.path.endsWith("SKILL.md"),
      )
    )
      changes = [...changes, change(item.name, path(item.name, "SKILL.md"), "update")];
    return ok({
      tool: name,
      tier: 3,
      isError: false,
      result: { sourcePath: item.path, newHash: "sha256:0123456789abcdef" },
    });
  }

  function agentCmd(argv: string[]): CtlReply {
    const dryRun = argv.includes("--dry-run");
    switch (argv[1]) {
      case "ls":
        return ok({ agents, repo: LIBRARY_REPO, discoveryWarnings: [] });
      case "status":
        return ok({
          drift: false,
          lockedCount: agentSynced ? agents.length : 0,
          lockfilePresent: agentSynced,
          outputs: agentSynced
            ? agents.map((a) => ({ name: a.name, client: "claude-code", present: true }))
            : [],
          repo: LIBRARY_REPO,
        });
      case "lint":
        return ok({
          agentCount: agents.length,
          errors: options.longAgent ? 1 : 0,
          warnings: 0,
          infos: 0,
          messages: options.longAgent
            ? [
                {
                  level: "error",
                  name: "long-agent",
                  message:
                    "agents/long-agent/AGENT.md cannot be read and is not synced: description must be 1-1536 characters",
                },
              ]
            : [],
          discoveryWarnings: [],
        });
      case "audit":
        return ok({
          clean: true,
          findings: [],
          high: 0,
          medium: 0,
          low: 0,
          skillCount: agents.length,
        });
      case "sync":
        if (!dryRun) agentSynced = true;
        return ok({
          dryRun,
          agentCount: agents.length,
          agents: agents.map((a) => ({
            name: a.name,
            clientsSynced: ["claude-code"],
            outputFiles: [
              { client: "claude-code", files: [`.claude/agents/${a.name}.md`] },
            ],
            warnings: [],
          })),
        });
      case "add": {
        const name = operand(argv, 2) ?? "";
        if (agents.some((a) => a.name === name))
          return fail("conflict", `agent ${name} already exists`);
        const path = `${LIBRARY_REPO}/agents/${name}/AGENT.md`;
        if (!dryRun) {
          agents = [
            ...agents,
            {
              name,
              description: "TODO: Describe the agent.",
              model: "inherit",
              path,
              tools: [],
            },
          ];
          bodies.set(`agents:${name}`, "TODO: Add the agent's system prompt here.\n");
          agentSynced = false;
        }
        return ok({ dryRun, name, path, repo: LIBRARY_REPO });
      }
      case "diff":
        return ok(
          contentDiff(
            "agents",
            agents.map((a) => a.name),
          ),
        );
      case "clean": {
        const removed = agentSynced
          ? agents.map((a) => `${OUT}/.claude/agents/${a.name}.md`)
          : [];
        if (!dryRun) agentSynced = false;
        return ok({
          dryRun,
          lockfilePresent: true,
          managed: agents.map((a) => a.name),
          removed,
          skipped: [],
        });
      }
      case "uninstall": {
        const name = operand(argv, 2) ?? "";
        if (!dryRun) agents = agents.filter((a) => a.name !== name);
        return ok({
          dryRun,
          name,
          outputs: [`${OUT}/.claude/agents/${name}.md`],
          sourcePath: `${LIBRARY_REPO}/agents/${name}`,
        });
      }
      default:
        return fail("usage", `unknown agents command: ${argv[1] ?? ""}`);
    }
  }

  const activePairs = () =>
    [...activeStyles].map(([client, style]) => ({ client, style }));

  function styleApply(argv: string[]): CtlReply {
    const dryRun = argv.includes("--dry-run");
    const name = operand(argv, 2) ?? "";
    if (!styles.some((s) => s.name === name))
      return fail(
        "styles",
        `Style '${name}' not found. Available: ${styles.map((s) => s.name).join(", ") || "none"}`,
      );
    const asked = argv.flatMap((a, i) => (a === "--client" ? [argv[i + 1]] : []));
    const targets = STYLE_CLIENTS.filter(
      (c) => asked.length === 0 || asked.includes(c.key),
    );
    if (targets.length === 0) return fail("usage", `unknown client: ${asked.join(", ")}`);
    const replaced = targets
      .filter((c) => activeStyles.has(c.key) && activeStyles.get(c.key) !== name)
      .map((c) => ({ client: c.key, style: activeStyles.get(c.key) ?? "" }));
    if (!dryRun) for (const c of targets) activeStyles.set(c.key, name);
    const after = new Map(activeStyles);
    if (dryRun) for (const c of targets) after.set(c.key, name);
    return ok({
      name,
      dryRun,
      scope: "global",
      applied: targets.map((c) => ({ client: c.key, path: `${OUT}/${c.path}` })),
      appliedCount: [...after.values()].filter((style) => style === name).length,
      replaced,
      active: [...after].map(([client, style]) => ({ client, style })),
      nativeClients: [],
      repo: LIBRARY_REPO,
      discoveryWarnings: [],
    });
  }

  function styleCmd(argv: string[]): CtlReply {
    const dryRun = argv.includes("--dry-run");
    switch (argv[1]) {
      case "ls":
        return ok({
          styles,
          active: activePairs(),
          repo: LIBRARY_REPO,
          lockfilePresent: activeStyles.size > 0,
        });
      case "status":
        if (options.styleStatusFails)
          return fail("skills", "the styles lock cannot be read: Permission denied");
        return ok({
          lockfilePresent: styles.some((s) => s.synced) || activeStyles.size > 0,
          native: [
            {
              client: "claude-code",
              name: "Claude Code",
              styles: styles.filter((s) => s.synced).map((s) => s.name),
            },
          ],
          applyRemove: STYLE_CLIENTS.map((c) => ({
            client: c.key,
            name: c.name,
            active: activeStyles.get(c.key) ?? null,
          })),
          repo: LIBRARY_REPO,
        });
      case "apply":
        return styleApply(argv);
      case "lint":
        return ok({ errors: 0, warnings: 0, infos: 0, messages: [] });
      case "sync":
        return ok({
          dryRun,
          styleCount: styles.length,
          outputs: styles.map((s) => `${OUT}/.claude/output-styles/${s.name}.md`),
          styles: styles.map((s) => ({
            name: s.name,
            clientsSynced: ["claude-code"],
            warnings: [],
          })),
        });
      case "remove": {
        const asked = argv.flatMap((a, i) => (a === "--client" ? [argv[i + 1]] : []));
        const targets = [...activeStyles].filter(
          ([client]) => asked.length === 0 || asked.includes(client),
        );
        if (!dryRun) for (const [client] of targets) activeStyles.delete(client);
        return ok({
          dryRun,
          hadActive: activeStyles.size > 0 || targets.length > 0,
          removed: targets.map(([client, style]) => ({
            client,
            style,
            path: `${OUT}/${STYLE_CLIENTS.find((c) => c.key === client)?.path ?? ""}`,
          })),
          active: activePairs(),
          clientKeys: asked,
        });
      }
      case "add": {
        const name = operand(argv, 2) ?? "";
        if (styles.some((s) => s.name === name))
          return fail("conflict", `style ${name} already exists`);
        const path = `${LIBRARY_REPO}/styles/${name}/STYLE.md`;
        if (!dryRun) {
          styles = [
            ...styles,
            {
              name,
              description: "TODO: Describe the style.",
              path,
              synced: false,
              clientsSynced: [],
            },
          ];
          bodies.set(`styles:${name}`, "TODO: Describe how replies should read.\n");
        }
        return ok({ dryRun, name, path, repo: LIBRARY_REPO });
      }
      case "diff":
        return ok(
          contentDiff(
            "styles",
            styles.map((s) => s.name),
          ),
        );
      case "clean":
        return ok({
          dryRun,
          lockfilePresent: true,
          managed: styles.map((s) => s.name),
          removed: styles
            .filter((s) => s.synced)
            .map((s) => `${OUT}/.claude/output-styles/${s.name}.md`),
          skipped: [],
        });
      default:
        return fail("usage", `unknown styles command: ${argv[1] ?? ""}`);
    }
  }

  seedLastRun();

  return {
    scenario,
    tool,
    inventory: inventoryOf,
    run(argv: string[]): CtlReply {
      const [first, second] = argv;
      switch (first) {
        case "library":
          return library(argv);
        case "sources":
          return sources(argv);
        case "agents":
          return agentCmd(argv);
        case "styles":
          return styleCmd(argv);
        default:
      }
      switch (second) {
        case "sync":
          return sync(argv);
        case "undo":
          return undo(argv);
        case "verify":
          return verify();
        case "auto":
          return auto(argv);
        case "status":
          return status();
        case "lint":
          return lint();
        case "audit":
          return audit();
        case "diff":
          return diff(argv);
        case "keep":
          return keep(argv);
        case "fix":
          return fix(argv);
        case "files":
          return files(argv);
        case "ls":
          return ls(argv);
        case "tap":
          return tapCmd(argv);
        case "search":
          return search(argv);
        case "install":
          return install(argv);
        case "clean":
          return clean(argv);
        case "bundle":
          return bundle(argv);
        case "unbundle":
          return unbundle(argv);
        case "add":
          return add(argv);
        case "init":
          return init(argv);
        case "uninstall":
          return uninstall(argv);
        case "resolve":
          return resolve(argv);
        default:
          throw new Error(`Unimplemented fixture command: plus_ctl ${argv.join(" ")}`);
      }
    },
  };
}

export type LibraryWorld = ReturnType<typeof createLibraryWorld>;
