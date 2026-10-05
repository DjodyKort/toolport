import {
  arr,
  lit,
  nullable,
  num,
  obj,
  opt,
  rec,
  str,
  bool,
  type Infer,
  type Shape,
} from "../bridge/shape";
import { planV1, resultV1 } from "../bridge/data";

/** `data` of `attention ls|dismiss` (contract section 9, MIG-GUI-11) and the result of the
 * self-MCP tool `attention_ls`, checked against the golden envelopes by `data.test.ts` and
 * `selfmcp.test.ts`. Reading is offline: every feed reads local state only, and a feed that
 * cannot answer contributes nothing. No item carries a secret value. */

export const attentionLevel = lit("needs-you", "look", "fyi");
export type AttentionLevel = Infer<typeof attentionLevel>;

/** The screen a row opens: a route of the app and the params that select the tab and the thing. */
export const attentionTarget = obj({ route: str, params: rec(str) });
export type AttentionTarget = Infer<typeof attentionTarget>;

/** The one command a row offers; `command` is an argv that starts with `toolportctl`. */
export const attentionAction = obj({ label: str, command: arr(str) });
export type AttentionAction = Infer<typeof attentionAction>;

export const attentionItem = obj({
  id: str,
  level: attentionLevel,
  title: str,
  detail: str,
  from: str,
  target: attentionTarget,
  action: nullable(attentionAction),
  since: str,
});
export type AttentionItem = Infer<typeof attentionItem>;

/** `counts` always cover every level; `--level` filters only `items`. */
export const attentionLsData = obj({
  counts: obj({ needsYou: num, look: num, fyi: num }),
  items: arr(attentionItem),
});
export type AttentionLsData = Infer<typeof attentionLsData>;

/** `until` is the date the row returns (`null` hides it for good); a dry run answers with `plan`,
 * an apply with `result`, and its `undo` is the command that shows the row again. */
export const attentionDismissData = obj({
  id: str,
  until: nullable(str),
  dryRun: bool,
  plan: opt(planV1),
  result: opt(resultV1),
});
export type AttentionDismissData = Infer<typeof attentionDismissData>;

/** Golden file stem to the shape of its envelope `data`. */
export const attentionShapes: Record<string, Shape<unknown>> = {
  "attention-ls.default": attentionLsData,
  "attention-ls.needs-you": attentionLsData,
  "attention-ls.fyi": attentionLsData,
  "attention-dismiss.preview": attentionDismissData,
  "attention-dismiss.apply": attentionDismissData,
  "attention-dismiss.hidden": attentionLsData,
  "attention-dismiss.until-preview": attentionDismissData,
  "attention-dismiss.forever": attentionDismissData,
};

/** Tool name to the shape of its `structuredContent`; it answers like the CLI. */
export const attentionToolShapes: Record<string, Shape<unknown>> = {
  attention_ls: attentionLsData,
};
