/** What the `where_am_i` and `flow_diagram` self-management tools say, in words for the
 * Self-management tab. Both answer through `toolportctl mcp call`. */

export const SECRETS_BACKEND: Record<string, string> = {
  "encrypted-file": "Encrypted file, opened with the key from the environment",
  "os-keychain": "The operating system keychain",
};

export const secretsBackendLabel = (backend: string) =>
  SECRETS_BACKEND[backend] ?? (backend || "Unknown");

const LOGIN_LABEL: Array<[string, string]> = [
  ["ok", "signed in"],
  ["needs_reauth", "need a new sign-in"],
  ["expiring", "about to expire"],
  ["revoked", "revoked"],
  ["misconfigured", "misconfigured"],
  ["unreachable", "unreachable"],
  ["unknown", "not checked"],
];

export interface LoginCount {
  key: string;
  label: string;
  count: number;
  problem: boolean;
}

/** The non-zero login states, in the order a person reads them: what works first. */
export function loginCounts(counts: Record<string, number>): LoginCount[] {
  return LOGIN_LABEL.flatMap(([key, label]) => {
    const count = counts[key] ?? 0;
    return count > 0 ? [{ key, label, count, problem: key !== "ok" }] : [];
  });
}

export type FlowArrow = "to" | "both" | "from";

export interface FlowNode {
  label: string;
  /** What follows the name in brackets, e.g. `registry (servers, profiles)`. */
  detail: string | null;
}

export type FlowBlock =
  | { kind: "heading"; level: number; text: string }
  | { kind: "chain"; nodes: FlowNode[]; arrows: FlowArrow[] }
  | { kind: "text"; text: string }
  | { kind: "code"; text: string };

const ARROW = /\s+(<->|<-+>|-+>|<-+)\s+/;

function arrowOf(token: string): FlowArrow {
  if (token.startsWith("<") && token.endsWith(">")) return "both";
  return token.startsWith("<") ? "from" : "to";
}

function nodeOf(text: string): FlowNode {
  const bracket = /^(.*?)\s*\(([^()]*)\)$/.exec(text.trim());
  if (bracket && bracket[1]) return { label: bracket[1], detail: bracket[2] };
  return { label: text.trim(), detail: null };
}

/** The markdown of `flow_diagram` as blocks the screen can draw: headings, chains of boxes
 * joined by arrows (`a -> b <-> c`), plain lines and fenced text kept as it is. Anything else
 * is shown as the line it is, so a new kind of line never disappears. */
export function parseFlow(markdown: string): FlowBlock[] {
  const blocks: FlowBlock[] = [];
  let fence: string[] | null = null;
  for (const raw of markdown.split(/\r?\n/)) {
    if (/^\s*```/.test(raw)) {
      if (fence) blocks.push({ kind: "code", text: fence.join("\n") });
      fence = fence ? null : [];
      continue;
    }
    if (fence) {
      fence.push(raw);
      continue;
    }
    const line = raw.trim();
    if (line === "") continue;
    const heading = /^(#{1,6})\s+(.+)$/.exec(line);
    if (heading) {
      blocks.push({ kind: "heading", level: heading[1].length, text: heading[2] });
      continue;
    }
    const text = line.replace(/^[-*]\s+/, "");
    const parts = text.split(ARROW);
    if (parts.length < 3) {
      blocks.push({ kind: "text", text });
      continue;
    }
    blocks.push({
      kind: "chain",
      nodes: parts.filter((_, at) => at % 2 === 0).map(nodeOf),
      arrows: parts.filter((_, at) => at % 2 === 1).map(arrowOf),
    });
  }
  if (fence) blocks.push({ kind: "code", text: fence.join("\n") });
  return blocks;
}
