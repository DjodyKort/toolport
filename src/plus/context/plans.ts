import type { PlanStep, PlanV1 } from "../ui";
import type { Check } from "./model";

type Rule = [RegExp, (m: RegExpMatchArray) => PlanStep];

const RULES: Rule[] = [
  [
    /^(?:would write|wrote) shims: (.+)$/,
    (m) => ({ op: "update", detail: "Shell shims file", path: m[1] }),
  ],
  [
    /^(?:would remove|removed) shims file$/,
    () => ({ op: "delete", detail: "Shell shims file" }),
  ],
  [
    /^(?:would remove|removed) profile dir (.+)$/,
    (m) => ({ op: "delete", detail: "Launch profile folder", path: m[1] }),
  ],
  [
    /^(?:would generate|generated) launch profile (\S+) \((.+?)\) in (.+)$/,
    (m) => ({
      op: "create",
      detail: `Launch profile ${m[1]} (${m[2]})`,
      path: m[3],
    }),
  ],
  [
    /^(?:would back up|backed up) (.+) as (.+)$/,
    (m) => ({ op: "create", detail: `Backup of ${m[1]}`, path: m[2] }),
  ],
  [
    /^(?:would copy|copied) (.+) to (.+?)(?: \(.*\))?$/,
    (m) => ({ op: "create", detail: `Copy of ${m[1]}`, path: m[2] }),
  ],
  [
    /^(?:would rewrite|rewrote) (\d+) line\(s\) of (.+)$/,
    (m) => ({ op: "update", detail: `Rewrite ${m[1]} source line(s)`, path: m[2] }),
  ],
  [
    /^(?:would save|saved) config \((\d+) profile\(s\)\)$/,
    (m) => ({ op: "update", detail: `Save the context config (${m[1]} profile(s))` }),
  ],
];

export function stepOf(line: string): PlanStep {
  for (const [pattern, build] of RULES) {
    const found = pattern.exec(line);
    if (found) return build(found);
  }
  return { op: "note", detail: line };
}

function record(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function list(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

export function checksOf(value: unknown): Check[] {
  return list(value).flatMap((entry) =>
    Array.isArray(entry) && entry.length === 2
      ? [[entry[0], String(entry[1])] as Check]
      : [],
  );
}

function zshrcSteps(zshrc: Record<string, unknown>): {
  steps: PlanStep[];
  warnings: string[];
} {
  const path = String(zshrc.path ?? "~/.zshrc");
  const steps: PlanStep[] = list(zshrc.changes).map((change) => {
    const c = record(change);
    return {
      op: "update",
      detail: `Line ${String(c.line)} of the shell rc file`,
      path,
      diff: { before: String(c.before), after: String(c.after) },
    };
  });
  const order = record(zshrc.order);
  const warnings = list(order.problems).map(String);
  return { steps, warnings };
}

const SUMMARY: Record<string, [string, string]> = {
  "context plan": ["What a context deploy would do", "Deployed"],
  "context apply": ["Deploy the context files", "Deployed"],
  "context sync": ["Plan and deploy the context files", "Deployed"],
  "context profile add": ["Define the launch profile", "Launch profile defined"],
  "context profile remove": ["Remove the launch profile", "Launch profile removed"],
  "context disable": ["Remove the generated shell shims", "Shims removed"],
  "context init": ["Set up the personal layer", "Personal layer set up"],
  "context client add": ["Add the client layer", "Client layer added"],
  "context folders": ["Change folder routing", "Folder routing changed"],
};

/** Words the data of the context commands as a `PlanV1`, so the preview and the result use the
 * same list of files as the dialogs of every other screen. */
export function contextPlan(command: string, raw: unknown, done: boolean): PlanV1 | null {
  const data = record(raw);
  const [before, after] = SUMMARY[command] ?? [command, "Done"];
  const source =
    command === "context sync"
      ? record(done ? (data.apply ?? data.plan) : data.plan)
      : data;
  const steps: PlanStep[] = list(source.actions).map((line) => stepOf(String(line)));
  const warnings = list(source.warnings).map(String);
  for (const [level, text] of checksOf(source.checks)) {
    if (level !== "ok") warnings.push(text);
  }
  const zshrc = record(source.zshrc);
  if (Object.keys(zshrc).length > 0) {
    const extra = zshrcSteps(zshrc);
    steps.push(...extra.steps);
    warnings.push(...extra.warnings);
  }
  if (command === "context init") {
    const personal = record(data.personal);
    const config = record(data.config);
    steps.push(
      personal.created
        ? { op: "create", detail: "Personal layer", path: String(personal.path) }
        : { op: "note", detail: "The personal layer already exists" },
    );
    steps.push({
      op: "update",
      detail: "Context config",
      path: String(config.path),
    });
  }
  if (command === "context folders") {
    steps.push({
      op: "update",
      detail: data.enabled ? "Folder routing is on" : "Folder routing is off",
    });
  }
  if (command === "context client add") {
    steps.push(
      data.created
        ? {
            op: "create",
            detail: `Layer ${String(data.rule)} for ${String(data.glob)}`,
            path: String(data.path),
          }
        : { op: "note", detail: `Layer ${String(data.rule)} already exists` },
    );
  }
  if (command === "context profile add") {
    const profile = record(data.profile);
    if (!profile.created)
      steps.unshift({
        op: "note",
        detail: `Replaces the profile ${String(profile.name)}`,
      });
  }
  if (steps.length === 0 && Object.keys(data).length === 0) return null;
  return {
    summary: done ? after : before,
    steps: steps.length > 0 ? steps : [{ op: "note", detail: "Nothing to write" }],
    effects: {},
    warnings,
    undo: "",
  };
}
