/** `PlanV1` and `ResultV1` of `index/cli-contract-gui-wave.md` section 0: what a dry run of
 * a new command returns in `data.plan` and what its apply returns in `data.result`. */
export type PlanOp = "create" | "merge" | "update" | "delete" | "exec" | "note";

export interface PlanStep {
  op: PlanOp;
  path?: string;
  detail: string;
  keys?: string[];
  diff?: { before: string; after: string };
}

export interface PlanV1 {
  summary: string;
  steps: PlanStep[];
  effects: { tokens?: { before: number; after: number; basis: string } };
  warnings: string[];
  undo: string;
}

export interface ResultV1 {
  applied: true;
  changed: string[];
  undo: string;
  backups: string[];
}

const OPS: PlanOp[] = ["create", "merge", "update", "delete", "exec", "note"];

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function isStep(value: unknown): value is PlanStep {
  const step = record(value);
  return !!step && OPS.includes(step.op as PlanOp) && typeof step.detail === "string";
}

export function isPlanV1(value: unknown): value is PlanV1 {
  const plan = record(value);
  return (
    !!plan &&
    typeof plan.summary === "string" &&
    Array.isArray(plan.steps) &&
    plan.steps.every(isStep)
  );
}

/** The plan of a dry-run `data`: its `plan` member, or the data itself when it is one. */
export function planOf(data: unknown): PlanV1 | null {
  const wrapped = record(data)?.plan;
  if (isPlanV1(wrapped)) return wrapped;
  return isPlanV1(data) ? data : null;
}

export function resultOf(data: unknown): ResultV1 | null {
  const result = record(data)?.result;
  const value = record(result);
  if (!value || value.applied !== true || !Array.isArray(value.changed)) return null;
  return value as unknown as ResultV1;
}

/** The data without the `result` member that `resultOf` already read. */
export function withoutResult(data: unknown): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(record(data) ?? {}).filter(([key]) => key !== "result"),
  );
}

/** `notInProfile` to "Not in profile". */
export function humanKey(key: string): string {
  const spaced = key
    .replace(/[_-]+/g, " ")
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .toLowerCase();
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

export function prettyJson(value: unknown): string {
  return JSON.stringify(value, null, 2) ?? String(value);
}

export type Shown =
  | { kind: "scalar"; text: string }
  | { kind: "list"; items: string[] }
  | { kind: "record"; entries: Array<[string, Shown]> }
  | { kind: "json"; text: string };

const MAX_DEPTH = 3;
const MAX_ENTRIES = 40;

function scalarText(value: unknown): string | null {
  if (value === null || value === undefined) return "none";
  if (typeof value === "boolean") return value ? "yes" : "no";
  if (typeof value === "string" || typeof value === "number") return String(value);
  return null;
}

/** Turns data into what the screen can list: scalars, lists of scalars and records of those,
 * nested up to three levels. Anything else (arrays of objects, deeper trees) is shown as
 * pretty JSON, so an unknown shape still renders. */
export function toShown(value: unknown, depth = 0): Shown {
  const scalar = scalarText(value);
  if (scalar !== null) return { kind: "scalar", text: scalar };
  if (Array.isArray(value)) {
    const items = value.map(scalarText);
    if (items.every((item): item is string => item !== null))
      return { kind: "list", items };
    return { kind: "json", text: prettyJson(value) };
  }
  const object = record(value);
  if (!object || depth >= MAX_DEPTH) return { kind: "json", text: prettyJson(value) };
  const keys = Object.keys(object).filter((key) => key !== "dryRun");
  if (keys.length > MAX_ENTRIES) return { kind: "json", text: prettyJson(value) };
  const entries: Array<[string, Shown]> = keys.map((key) => [
    key,
    toShown(object[key], depth + 1),
  ]);
  if (entries.some(([, shown]) => shown.kind === "json"))
    return { kind: "json", text: prettyJson(value) };
  return { kind: "record", entries };
}
