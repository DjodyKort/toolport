import type { CommandRow } from "../bridge/data";
import type { Tier } from "../allcommands/model";

export interface LintMessage {
  level: string;
  name: string;
  message: string;
}
export interface AuditFinding {
  severity: string;
  agent: string;
  message: string;
  line: number | null;
}
export interface StatusOutput {
  name: string;
  client: string;
  present: boolean;
}
export interface NativeRow {
  client: string;
  name: string;
  styles: string[];
}
export interface ActiveRow {
  client: string;
  name: string;
  active: string | null;
}
export interface ClientStyle {
  client: string;
  style: string;
}

const CLIENT_NAMES: Record<string, string> = {
  "claude-code": "Claude Code",
  "codex-cli": "Codex CLI",
  cursor: "Cursor",
  "gemini-cli": "Gemini CLI",
  "vscode-copilot": "VS Code Copilot",
  "roomodes-style": "Roo Code modes",
  "amazon-q": "Amazon Q",
  jetbrains: "JetBrains",
  windsurf: "Windsurf",
  cline: "Cline",
  continue: "Continue",
  aider: "Aider",
  goose: "Goose",
  trae: "Trae",
  zed: "Zed",
};

export function clientName(key: string): string {
  return CLIENT_NAMES[key] ?? key;
}

const CLIENT_ORDER = ["claude-code", "codex-cli", "cursor", "gemini-cli"];

export function byClient(a: string, b: string): number {
  const rank = (key: string) => {
    const at = CLIENT_ORDER.indexOf(key);
    return at < 0 ? CLIENT_ORDER.length : at;
  };
  return rank(a) - rank(b) || a.localeCompare(b);
}

export interface Dropped {
  client: string | null;
  field: string | null;
  text: string;
}

const DROPPED = /^([\w-]+):\s*'([^']+)'/;

/** A transpiler warning such as `cursor: 'tools' field not supported, dropped`, split into the
 * client and the field. A line of any other shape keeps its text and no client. */
export function droppedOf(warning: string): Dropped {
  const found = DROPPED.exec(warning);
  return { client: found?.[1] ?? null, field: found?.[2] ?? null, text: warning };
}

/** Whether a warning belongs to a client column (`vscode` is the prefix of `vscode-copilot`). */
export function warnsClient(dropped: Dropped, client: string): boolean {
  if (!dropped.client) return false;
  return client === dropped.client || client.startsWith(`${dropped.client}-`);
}

export interface Policy {
  tier: Tier;
  previewFlag: string | null;
  terminal: boolean;
}

/** What the registry says about a command; a command it does not have, or marks as planned,
 * has no policy and the screen does not run it. */
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

/** Why a new agent or style cannot be named so, or null. The CLI has the last word. */
export function nameProblem(name: string): string | null {
  if (name === "") return "Give it a name";
  if (!NAME.test(name))
    return "Use lowercase letters, digits, dashes and underscores, starting with a letter or digit";
  return null;
}

export const clientFlag = (key: string | null): string[] =>
  key ? [`--client=${key}`] : [];

export const plural = (n: number, one: string, many = `${one}s`) =>
  `${n} ${n === 1 ? one : many}`;

export function shortPath(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts.length > 3 ? `…/${parts.slice(-3).join("/")}` : path;
}
