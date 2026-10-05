import { CtlReplyFailure } from "../fixtures/ctlReply";
import type { BodyKind } from "./bodyEdit";

type Row = (argv: string[], stdin?: string) => unknown;

interface File {
  body: string;
  description: string;
  deleted?: boolean;
}

const FILE = { agents: "AGENT.md", styles: "STYLE.md", skills: "SKILL.md" } as const;
const ROOT = "/fixture/skills-repo";

export const mcpKey = (tool: string) => `mcp call ${tool} --args-stdin`;

const answer = (tool: string, tier: number, result: unknown) => ({
  isError: false,
  result,
  tier,
  tool,
});

function hash(text: string): string {
  let h = 0;
  for (const ch of text) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return `sha256:${h.toString(16).padStart(8, "0")}${"0".repeat(8)}`;
}

/** The self-management tools of the Library screens as the fixture bridges answer them
 * (`mcp call <tool> --args-stdin`, the arguments on stdin). The files are held in memory, so
 * a write changes what the next `*_get` returns; the answers have the shape of the real
 * `mcp-call.*` goldens. A name nobody wrote yet reads as a synthetic file. */
export function createMcpWorld(
  seed: Partial<Record<BodyKind, Record<string, Partial<File>>>> = {},
): Array<[string, Row]> {
  const files = new Map<string, File>();
  const key = (kind: BodyKind, name: string) => `${kind}/${name}`;
  const get = (kind: BodyKind, name: string): File => {
    const id = key(kind, name);
    if (!files.has(id)) {
      files.set(id, {
        body: `Body of ${name}\n`,
        description: `A synthetic ${kind.slice(0, -1)} ${name}`,
        ...seed[kind]?.[name],
      });
    }
    return files.get(id) as File;
  };
  const path = (kind: BodyKind, name: string) => `${ROOT}/${kind}/${name}/${FILE[kind]}`;
  const args = (stdin?: string) => JSON.parse(stdin ?? "{}") as Record<string, unknown>;
  const need = (tool: string, input: Record<string, unknown>) => {
    if (input.confirm !== true)
      return new CtlReplyFailure(
        "refused",
        `Refused: ${tool} (tier 3). Pass confirm=true to proceed.`,
      );
    return null;
  };
  const found = (kind: BodyKind, name: string) => {
    const file = get(kind, name);
    return file.deleted
      ? new CtlReplyFailure("not_found", `${kind.slice(0, -1)} not found: ${name}`)
      : file;
  };

  const rows: Array<[string, Row]> = [];
  for (const kind of ["agents", "styles", "skills"] as const) {
    rows.push([
      mcpKey(`${kind}_get`),
      (_argv, stdin) => {
        const name = String(args(stdin).name ?? "");
        const file = found(kind, name);
        if (file instanceof CtlReplyFailure) return file;
        const extra =
          kind === "agents"
            ? { model: "inherit", tools: [] }
            : kind === "styles"
              ? { keepCodingInstructions: true }
              : { activation: "auto", type: "skill" };
        return answer(`${kind}_get`, 1, {
          ...extra,
          body: file.body,
          description: file.description,
          name,
          path: path(kind, name),
        });
      },
    ]);
    rows.push([
      mcpKey(`${kind}_edit_body`),
      (_argv, stdin) => {
        const input = args(stdin);
        const refused = need(`${kind}_edit_body`, input);
        if (refused) return refused;
        const name = String(input.name ?? "");
        const file = found(kind, name);
        if (file instanceof CtlReplyFailure) return file;
        file.body = String(input.new_body ?? "");
        return answer(`${kind}_edit_body`, 3, {
          newHash: hash(file.body),
          sourcePath: path(kind, name),
        });
      },
    ]);
  }
  rows.push([
    mcpKey("skills_edit_frontmatter"),
    (_argv, stdin) => {
      const input = args(stdin);
      const refused = need("skills_edit_frontmatter", input);
      if (refused) return refused;
      const name = String(input.name ?? "");
      const file = found("skills", name);
      if (file instanceof CtlReplyFailure) return file;
      const patch = (input.patch ?? {}) as Record<string, unknown>;
      if (typeof patch.description === "string") file.description = patch.description;
      return answer("skills_edit_frontmatter", 3, {
        newHash: hash(file.description),
        sourcePath: path("skills", name),
      });
    },
  ]);
  rows.push([
    mcpKey("skills_delete"),
    (_argv, stdin) => {
      const input = args(stdin);
      const refused = need("skills_delete", input);
      if (refused) return refused;
      const name = String(input.name ?? "");
      const file = found("skills", name);
      if (file instanceof CtlReplyFailure) return file;
      file.deleted = true;
      return answer("skills_delete", 3, { removedPath: `${ROOT}/skills/${name}` });
    },
  ]);
  rows.push([
    mcpKey("agents_list_transpilers"),
    () =>
      answer("agents_list_transpilers", 1, {
        transpilers: [
          "claude-code",
          "codex-cli",
          "cursor",
          "gemini-cli",
          "roomodes",
          "vscode",
        ],
      }),
  ]);
  rows.push([
    mcpKey("skills_list_transpilers"),
    () =>
      answer("skills_list_transpilers", 1, {
        transpilers: [
          "agents-md",
          "aider",
          "amazon-q",
          "claude-code",
          "cline",
          "codex-cli",
          "continue",
          "cursor",
          "gemini-cli",
          "goose-cli",
          "jetbrains",
          "roo-code",
          "trae",
          "windsurf",
          "zed",
        ],
      }),
  ]);
  rows.push([
    mcpKey("styles_list_transpilers"),
    () =>
      answer("styles_list_transpilers", 1, {
        tier1: ["claude-code", "roomodes-style"],
        tier2: [
          "aider",
          "amazon-q",
          "cline",
          "codex-cli",
          "continue",
          "cursor",
          "gemini-cli",
          "goose",
          "jetbrains",
          "trae",
          "vscode-copilot",
          "windsurf",
          "zed",
        ],
      }),
  ]);
  return rows;
}
