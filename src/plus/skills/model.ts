import type { CommandRow, Origin, SkillsLsData } from "../bridge/data";
import type { Tier } from "../allcommands/model";

export type SkillRow = SkillsLsData["skills"][number];
export type OriginKind = Origin["kind"];

export interface LintMessage {
  level: string;
  name: string;
  message: string;
}
export interface AuditFinding {
  severity: string;
  skill: string;
  line: number | null;
  message: string;
}
export interface StatusOutput {
  client: string;
  name: string;
  present: boolean;
}
export interface Rejected {
  name: string;
  client: string;
  code: string;
  path: string;
  reason: string;
}
export interface Collision {
  skill: string;
  client: string;
  action: string;
  collisionPath: string;
  backupPath: string | null;
}

const CLIENT_NAMES: Record<string, string> = {
  "claude-code": "Claude Code",
  "codex-cli": "Codex CLI",
  cursor: "Cursor",
  "gemini-cli": "Gemini CLI",
  "agents-md": "AGENTS.md",
  aider: "Aider",
  "amazon-q": "Amazon Q",
  cline: "Cline",
  continue: "Continue",
  "goose-cli": "Goose",
  jetbrains: "JetBrains",
  "roo-code": "Roo Code",
  trae: "Trae",
  windsurf: "Windsurf",
  zed: "Zed",
};

/** The clients `skills status` lists when no lock narrows them (the golden of the CLI). */
export const SKILL_CLIENTS = Object.keys(CLIENT_NAMES);

export const clientName = (key: string): string => CLIENT_NAMES[key] ?? key;

const CLIENT_ORDER = ["claude-code", "codex-cli", "cursor", "gemini-cli"];

export function byClient(a: string, b: string): number {
  const rank = (key: string) => {
    const at = CLIENT_ORDER.indexOf(key);
    return at < 0 ? CLIENT_ORDER.length : at;
  };
  return rank(a) - rank(b) || a.localeCompare(b);
}

export const plural = (n: number, one: string, many = `${one}s`) =>
  `${n} ${n === 1 ? one : many}`;

export function shortPath(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts.length > 4 ? `…/${parts.slice(-4).join("/")}` : path;
}

type BadgeTone = "success" | "info" | "warning" | "secondary";

const KIND_TONE: Partial<Record<OriginKind, BadgeTone>> = {
  library: "success",
  org: "info",
  plugin: "warning",
  loose: "warning",
};

export const kindTone = (kind: OriginKind): BadgeTone => KIND_TONE[kind] ?? "secondary";

export interface Policy {
  tier: Tier;
  previewFlag: string | null;
  terminal: boolean;
}

/** What the registry says about a command; one it lacks, or marks as planned, has no policy
 * and the screen does not run it. */
export function policyOf(rows: CommandRow[] | null, id: string): Policy | null {
  const row = rows?.find((c) => c.kind === "command" && c.id === id);
  if (!row || row.planned || !row.tier) return null;
  return {
    tier: row.tier,
    previewFlag: row.preview?.mode === "flag" ? row.preview.flag : null,
    terminal: row.surface === "terminal" || row.needs.includes("terminal-only"),
  };
}

const NAME = /^[a-z0-9][a-z0-9_-]*$/;

/** Why a new skill cannot be named so, or null. The CLI has the last word. */
export function nameProblem(name: string): string | null {
  if (name === "") return "Give it a name";
  if (!NAME.test(name))
    return "Use lowercase letters, digits, dashes and underscores, starting with a letter or digit";
  return null;
}

export const clientArgs = (clients: string[]): string[] =>
  clients.flatMap((client) => ["--client", client]);

export const rowKey = (row: SkillRow): string =>
  `${row.origin.kind}:${row.origin.name}:${row.type}:${row.name}`;

export function isLibrary(row: SkillRow): boolean {
  return row.origin.kind === "library";
}

export const EDITOR_REASON =
  "Editing the body and the frontmatter needs `mcp call skills_get`, `skills_edit_body` and `skills_edit_frontmatter` (MIG-GUI-14). Open the file above in your editor for now.";
