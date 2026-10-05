import type { PlanV1 } from "../ui";

export type BodyKind = "agents" | "styles" | "skills";

export const NOUN: Record<BodyKind, string> = {
  agents: "agent",
  styles: "style",
  skills: "skill",
};

/** The lines that differ once the shared start and end of two texts are cut away. */
export function lineChange(before: string, after: string) {
  const a = before.split("\n");
  const b = after.split("\n");
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start += 1;
  let end = 0;
  while (
    end < a.length - start &&
    end < b.length - start &&
    a[a.length - 1 - end] === b[b.length - 1 - end]
  )
    end += 1;
  return {
    removed: a.slice(start, a.length - end),
    added: b.slice(start, b.length - end),
    start: start + 1,
  };
}

export interface EditPlanInput {
  kind: BodyKind;
  name: string;
  path: string;
  body: { before: string; after: string };
  description?: { before: string; after: string };
}

/** The preview of an edit: the tools have no dry run, so the plan is what the editor knows. */
export function editPlan(input: EditPlanInput): PlanV1 {
  const { kind, name, path, body, description } = input;
  const noun = NOUN[kind];
  const steps: PlanV1["steps"] = [];
  if (description && description.before !== description.after) {
    steps.push({
      op: "update",
      path,
      detail: `Change the description of ${noun} ${name}`,
      keys: ["description"],
      diff: { before: description.before, after: description.after },
    });
  }
  if (body.before !== body.after) {
    const change = lineChange(body.before, body.after);
    steps.push({
      op: "update",
      path,
      detail: `Replace the body: ${change.removed.length} line(s) removed, ${change.added.length} added, from line ${change.start}`,
      diff: { before: change.removed.join("\n"), after: change.added.join("\n") },
    });
  }
  return {
    summary: `Edit ${noun} ${name}`,
    steps,
    effects: {},
    warnings: [
      "The file in the library is rewritten. Synced copies change at the next sync.",
    ],
    undo: "Open the editor again and restore the lines above, or revert the file with git.",
  };
}
