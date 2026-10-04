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

type Loose = Record<string, unknown>;

const rec = (value: unknown): Loose | null =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Loose)
    : null;
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const text = (value: unknown) => (typeof value === "string" ? value : "");

/** The knob changes of a preset re-snapshot, as `refresh_lines` prints them. */
function refreshSteps(refresh: unknown): PlanStep[] {
  const data = rec(refresh);
  if (!data) return [];
  const version = text(data.version) || "unknown";
  const steps: PlanStep[] = [];
  let changed = false;
  for (const entry of list(data.presets)) {
    const preset = rec(entry);
    if (!preset) continue;
    const name = text(preset.name);
    if (preset.changed === true) {
      changed = true;
      const keys = [
        ...list(preset.added).map(
          (a) => `+ ${text(rec(a)?.knob)}=${text(rec(a)?.value)}`,
        ),
        ...list(preset.removed).map(
          (r) => `- ${text(rec(r)?.knob)} (was ${text(rec(r)?.was)})`,
        ),
        ...list(preset.moved).map(
          (m) =>
            `~ ${text(rec(m)?.knob)}: ${text(rec(m)?.from)} \u2192 ${text(rec(m)?.to)}`,
        ),
      ];
      steps.push({
        op: "update",
        detail: `Re-snapshot preset ${name} from ${version}`,
        keys,
      });
    }
    const kept = list(preset.kept).map(text);
    if (kept.length > 0)
      steps.push({
        op: "note",
        detail: `Preset ${name} keeps ${kept.length} declared knob(s): ${kept.join(", ")}`,
      });
  }
  if (!changed)
    steps.push({ op: "note", detail: `Preset knobs already match ${version}` });
  return steps;
}

/** `pin` (set, install, refresh) and `presets --refresh` as a plan. A dry run and an apply
 * come out in the same words, so the confirmation shows what the result will say. */
export function planOfPin(data: unknown, undo: string): PlanV1 | null {
  const pin = rec(data);
  if (!pin || typeof pin.pin !== "string") return null;
  const steps: PlanStep[] = [];
  if (pin.set === true)
    steps.push({
      op: "update",
      detail: `Pin the engine to ${pin.pin} (requirement: ${text(pin.requirement)})`,
    });
  const install = rec(pin.install);
  if (install)
    steps.push({
      op: "exec",
      detail:
        install.dryRun === false
          ? `Installed ${text(install.requirement)}: ${text(install.detail)}`
          : `Install ${text(install.requirement)}`,
    });
  if (rec(pin.refresh)) steps.push(...refreshSteps(pin.refresh));
  if (pin.restartProxies === true)
    steps.push({
      op: "note",
      detail: "Restart the proxy to run the pinned build (close attached sessions first)",
    });
  if (steps.length === 0)
    steps.push({ op: "note", detail: `Pinned at ${pin.pin}, nothing to change` });
  return {
    summary: `Engine pin ${pin.pin}`,
    steps,
    effects: {},
    warnings:
      pin.drift === true
        ? [`The installed build ${text(pin.installed)} differs from the pin ${pin.pin}`]
        : [],
    undo,
  };
}

export function planOfPresets(data: unknown, undo: string): PlanV1 | null {
  const refresh = rec(data)?.refresh;
  if (!rec(refresh)) return null;
  return {
    summary: "Re-snapshot the preset knobs from the installed engine",
    steps: refreshSteps(refresh),
    effects: {},
    warnings: [],
    undo,
  };
}

export function planOfUpdate(data: unknown): PlanV1 | null {
  const update = rec(data);
  if (!update || typeof update.target !== "string") return null;
  const { current, target } = update as { current: string; target: string };
  const steps: PlanStep[] =
    update.same === true
      ? [{ op: "note", detail: `Already pinned at ${target}` }]
      : [
          { op: "update", detail: `Move the pin ${current} \u2192 ${target}` },
          { op: "exec", detail: `Install the engine at ${target}` },
          { op: "update", detail: "Re-snapshot the preset knobs from that build" },
        ];
  return {
    summary: `Update the engine pin to ${target}`,
    steps,
    effects: {},
    warnings: [
      "This build is unverified against the recorded contract. Run Verify after the update.",
    ],
    undo: `toolportctl compression update --to ${current} --accept`,
  };
}

export function planOfSeal(data: unknown): PlanV1 | null {
  const seal = rec(data);
  if (!seal || !Array.isArray(seal.declarable)) return null;
  const declared = list(seal.declarable).map(rec);
  const steps: PlanStep[] = [
    ...declared.map((knob): PlanStep => ({
      op: "update",
      detail: `Declare ${text(knob?.knob)}=${text(knob?.value)}`,
    })),
    ...list(seal.unset).map((knob): PlanStep => ({
      op: "note",
      detail: `${text(knob)} stays unset`,
    })),
  ];
  if (declared.length === 0)
    steps.unshift({ op: "note", detail: "Nothing new to declare" });
  return {
    summary:
      seal.apply === true
        ? `Sealed ${Number(seal.sealed) || 0} knob(s) of preset ${text(seal.preset)} (proxy on port ${Number(seal.port)})`
        : `Declare the live posture of preset ${text(seal.preset)} (proxy on port ${Number(seal.port)}) as policy`,
    steps,
    effects: {},
    warnings: [],
    undo: "",
  };
}

export interface FlagParts {
  flag: string;
  value: string;
}

/** `--flag value` pairs for the fields that were filled in. */
export function flagArgs(parts: FlagParts[]): string[] {
  return parts.flatMap(({ flag, value }) => (value.trim() ? [flag, value.trim()] : []));
}
