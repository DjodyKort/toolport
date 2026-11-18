/** The fixture side of `toolportctl library select`: one tap, `acme-skills`, whose items, pin and
 * upstream move follow the scenario. A second read sees the first write, like the real store. */
import type { CtlReply } from "./ctlWorld";
import type {
  ItemKind,
  SelectCheckData,
  SelectHistory,
  SelectItem,
  SelectLsData,
  SelectPlanData,
  RuleRow,
} from "../library/selectData";

export type SelectScenario =
  | "fresh"
  | "chosen"
  | "update"
  | "whole"
  | "hooks-off"
  | "blocked"
  | "offline"
  | "gone-only"
  | "everything"
  | "rewritten"
  | "rewritten-seen"
  | "comma"
  | "rule-conflict";

export const SELECT_SOURCE = "acme-skills";
const WHOLE = "acme-skills@acme-skills";
const SLICE = "acme-skills-selected@toolport-selections";
const OLD_REF = "a1b2c3d4e5f6";
const NEW_REF = "e4f5a6b7c8d9";
const FORMAT_ID = "PostToolUse:Write|Edit";
const FORMAT = '"${CLAUDE_PLUGIN_ROOT}"/scripts/format.sh';

const ok = (data: unknown): CtlReply => ({ ok: true, data });
const fail = (code: string, message: string, data?: unknown): CtlReply => ({
  ok: false,
  error: { code, message },
  data,
});

const hash = (text: string) => {
  let h = 5381;
  for (const c of text) h = (h * 33) ^ c.charCodeAt(0);
  return `sha256:${(h >>> 0).toString(16).padStart(8, "0")}${text.length.toString(16).padStart(8, "0")}`;
};

function item(
  kind: ItemKind,
  id: string,
  state: SelectItem["state"],
  extra: Partial<SelectItem> = {},
): SelectItem {
  return {
    id,
    kind,
    state,
    hash: hash(`${kind}:${id}`),
    inSlice: state === "on",
    ...extra,
  };
}

function hook(id: string, event: string, matcher: string, command: string, on: boolean) {
  return item("hook", id, on ? "on" : "new", { event, matcher, command });
}

function seed(on: boolean): SelectItem[] {
  const state = (yes: boolean) => (on && yes ? "on" : on ? "off" : "new");
  return [
    item("skill", "code-review", state(true)),
    item("skill", "tdd-workflow", state(false)),
    item("skill", "deploy-runbook", state(false)),
    item("skill", "security-scan", state(false)),
    item("agent", "reviewer", state(true)),
    item("agent", "planner", state(false)),
    item("command", "deploy", state(false)),
    hook(FORMAT_ID, "PostToolUse", "Write|Edit", FORMAT, on),
    hook("PreToolUse:Bash", "PreToolUse", "Bash", "echo checked", false),
    item("rule", "style", state(false)),
  ];
}

const key = (i: { kind: string; id: string }) => `${i.kind}:${i.id}`;
const wanted = (i: SelectItem) => i.state === "on" || i.state === "removed";

