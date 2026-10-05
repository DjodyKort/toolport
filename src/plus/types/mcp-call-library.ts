import { bool, num, obj, str, type Infer, type Shape } from "../bridge/shape";
import {
  agentsEditBodyResult,
  agentsGetResult,
  agentsListTranspilersResult,
} from "./selfmcp-agents";
import {
  skillsDeleteResult,
  skillsEditBodyResult,
  skillsEditFrontmatterResult,
  skillsGetResult,
  skillsGitPushResult,
  skillsListTranspilersResult,
} from "./selfmcp-skills";
import {
  stylesActiveResult,
  stylesEditBodyResult,
  stylesGetResult,
  stylesListTranspilersResult,
} from "./selfmcp-styles";

/** `data` of `toolportctl mcp call <tool>` for the tools of the Library screens (contract
 * section 15): the tool's own answer wrapped with its tier, checked against the `mcp-call.*`
 * goldens by `bridge/data.test.ts`. The `result` shapes are those of `selfmcp-*.ts`. */
const answer = <T>(result: Shape<T>) =>
  obj({ isError: bool, result, tier: num, tool: str });

export const libraryCallAnswer = answer(skillsGetResult);
export type LibraryCallAnswer = Infer<typeof libraryCallAnswer>;

export const mcpCallLibraryShapes: Record<string, Shape<unknown>> = {
  "mcp-call.agents_edit_body": answer(agentsEditBodyResult),
  "mcp-call.agents_get": answer(agentsGetResult),
  "mcp-call.agents_list_transpilers": answer(agentsListTranspilersResult),
  "mcp-call.skills_delete": answer(skillsDeleteResult),
  "mcp-call.skills_edit_body": answer(skillsEditBodyResult),
  "mcp-call.skills_edit_frontmatter": answer(skillsEditFrontmatterResult),
  "mcp-call.skills_get": answer(skillsGetResult),
  "mcp-call.skills_git_push": answer(skillsGitPushResult),
  "mcp-call.skills_list_transpilers": answer(skillsListTranspilersResult),
  "mcp-call.styles_active": answer(stylesActiveResult),
  "mcp-call.styles_edit_body": answer(stylesEditBodyResult),
  "mcp-call.styles_get": answer(stylesGetResult),
  "mcp-call.styles_list_transpilers": answer(stylesListTranspilersResult),
};
