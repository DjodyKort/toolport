import type { CommandRow } from "../bridge/data";
import type { Tier } from "../allcommands/model";
import type { PlanOp, PlanStep, PlanV1 } from "../ui";

export const PROVIDERS = [
  { id: "rtk-only", blurb: "Hook runtime, no proxy" },
  { id: "headroom", blurb: "Local proxy with cache mode" },
  { id: "parsec", blurb: "Hosted engine" },
  { id: "none", blurb: "Compression off" },
] as const;

export interface Policy {
  tier: Tier;
  previewFlag: string | null;
  terminal: boolean;
  network: boolean;
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
    network: row.needs.includes("network"),
  };
}

const STEP_OPS: Array<[RegExp, PlanOp]> = [
  [/^(would )?(run|start|stop)\b/, "exec"],
  [/^(would )?(remove|unregister|delete|unwrap)\b/, "delete"],
  [/^(would )?(write|register|create)\b/, "create"],
  [/^(would )?(save|update|set|apply)\b/, "update"],
];

export function stepOf(action: string): PlanStep {
  const op = STEP_OPS.find(([pattern]) => pattern.test(action))?.[1] ?? "note";
  return { op, detail: action };
}

interface CompressionWrite {
  actions: string[];
  warnings: unknown[];
  provider: string;
  runtime: string;
  preset: { name: string; mode: string; port: number };
}

const asText = (value: unknown) =>
  typeof value === "string" ? value : JSON.stringify(value);

/** The dry run of `set-provider`, `use`, `enable`, `disable` and `sync` as a plan: the CLI's
 * own action lines, word for word, so the preview is the dry run and nothing else. */
export function planOfWrite(data: unknown, undo: string): PlanV1 | null {
  const write = data as Partial<CompressionWrite> | null;
  if (!write || !Array.isArray(write.actions) || !write.preset) return null;
  return {
    summary: `Provider ${write.provider} (${write.runtime}), preset ${write.preset.name}: ${write.preset.mode} mode, port ${write.preset.port}`,
    steps: write.actions.map(stepOf),
    effects: {},
    warnings: (write.warnings ?? []).map(asText),
    undo,
  };
}

export const ctl = (...parts: string[]) => ["compression", ...parts];