export function createSelectWorld(scenario: SelectScenario | undefined) {
  const scene = scenario ?? "fresh";
  const chosen = scene !== "fresh" && scene !== "whole" && scene !== "blocked";
  const rewrittenScene = scene === "rewritten" || scene === "rewritten-seen";
  let items = seed(chosen || scene === "blocked");
  if (scene === "comma") items.push(item("skill", "lint,format", "off"));
  if (scene === "everything")
    items = items.map((i) => ({ ...i, state: "on", inSlice: true }));
  let recorded = chosen || scene === "blocked";
  let pinned: string | null = recorded ? OLD_REF : null;
  let moved = scene === "update" || scene === "gone-only" || rewrittenScene;
  let historyState: SelectHistory["state"] =
    scene === "rewritten-seen" ? "rewritten" : "unchecked";
  let linked = new Set<string>();
  let foreign = new Set<string>(scene === "rule-conflict" ? ["style"] : []);
  let journal: {
    items: SelectItem[];
    pinned: string | null;
    recorded: boolean;
    linked: Set<string>;
    foreign: Set<string>;
  } | null = null;
  let whole = scene === "whole";
  if (scene === "update" || scene === "gone-only") {
    items.push(item("skill", "legacy-lint", "on"));
  }
  const policy = {
    hooksOff: scene === "hooks-off" ? "your Claude Code settings" : null,
    blocked: scene === "blocked" ? "the managed policy" : null,
  };
  const offer = () =>
    whole
      ? {
          kind: "switch-to-chosen-items" as const,
          whole: WHOLE,
          slice: SLICE,
          text: "This plugin is switched on whole.",
        }
      : null;

  const upstream = () => {
    const rows = {
      changed: [] as Array<{ kind: ItemKind; id: string; from: string; to: string }>,
      retick: [] as Array<{ kind: ItemKind; id: string; from: string; to: string }>,
      fresh: [] as Array<{ kind: ItemKind; id: string }>,
      gone: [] as Array<{ kind: ItemKind; id: string }>,
    };
    if (!moved) return rows;
    const find = (kind: ItemKind, id: string) =>
      items.find((i) => i.kind === kind && i.id === id);
    const review = find("skill", "code-review");
    if (scene === "update" && review && wanted(review)) {
      rows.changed.push({
        kind: "skill",
        id: "code-review",
        from: review.hash,
        to: hash("skill:code-review:v2"),
      });
    }
    const format = find("hook", FORMAT_ID);
    if (scene === "update" && format && wanted(format) && !format.pending) {
      rows.retick.push({
        kind: "hook",
        id: FORMAT_ID,
        from: format.hash,
        to: hash("format --all"),
      });
    }
    if (scene === "update") {
      rows.fresh.push(
        { kind: "skill", id: "perf-profile" },
        { kind: "hook", id: "PreToolUse:Audit" },
      );
    }
    const legacy = find("skill", "legacy-lint");
    if (legacy && legacy.state === "on")
      rows.gone.push({ kind: "skill", id: "legacy-lint" });
    return rows;
  };

  const ruleRows = (): RuleRow[] => [
    ...items
      .filter((i) => i.kind === "rule" && i.state === "on" && !linked.has(i.id))
      .map((i): RuleRow => {
        const row: RuleRow = {
          id: i.id,
          name: i.id,
          action: "install",
          path: `rules/${i.id}`,
        };
        if (foreign.has(i.id)) {
          row.conflict = {
            code: "foreign",
            reason: `rules/${i.id} already exists in the Library and Toolport did not write it`,
            choices: ["force", "skip"],
          };
        }
        return row;
      }),
    ...[...linked]
      .filter((id) => !items.some((i) => i.kind === "rule" && i.id === id && wanted(i)))
      .map((id): RuleRow => ({ id, name: id, action: "remove", path: `rules/${id}` })),
  ];

  function planData(switchWhole: boolean): SelectPlanData {
    const up = upstream();
    const added = items.filter((i) => wanted(i) && !i.inSlice && i.kind !== "rule");
    const dropped = items.filter((i) => !wanted(i) && i.inSlice);
    const rules = ruleRows();
    const diffs = {
      added: added.map((i) => ({ kind: i.kind, id: i.id })),
      dropped: dropped.map((i) => ({ kind: i.kind, id: i.id })),
      changed: up.changed,
      retick: up.retick,
      fresh: up.fresh,
      gone: up.gone,
      rules: items
        .filter((i) => i.kind === "rule" && i.state === "on")
        .map((i) => ({ kind: i.kind, id: i.id })),
    };
    const warnings: string[] = [];
    const offline = scene === "offline";
    if (offline)
      warnings.push("could not check the source; this plan uses the local clone");
    if (items.some((i) => i.kind === "hook" && wanted(i))) {
      warnings.push(
        "ticked hooks run commands in every session once the plan is applied",
      );
    }
    if (policy.blocked)
      warnings.push(`${policy.blocked} blocks the marketplace toolport-selections`);
    const steps = [
      ...diffs.added.map((r) => ({
        op: "add",
        detail: `${r.kind}:${r.id} joins ${SLICE}`,
      })),
      ...diffs.dropped.map((r) => ({
        op: "drop",
        detail: `${r.kind}:${r.id} leaves ${SLICE}`,
      })),
      ...rules.map((r) =>
        r.conflict
          ? {
              op: "conflict",
              detail: `rule:${r.id}: ${r.conflict.reason}; choose --force ${r.id} or --skip ${r.id} when applying`,
            }
          : r.action === "install"
            ? {
                op: "link",
                detail: `rule:${r.id} is installed into the Library as ${r.path}`,
              }
            : {
                op: "unlink",
                detail: `rule:${r.id} is off now; ${r.path} is removed from the Library`,
              },
      ),
      { op: "merge", detail: `enable ${SLICE}` },
      ...(switchWhole && whole ? [{ op: "merge", detail: `disable ${WHOLE}` }] : []),
      {
        op: "note",
        detail:
          "Run /reload-plugins in running Claude Code sessions, or restart them, to load the change",
      },
    ];
    const latest = moved ? NEW_REF : (pinned ?? OLD_REF);
    return {
      source: SELECT_SOURCE,
      slice: SLICE,
      planHash: hash(JSON.stringify([diffs, latest, switchWhole && whole, rules])),
      waiting:
        diffs.added.length +
          diffs.dropped.length +
          up.changed.length +
          up.retick.length +
          rules.length >
          0 || moved,
      dryRun: true,
      ref: { pinned, latest },
      checked: !offline,
      fetchError: offline ? "could not resolve host" : null,
      history: history(),
      rules,
      conflicts: rules.filter((r) => r.conflict),
      diffs,
      lines: up.changed.length
        ? { "skill:code-review": { added: 12, removed: 3, files: 1 } }
        : {},
      hooks: Object.fromEntries([
        ...up.retick.map((r) => [
          key(r),
          {
            event: "PostToolUse",
            matcher: "Write|Edit",
            command: `${FORMAT} --all`,
            was: FORMAT,
          },
        ]),
        ...up.fresh
          .filter((r) => r.kind === "hook")
          .map((r) => [
            key(r),
            { event: "PreToolUse", matcher: "Audit", command: "echo audited", was: null },
          ]),
      ]),
      policy,
      offer: offer(),
      plan: {
        summary: `Load the chosen items of ${SELECT_SOURCE} through ${SLICE}: ${items.filter(wanted).length} of ${items.length} item(s) on`,
        steps,
        effects: {},
        warnings,
        undo: `toolportctl library select undo ${SELECT_SOURCE}`,
      },
      result: null,
    };
  }

  const history = (): SelectHistory => ({
    state: pinned === null ? "unchecked" : historyState,
    pinned,
    error: null,
  });

  function checkHistory(): CtlReply {
    historyState = rewrittenScene ? "rewritten" : "intact";
    const data: SelectCheckData = {
      checkedAt: "2026-11-18T10:00:00Z",
      waiting: moved ? 1 : 0,
      sources: [{ source: SELECT_SOURCE, history: history() }],
    };
    return ok(data);
  }

  function ls(): CtlReply {
    const data: SelectLsData = {
      source: SELECT_SOURCE,
      tap: SELECT_SOURCE,
      ref: pinned,
      history: history(),
      slice: SLICE,
      recorded,
      on: items.filter((i) => i.state === "on").length,
      total: items.length,
      new: items.filter((i) => i.state === "new").length,
      removed: items.filter((i) => i.state === "removed").length,
      items: structuredClone(items),
      offer: offer(),
      policy,
      warnings: [],
    };
    return ok(data);
  }

  const values = (argv: string[], flag: string) =>
    argv.flatMap((a, i) =>
      a === flag ? [(argv[i + 1] ?? "").replace(/\\([,\\])/g, "$1")] : [],
    );
  const find = (ref: string) =>
    items.find((i) => key(i) === ref) ?? items.find((i) => i.id === ref);

  function set(argv: string[]): CtlReply {
    const [on, off, ticks] = [
      values(argv, "--on"),
      values(argv, "--off"),
      values(argv, "--tick-hook"),
    ];
    if (on.length + off.length + ticks.length === 0) {
      return fail("usage", "nothing to set: pass --on, --off or --tick-hook");
    }
    for (const ref of [...on, ...off, ...ticks]) {
      if (!find(ref)) {
        return fail(
          "not_found",
          `no item '${ref}' in this source; \`library select ls <source>\` lists them`,
        );
      }
    }
    for (const ref of on) {
      if (find(ref)?.kind === "hook") {
        return fail(
          "usage",
          `hook '${ref}' runs a command in every session; tick it with --tick-hook so the command is read first`,
        );
      }
    }
    const changed: string[] = [];
    recorded = true;
    pinned ??= OLD_REF;
    for (const ref of on) {
      const found = find(ref)!;
      found.state = "on";
      changed.push(`${key(found)} on`);
    }
    for (const ref of ticks) {
      const found = find(ref)!;
      if (found.pending) {
        found.hash = hash(found.pending.command);
        found.command = found.pending.command;
        found.pending = undefined;
        found.inSlice = false;
      }
      found.state = "on";
      changed.push(`${key(found)} ticked`);
    }
    for (const ref of off) {
      const found = find(ref)!;
      found.state = "off";
      found.pending = undefined;
      changed.push(`${key(found)} off`);
    }
    return ok({
      source: SELECT_SOURCE,
      dryRun: false,
      changed,
      on: items.filter((i) => i.state === "on").length,
      total: items.length,
      next: `toolportctl library select plan ${SELECT_SOURCE}`,
    });
  }

  function apply(argv: string[]): CtlReply {
    const switchWhole = argv.includes("--switch");
    const plan = planData(switchWhole);
    const given = values(argv, "--plan-hash")[0];
    if (given !== plan.planHash) {
      return fail(
        "conflict",
        `the plan changed since it was made (now ${plan.planHash}); run \`toolportctl library select plan ${SELECT_SOURCE}\` again and apply that`,
        { planHash: plan.planHash },
      );
    }
    const rules = ruleRows();
    const force = values(argv, "--force").flatMap((v) => v.split(","));
    const skip = values(argv, "--skip").flatMap((v) => v.split(","));
    const open = rules.filter(
      (r) => r.conflict && !force.includes(r.id) && !skip.includes(r.id),
    );
    if (open.length > 0) {
      return fail(
        "conflict",
        `${open.length} rule(s) need a choice before apply: ${open.map((r) => `rule:${r.id}`).join(", ")}; nothing was changed`,
      );
    }
    journal = {
      items: structuredClone(items),
      pinned,
      recorded,
      linked: new Set(linked),
      foreign: new Set(foreign),
    };
    const written: string[] = [];
    for (const row of rules) {
      if (skip.includes(row.id)) continue;
      if (row.action === "install") {
        linked.add(row.id);
        foreign.delete(row.id);
        written.push(`${row.path} written`);
      } else {
        linked.delete(row.id);
        written.push(`${row.path} removed`);
      }
    }
    const up = upstream();
    for (const row of up.changed) {
      const found = items.find((i) => key(i) === key(row));
      if (found) found.hash = row.to;
    }
    for (const row of up.retick) {
      const found = items.find((i) => key(i) === key(row));
      if (found) found.pending = { hash: row.to, command: `${FORMAT} --all` };
    }
    for (const row of up.fresh) {
      items.push(
        row.kind === "hook"
          ? hook(row.id, "PreToolUse", "Audit", "echo audited", false)
          : item(row.kind, row.id, "new"),
      );
    }
    for (const row of up.gone) {
      const found = items.find((i) => key(i) === key(row));
      if (found) found.state = "removed";
    }
    for (const found of items) found.inSlice = wanted(found) && found.kind !== "rule";
    if (switchWhole) whole = false;
    if (moved) historyState = "unchecked";
    pinned = moved ? NEW_REF : (pinned ?? OLD_REF);
    moved = false;
    return ok({
      ...plan,
      dryRun: false,
      waiting: false,
      result: { applied: true, changed: [...written, `enabledPlugins.${SLICE}`] },
    });
  }

  function undo(): CtlReply {
    if (!journal) return fail("not_found", `nothing to undo for ${SELECT_SOURCE}`);
    items = journal.items;
    pinned = journal.pinned;
    recorded = journal.recorded;
    linked = journal.linked;
    foreign = journal.foreign;
    journal = null;
    return ok({
      source: SELECT_SOURCE,
      plan: {
        summary: "Restore the selection",
        steps: [],
        warnings: [],
        undo: "",
        effects: {},
      },
      result: { applied: true, changed: [] },
    });
  }

  return {
    run(argv: string[]): CtlReply {
      const [, , command, source] = argv;
      if (command === "ls" && source === undefined) {
        return ok({
          sources: recorded
            ? [
                {
                  source: SELECT_SOURCE,
                  slice: SLICE,
                  ref: pinned,
                  on: items.filter((i) => i.state === "on").length,
                  new: items.filter((i) => i.state === "new").length,
                  removed: items.filter((i) => i.state === "removed").length,
                  total: items.length,
                },
              ]
            : [],
          offers: whole ? [{ source: SELECT_SOURCE, offer: offer() }] : [],
        });
      }
      if (source !== SELECT_SOURCE)
        return fail("not_found", `Tap '${source ?? ""}' not found`);
      switch (command) {
        case "ls":
          return ls();
        case "set":
          return set(argv);
        case "plan":
          return ok(planData(argv.includes("--switch")));
        case "apply":
          return apply(argv);
        case "undo":
          return undo();
        case "check":
          return checkHistory();
        default:
          return fail("usage", `unknown library select command: ${command ?? ""}`);
      }
    },
  };
}

export type SelectWorld = ReturnType<typeof createSelectWorld>;
