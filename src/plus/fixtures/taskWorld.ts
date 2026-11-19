import { now as pageNow } from "../tasks/clock";
import type {
  MissedRecord,
  Plan,
  RunStatus,
  RunTrigger,
  RunView,
  StepRunView,
  TaskDefinition,
  TaskProblem,
  TaskStep,
} from "../tasks/data";
import type { CtlReply } from "./ctlWorld";

/** The fixed morning of the screenshots and the design: Tuesday 17 November 2026, 09:58 in
 * Amsterdam. */
export const FIXTURE_NOW = Date.parse("2026-11-17T08:58:00Z");

const MIN = 60_000;
const HOUR = 60 * MIN;

export interface TaskWorldOptions {
  /** `default`: the five tasks of the design. `empty`: none. `edge`: adds a 1.x task that names a
   * routine and a task file that cannot be read. */
  scenario?: "default" | "empty" | "edge";
  /** Milliseconds a live run spends per step; large keeps a run live for a test. */
  stepMs?: number;
  /** Milliseconds `task cancel` takes to answer. */
  cancelDelayMs?: number;
  /** Servers that are switched off, so a plan warns about them. */
  offServers?: string[];
  /** `task ls` fails with this message. */
  listError?: string;
  /** Without a heartbeat the scheduler line is left out. */
  heartbeat?: boolean;
  /** The runner of a waiting run is gone. */
  runnerGone?: boolean;
  /** A first run that starts while this one is already live is refused. */
  refuseStart?: boolean;
  /** The built-in scheduled check of chosen items and plugin marketplaces, off by default. */
  updateCheck?: boolean | "off";
}

function updateCheckTask(enabled: boolean): TaskDefinition {
  return define(
    "selective-update-check",
    "Check chosen items for updates",
    [
      exec("check", "Fetch the chosen sources", "toolportctl", [
        "library",
        "select",
        "check",
      ]),
    ],
    {
      enabled,
      triggers: triggers({ schedule: { cron: "0 6 * * *", autoRun: true } }),
      createdFrom: { kind: "manual" },
    },
  );
}

const fail = (code: string, message: string, data?: unknown): CtlReply => ({
  ok: false,
  error: { code, message },
  data,
});

const iso = (at: number) => new Date(at).toISOString().replace(/\.\d+Z$/, "Z");

const exec = (id: string, title: string, program: string, args: string[]): TaskStep => ({
  id,
  title,
  type: "exec",
  program,
  args,
});

function triggers(extra: Partial<TaskDefinition["triggers"]> = {}) {
  return {
    manual: true,
    cli: true,
    selfMcp: { enabled: false, approval: "every-run" as const },
    schedule: null,
    onAuthFailure: [] as string[],
    ...extra,
  };
}

function define(
  id: string,
  title: string,
  steps: TaskStep[],
  extra: Partial<TaskDefinition> = {},
): TaskDefinition {
  return {
    id,
    title,
    description: "",
    enabled: true,
    requires: { servers: [], commands: [] },
    writesSecrets: [],
    steps,
    triggers: triggers(),
    createdFrom: null,
    ...extra,
  };
}

export function defaultTasks(): TaskDefinition[] {
  return [
    define(
      "export-notes",
      "Export notes",
      [
        exec("collect", "Collect the notes", "notes-export", ["--collect"]),
        exec("archive", "Build the archive", "tar", [
          "-czf",
          "~/exports/notes.tgz",
          "~/exports/stage",
        ]),
        {
          id: "confirm",
          title: "Confirm upload",
          type: "needs-you",
          instructions: "Check the archive is under 200 MB, then continue.",
        },
        {
          id: "upload",
          title: "Upload",
          type: "mcp",
          server: "file-store",
          tool: "upload",
          args: {},
        },
      ],
      {
        requires: { servers: ["file-store"], commands: ["notes-export", "tar"] },
        triggers: triggers({ selfMcp: { enabled: true, approval: "every-run" } }),
        createdFrom: { kind: "manual" },
      },
    ),
    define(
      "nightly-sync",
      "Sync the notes mirror",
      [
        exec("pull", "Pull remote changes", "git", [
          "-C",
          "~/work/notes-mirror",
          "pull",
          "--ff-only",
        ]),
        exec("export", "Export changed notes", "notes-export", ["--since-last"]),
        exec("push", "Push the mirror", "git", ["-C", "~/work/notes-mirror", "push"]),
      ],
      {
        description: "Pull, export and push the shared notes mirror every four hours.",
        requires: { servers: [], commands: ["git", "notes-export"] },
        triggers: triggers({ schedule: { cron: "7 1-23/4 * * *", autoRun: true } }),
        createdFrom: { kind: "manual" },
      },
    ),
    define(
      "weekly-digest",
      "Weekly digest",
      [
        {
          id: "digest",
          title: "Write the digest",
          type: "prompt",
          prompt: "Summarise last week's notes in ten lines.",
          allowedTools: ["Read"],
        },
      ],
      {
        triggers: triggers({
          schedule: { cron: "0 7 * * 1", autoRun: false },
          selfMcp: { enabled: true, approval: "every-run" },
        }),
        createdFrom: { kind: "manual" },
      },
    ),
    define(
      "renew-beta-token",
      "Renew the beta-mail token",
      [
        {
          id: "read",
          title: "Read the token",
          type: "mcp",
          server: "beta-mail",
          tool: "get_token",
          args: {},
          capture: ["token"],
        },
        {
          id: "store",
          title: "Store the token",
          type: "secret-set",
          server: "beta-mail",
          key: "BETA_TOKEN",
          from: "token",
        },
      ],
      {
        requires: { servers: ["beta-mail"], commands: [] },
        writesSecrets: [{ server: "beta-mail", key: "BETA_TOKEN" }],
        triggers: triggers({ onAuthFailure: ["beta-mail"] }),
        createdFrom: { kind: "manual" },
      },
    ),
    define(
      "tidy-exports",
      "Tidy old exports",
      [
        exec("tidy", "Remove old archives", "find", [
          "~/exports",
          "-mtime",
          "+30",
          "-delete",
        ]),
      ],
      {
        enabled: false,
        requires: { servers: [], commands: ["find"] },
        triggers: triggers({ schedule: { cron: "30 2 * * *", autoRun: true } }),
        createdFrom: { kind: "manual" },
      },
    ),
  ];
}

function stepsOf(
  def: TaskDefinition,
  states: RunStatus[],
  at: number,
  output: Record<number, string> = {},
): StepRunView[] {
  return def.steps.map((step, i) => ({
    id: step.id,
    title: step.title,
    type: step.type === "routine" ? "script" : step.type,
    status: states[i] ?? "pending",
    startedAt:
      states[i] && states[i] !== "pending" && states[i] !== "skipped"
        ? iso(at + i * 6000)
        : null,
    endedAt:
      states[i] === "ok" || states[i] === "failed" ? iso(at + i * 6000 + 5000) : null,
    ...(states[i] === "waiting" && step.type === "needs-you"
      ? { instructions: step.instructions }
      : {}),
    ...(output[i] !== undefined ? { output: output[i] } : {}),
  }));
}

interface Seed {
  id: string;
  task: string;
  trigger: RunTrigger;
  status: RunStatus;
  ago: number;
  seconds: number | null;
  states: RunStatus[];
  output?: Record<number, string>;
  error?: string;
  missed?: RunView["missed"];
}

const SYNC_OUTPUT = {
  0: "$ git -C ~/work/notes-mirror pull --ff-only\nAlready up to date.",
  1: "$ notes-export --since-last\nscanning 214 notes\nerror: cannot read ~/work/notes-mirror/archive/2026-11.idx (permission denied)\nexit code 1",
};

function seeds(): Seed[] {
  const ok3: RunStatus[] = ["ok", "ok", "ok"];
  return [
    {
      id: "run-export-waiting",
      task: "export-notes",
      trigger: "manual",
      status: "waiting",
      ago: 25 * MIN,
      seconds: null,
      states: ["ok", "ok", "waiting", "pending"],
    },
    {
      id: "run-sync-1",
      task: "nightly-sync",
      trigger: "schedule",
      status: "failed",
      ago: 3 * HOUR - 8 * MIN,
      seconds: 41,
      states: ["ok", "failed", "skipped"],
      output: SYNC_OUTPUT,
      error: "Export changed notes ended with exit code 1",
    },
    {
      id: "run-sync-2",
      task: "nightly-sync",
      trigger: "schedule",
      status: "ok",
      ago: 7 * HOUR - 8 * MIN,
      seconds: 38,
      states: ok3,
    },
    {
      id: "run-sync-3",
      task: "nightly-sync",
      trigger: "schedule",
      status: "ok",
      ago: 10 * HOUR + 51 * MIN,
      seconds: 36,
      states: ok3,
    },
    {
      id: "run-sync-4",
      task: "nightly-sync",
      trigger: "catch-up",
      status: "ok",
      ago: 11 * HOUR + 55 * MIN,
      seconds: 39,
      states: ok3,
      missed: {
        slots: 2,
        firstDue: "2026-11-16T13:07:00Z",
        lastDue: "2026-11-16T17:07:00Z",
      },
    },
    {
      id: "run-sync-5",
      task: "nightly-sync",
      trigger: "schedule",
      status: "ok",
      ago: 22 * HOUR + 51 * MIN,
      seconds: 39,
      states: ok3,
    },
    {
      id: "run-sync-6",
      task: "nightly-sync",
      trigger: "cli",
      status: "ok",
      ago: 30 * HOUR,
      seconds: 252,
      states: ok3,
    },
    {
      id: "run-digest-1",
      task: "weekly-digest",
      trigger: "selfMcp",
      status: "ok",
      ago: 25 * HOUR + 46 * MIN,
      seconds: 180,
      states: ["ok"],
    },
    {
      id: "run-beta-1",
      task: "renew-beta-token",
      trigger: "manual",
      status: "ok",
      ago: 4 * 24 * HOUR - 18 * HOUR,
      seconds: 120,
      states: ["ok", "ok"],
    },
  ];
}

function seedMissed(now: number): MissedRecord[] {
  return [
    {
      task: "nightly-sync",
      slots: 1,
      firstDue: iso(now - 30 * HOUR - 5 * MIN),
      lastDue: iso(now - 30 * HOUR - 5 * MIN),
      outcome: "busy",
      at: iso(now - 30 * HOUR - 5 * MIN),
    },
    {
      task: "nightly-sync",
      slots: 2,
      firstDue: "2026-11-16T13:07:00Z",
      lastDue: "2026-11-16T17:07:00Z",
      outcome: "caught-up",
      at: iso(now - 11 * HOUR - 55 * MIN),
      run: "run-sync-4",
    },
    {
      task: "weekly-digest",
      slots: 1,
      firstDue: iso(now - 6 * 24 * HOUR),
      lastDue: iso(now - 6 * 24 * HOUR),
      outcome: "too-old",
      at: iso(now - 5 * 24 * HOUR - 20 * HOUR),
    },
  ];
}

const fields = (text: string) => text.trim().split(/\s+/);

function matches(part: string, value: number, low: number): boolean {
  return part.split(",").some((piece) => {
    const [range, stepText] = piece.split("/");
    const step = stepText ? Number(stepText) : 1;
    let from = low;
    let to = low === 0 && value > 23 ? 59 : Number.POSITIVE_INFINITY;
    if (range !== "*") {
      const [a, b] = range.split("-").map(Number);
      from = a;
      to = b ?? (stepText ? Number.POSITIVE_INFINITY : a);
    }
    return value >= from && value <= to && (value - from) % step === 0;
  });
}

/** The next `count` slots of a five field UTC cron after `from` (none for a cron it cannot read). */
export function nextSlots(cron: string, from: number, count: number): number[] {
  const parts = fields(cron);
  if (parts.length !== 5) return [];
  const slots: number[] = [];
  let at = Math.floor(from / MIN) * MIN + MIN;
  for (let i = 0; i < 14 * 24 * 60 && slots.length < count; i++, at += MIN) {
    const d = new Date(at);
    if (
      matches(parts[0], d.getUTCMinutes(), 0) &&
      matches(parts[1], d.getUTCHours(), 0) &&
      matches(parts[2], d.getUTCDate(), 1) &&
      matches(parts[3], d.getUTCMonth() + 1, 1) &&
      matches(parts[4], d.getUTCDay(), 0)
    )
      slots.push(at);
  }
  return slots;
}

const validCron = (cron: string) =>
  fields(cron).length === 5 && /^[\d*,/\-\s]+$/.test(cron);

function describeStep(step: TaskStep, i: number) {
  let op = "note";
  let detail: string;
  switch (step.type) {
    case "needs-you":
      detail = `pause for you: ${step.title} (until you resume)`;
      break;
    case "mcp":
      detail = `call ${step.server}/${step.tool}`;
      break;
    case "exec":
      op = "exec";
      detail = `run ${step.program}${step.args?.length ? ` ${step.args.join(" ")}` : ""}`;
      break;
    case "prompt":
      detail = `ask Claude: ${step.title}`;
      break;
    case "secret-set":
      op = "update";
      detail = `write the secret ${step.server}/${step.key} from a captured value (never shown)`;
      break;
    case "restart-server":
      op = "exec";
      detail = `restart ${step.server}`;
      break;
    default:
      detail = "run an inline script";
  }
  return { op, detail: `${i + 1}. ${step.title} (${step.type}): ${detail}` };
}

/** The `task` commands of the in-memory toolportctl: enough state for a write to show in the next
 * read, a run that moves on with time and the refusals the golden envelopes show. */
export function createTaskWorld(options: TaskWorldOptions = {}) {
  const scenario = options.scenario ?? "default";
  const stepMs = options.stepMs ?? 2500;
  const base = [
    ...(scenario === "empty" ? [] : defaultTasks()),
    ...(options.updateCheck ? [updateCheckTask(options.updateCheck !== "off")] : []),
  ];
  const defs = new Map<string, TaskDefinition>(base.map((d) => [d.id, d]));
  const routineIds = new Set<string>();
  const problems = new Map<string, TaskProblem[]>();
  const runs: Array<RunView & { real?: number }> = [];
  const t0 = pageNow();
  const resumed = new Set<string>();
  let missed: MissedRecord[] = scenario === "default" ? seedMissed(t0) : [];

  if (scenario !== "empty") {
    for (const seed of seeds()) {
      const def = defs.get(seed.task)!;
      const at = t0 - seed.ago;
      runs.push({
        id: seed.id,
        task: seed.task,
        trigger: seed.trigger,
        status: seed.status,
        startedAt: iso(at),
        endedAt: seed.seconds === null ? null : iso(at + seed.seconds * 1000),
        durationMs: seed.seconds === null ? null : seed.seconds * 1000,
        error: seed.error ?? null,
        steps: stepsOf(def, seed.states, at, seed.output),
        ...(seed.missed ? { missed: seed.missed } : {}),
      });
    }
  }
  if (scenario === "edge") {
    const routine = define(
      "refresh-reports",
      "Refresh the report cache",
      [
        exec("warm", "Warm up", "echo", ["warm"]),
        {
          id: "refresh",
          title: "Run the routine",
          type: "routine",
          script: "",
          routineId: "reports-refresh",
        },
      ],
      { requires: { servers: [], commands: ["echo"] } },
    );
    defs.set(routine.id, routine);
    routineIds.add(routine.id);
    problems.set(routine.id, [
      {
        code: "routine-step",
        step: "refresh",
        index: 1,
        routine: "reports-refresh",
        message:
          "step 2 (Run the routine) names the routine reports-refresh; routines left Toolport 2.3, so replace it with a script step",
      },
    ]);
  }

  function refresh() {
    const real = Date.now();
    for (const run of runs) {
      if (run.status !== "running" || run.real === undefined) continue;
      const def = defs.get(run.task);
      if (!def) continue;
      const done = Math.floor((real - run.real) / stepMs);
      const states: RunStatus[] = def.steps.map((_step, i) =>
        i < done ? "ok" : "pending",
      );
      const waitAt = resumed.has(run.id)
        ? -1
        : def.steps.findIndex((s, i) => s.type === "needs-you" && i <= done);
      if (waitAt >= 0 && done >= waitAt) {
        for (let i = waitAt; i < states.length; i++)
          states[i] = i === waitAt ? "waiting" : "pending";
        run.status = "waiting";
      } else if (done >= def.steps.length) {
        run.status = "ok";
        run.durationMs = def.steps.length * stepMs;
        run.endedAt = iso(pageNow());
      } else {
        states[done] = "running";
      }
      run.steps = stepsOf(def, states, Date.parse(run.startedAt));
    }
  }

  const liveRun = (id: string) =>
    runs.find((r) => r.task === id && (r.status === "running" || r.status === "waiting"));

  const rowOf = (def: TaskDefinition) => {
    const last = runs.find((r) => r.task === def.id);
    const next =
      def.enabled && def.triggers.schedule
        ? nextSlots(def.triggers.schedule.cron, pageNow(), 1)[0]
        : undefined;
    const row = {
      id: def.id,
      title: def.title,
      enabled: def.enabled,
      triggers: def.triggers,
      lastRun: last
        ? {
            runId: last.id,
            status: last.status,
            startedAt: last.startedAt,
            durationMs: last.durationMs,
          }
        : null,
      nextRun: next === undefined ? null : iso(next),
      refreshes: def.triggers.onAuthFailure,
      waiting: runs.some((r) => r.task === def.id && r.status === "waiting"),
      kind:
        def.triggers.onAuthFailure.length || def.writesSecrets.length
          ? "login"
          : "script",
    };
    return problems.has(def.id) ? { ...row, problems: problems.get(def.id) } : row;
  };

  const withoutOutput = (run: RunView): RunView => ({
    ...run,
    steps: run.steps.map((step) => {
      const copy = { ...step };
      delete copy.output;
      return copy;
    }),
  });

  function validate(def: TaskDefinition): string | null {
    if (!/^[a-z0-9][a-z0-9-]{0,63}$/.test(def.id))
      return `id ${JSON.stringify(def.id)} must be 1-64 characters of a-z, 0-9 and '-'`;
    if (!def.title?.trim()) return "title is empty";
    if (!def.steps?.length) return "steps is empty: a task needs at least one step";
    for (const step of def.steps) {
      if ("routineId" in step && step.routineId)
        return `step "${step.id}" refers to a saved routine, which Toolport 2 no longer has; use an inline script, mcp, exec or prompt step`;
    }
    if (def.triggers?.schedule && !validCron(def.triggers.schedule.cron))
      return `schedule.cron ${JSON.stringify(def.triggers.schedule.cron)} is not a five field cron`;
    return null;
  }

  function savePlan(def: TaskDefinition, create: boolean, dryRun: boolean): CtlReply {
    const path = `/Users/example/Library/Application Support/Toolport/plus/tasks/${def.id}.json`;
    const before = defs.get(def.id);
    const warnings: string[] = [];
    if (def.triggers.schedule?.autoRun && !before?.triggers.schedule?.autoRun)
      warnings.push("the schedule runs the task on its own (autoRun)");
    if (def.triggers.selfMcp.enabled && !before?.triggers.selfMcp.enabled)
      warnings.push("an AI client may ask to run it (you approve every run)");
    if (!def.enabled)
      warnings.push("the task is disabled: enable it (enabled: true) before it runs");
    const backup = create
      ? null
      : `${path.replace("plus/tasks", "plus/tasks/.backups")}.bak`;
    const undo = create ? `toolportctl task rm ${def.id}` : `cp ${backup} ${path}`;
    const plan: Plan = {
      summary: `${create ? "Add" : "Edit"} task ${def.id}`,
      steps: [
        {
          op: create ? "create" : "update",
          path,
          detail: create ? `write the new task ${def.id}` : `change task ${def.id}`,
          diff: {
            before: before ? JSON.stringify(before, null, 2) : "",
            after: JSON.stringify(def, null, 2),
          },
        },
      ],
      effects: {},
      warnings,
      undo,
    };
    if (!dryRun) defs.set(def.id, structuredClone(def));
    const data = {
      dryRun,
      task: def.id,
      path,
      created: create,
      plan,
      result: dryRun
        ? null
        : { applied: true, changed: [path], undo, backups: backup ? [backup] : [] },
      ...(def.triggers.schedule
        ? { nextRuns: nextSlots(def.triggers.schedule.cron, pageNow(), 5).map(iso) }
        : {}),
    };
    return { ok: true, data };
  }

  function fromCommand(id: string, path: string, dryRun: boolean): CtlReply {
    if (!path.endsWith(".md"))
      return fail("read", `cannot read ${path}: No such file or directory`);
    const name = path.split("/").pop()!.replace(/\.md$/, "");
    const def = define(
      id,
      name.replace(/-/g, " ").replace(/^./, (c) => c.toUpperCase()),
      [
        {
          id: "ask",
          title: "Ask Claude",
          type: "prompt",
          prompt: `Run the command ${name}`,
          allowedTools: ["Read"],
        },
      ],
      { enabled: false, createdFrom: { kind: "command", path } },
    );
    return savePlan(def, true, dryRun);
  }

  function runPlan(def: TaskDefinition, dryRun: boolean): CtlReply {
    const warnings: string[] = [];
    if (!def.enabled)
      warnings.push(`task "${def.id}" is disabled: enable it first (task edit)`);
    const busy = liveRun(def.id);
    if (busy) warnings.push(`task "${def.id}" is already running as ${busy.id}`);
    for (const server of def.requires.servers)
      if (options.offServers?.includes(server))
        warnings.push(
          `the server ${server} is not installed: run toolportctl server install ${server}`,
        );
    const plan: Plan = {
      summary: `Run task ${def.id} (${def.steps.length} step${def.steps.length === 1 ? "" : "s"})`,
      steps: [
        ...def.steps.map(describeStep),
        ...def.writesSecrets.map((s) => ({
          op: "note",
          detail: `may write the secret ${s.server}/${s.key} (writesSecrets)`,
        })),
        ...def.requires.servers.map((s) => ({
          op: "note",
          detail: `needs the server ${s}`,
        })),
      ],
      effects: {},
      warnings,
      undo: "toolportctl task cancel <run-id>",
    };
    const common = {
      task: def.id,
      plan,
      writesSecrets: def.writesSecrets,
      requires: def.requires,
    };
    if (dryRun)
      return { ok: true, data: { dryRun: true, ...common, run: null, result: null } };
    if (!def.enabled)
      return fail(
        "conflict",
        `task "${def.id}" is disabled: enable it first (task edit)`,
      );
    if (busy)
      return fail("conflict", `task "${def.id}" is already running as ${busy.id}`, {
        task: def.id,
        run: busy.id,
        status: busy.status,
      });
    if (options.refuseStart) {
      options.refuseStart = false;
      const live: RunView & { real?: number } = {
        id: "run-by-schedule",
        task: def.id,
        trigger: "schedule",
        status: "running",
        startedAt: iso(pageNow()),
        endedAt: null,
        durationMs: null,
        error: null,
        steps: stepsOf(def, ["running"], pageNow()),
        real: Date.now(),
      };
      runs.unshift(live);
      return fail("conflict", `task "${def.id}" is already running as ${live.id}`, {
        task: def.id,
        run: live.id,
        status: live.status,
      });
    }
    const run: RunView & { real?: number } = {
      id: `run-${Date.now().toString(36)}`,
      task: def.id,
      trigger: "manual",
      status: "running",
      startedAt: iso(pageNow()),
      endedAt: null,
      durationMs: null,
      error: null,
      steps: stepsOf(def, ["running"], pageNow()),
      real: Date.now(),
    };
    runs.unshift(run);
    const result = {
      applied: true,
      changed: [`/plus/task-runs/${run.id}.json`],
      undo: `toolportctl task cancel ${run.id}`,
      backups: [],
    };
    return {
      ok: true,
      data: { dryRun: false, ...common, run: withoutOutput(run), result },
    };
  }

  function lookup(id: string | undefined): TaskDefinition | CtlReply {
    const def = id ? defs.get(id) : undefined;
    if (!def)
      return fail(
        "not_found",
        `no task ${JSON.stringify(id ?? "")} (looked in /plus/tasks)`,
      );
    return def;
  }
  const isReply = (value: TaskDefinition | CtlReply): value is CtlReply => "ok" in value;

  return {
    defs,
    runs,
    get missed() {
      return missed;
    },
    set missed(next: MissedRecord[]) {
      missed = next;
    },
    run(argv: string[], stdin?: string): CtlReply {
      refresh();
      const [verb, ...rest] = argv;
      const positional = rest.filter((a) => !a.startsWith("--"));
      const flag = (name: string) => {
        const at = rest.indexOf(name);
        return at >= 0 ? rest[at + 1] : undefined;
      };
      const dryRun = rest.includes("--dry-run");
      switch (verb) {
        case "ls": {
          if (options.listError) return fail("no_data_dir", options.listError);
          const all = rest.includes("--all");
          const rows = [...defs.values()].filter((d) => d.enabled || all).map(rowOf);
          const invalid =
            scenario === "edge"
              ? [
                  {
                    id: "tidy-old",
                    error:
                      "/plus/tasks/tidy-old.json: not a valid task definition: unexpected end of input at line 12",
                  },
                ]
              : [];
          const scheduler =
            options.heartbeat === false || scenario === "empty"
              ? {}
              : { scheduler: { at: iso(pageNow() - 5000), by: "app", intervalSecs: 20 } };
          return { ok: true, data: { tasks: rows, invalid, ...scheduler } };
        }
        case "show": {
          const def = lookup(positional[0]);
          if (isReply(def)) return def;
          const shown = runs
            .filter((r) => r.task === def.id)
            .slice(0, 5)
            .map(withoutOutput);
          return {
            ok: true,
            data: {
              task: def,
              runs: shown,
              ...(problems.has(def.id) ? { problems: problems.get(def.id) } : {}),
            },
          };
        }
        case "history": {
          const run = flag("--run");
          if (run) {
            const found = runs.find((r) => r.id === run);
            return found
              ? { ok: true, data: { run: found } }
              : fail("not_found", `no run "${run}"`);
          }
          const limit = Number(flag("--limit") ?? 20);
          const only = positional.find((p) => p !== flag("--limit"));
          if (only && !defs.has(only)) return fail("not_found", `no task "${only}"`);
          return {
            ok: true,
            data: {
              runs: runs
                .filter((r) => !only || r.task === only)
                .slice(0, limit)
                .map(withoutOutput),
            },
          };
        }
        case "missed": {
          const only = positional[0];
          return {
            ok: true,
            data: { missed: missed.filter((m) => !only || m.task === only) },
          };
        }
        case "run": {
          const def = lookup(positional[0]);
          return isReply(def) ? def : runPlan(def, dryRun);
        }
        case "resume": {
          const run = runs.find((r) => r.id === positional[0]);
          if (!run) return fail("not_found", `no run "${positional[0]}"`);
          if (run.status !== "waiting")
            return fail(
              "conflict",
              `run ${run.id} is ${run.status}, not waiting for you`,
            );
          if (options.runnerGone)
            return fail(
              "runner_gone",
              `the process that runs ${run.id} is gone: cancel it and start the task again`,
            );
          const def = defs.get(run.task)!;
          const at = def.steps.findIndex((s) => s.type === "needs-you");
          run.status = "running";
          run.real = Date.now() - (at + 1) * stepMs;
          resumed.add(run.id);
          return {
            ok: true,
            data: {
              run: withoutOutput(run),
              result: {
                applied: true,
                changed: [],
                undo: `toolportctl task cancel ${run.id}`,
                backups: [],
              },
            },
          };
        }
        case "cancel": {
          const run = runs.find((r) => r.id === positional[0]);
          if (!run) return fail("not_found", `no run "${positional[0]}"`);
          if (run.status !== "running" && run.status !== "waiting")
            return fail("conflict", `run ${run.id} already ended`);
          run.status = "cancelled";
          run.endedAt = iso(pageNow());
          run.durationMs = Math.max(1000, pageNow() - Date.parse(run.startedAt));
          run.steps = run.steps.map((s) =>
            s.status === "running" || s.status === "waiting"
              ? { ...s, status: "cancelled" as RunStatus }
              : s,
          );
          const reply: CtlReply = {
            ok: true,
            data: {
              run: withoutOutput(run),
              result: {
                applied: true,
                changed: [],
                undo: `toolportctl task run ${run.task}`,
                backups: [],
              },
            },
          };
          return options.cancelDelayMs
            ? { ...reply, delayMs: options.cancelDelayMs }
            : reply;
        }
        case "add":
        case "edit": {
          const create = verb === "add";
          const id = positional[0];
          if (create && flag("--from-command"))
            return fromCommand(id, flag("--from-command")!, dryRun);
          let def: TaskDefinition;
          try {
            def = JSON.parse(stdin ?? "");
          } catch {
            return fail(
              "invalid_task",
              "nothing on stdin: pipe the task definition in (--file - < task.json)",
            );
          }
          if (def.id !== id)
            return fail(
              "usage",
              `the definition on stdin defines the task "${def.id}", not "${id}"`,
            );
          const invalid = validate(def);
          if (invalid)
            return fail("invalid_task", `not a valid task definition: ${invalid}`);
          if (create && defs.has(id))
            return fail(
              "conflict",
              `task "${id}" already exists: change it with task edit`,
            );
          if (!create && !defs.has(id))
            return fail("not_found", `no task "${id}": create it with task add`);
          return savePlan(def, create, dryRun);
        }
        case "rm": {
          const def = lookup(positional[0]);
          if (isReply(def)) return def;
          const busy = liveRun(def.id);
          if (busy)
            return fail(
              "conflict",
              `task "${def.id}" is running as ${busy.id}: cancel the run first`,
            );
          const path = `/Users/example/Library/Application Support/Toolport/plus/tasks/${def.id}.json`;
          const plan: Plan = {
            summary: `Remove task ${def.id}`,
            steps: [
              {
                op: "delete",
                path,
                detail: `remove the task ${def.id} (its run history stays)`,
                diff: { before: JSON.stringify(def, null, 2), after: "" },
              },
            ],
            effects: {},
            warnings: [],
            undo: "restore the task file from the backup the apply prints",
          };
          if (!dryRun) defs.delete(def.id);
          return {
            ok: true,
            data: {
              dryRun,
              task: def.id,
              plan,
              result: dryRun
                ? null
                : {
                    applied: true,
                    changed: [path],
                    undo: `cp ${path}.bak ${path}`,
                    backups: [`${path}.bak`],
                  },
            },
          };
        }
        case "enable":
        case "disable": {
          const def = lookup(positional[0]);
          if (isReply(def)) return def;
          const enabled = verb === "enable";
          const changes = def.enabled !== enabled;
          if (!dryRun && changes) def.enabled = enabled;
          const word = enabled ? "on" : "off";
          const undo = `toolportctl task ${enabled ? "disable" : "enable"} ${def.id}`;
          const path = `/plus/tasks/${def.id}.json`;
          return {
            ok: true,
            data: {
              dryRun,
              task: def.id,
              enabled,
              path,
              plan: {
                summary: changes
                  ? `Turn task ${def.id} ${word}`
                  : `Task ${def.id} is already ${word}`,
                steps: changes
                  ? [{ op: "update", path, detail: `turn task ${def.id} ${word}` }]
                  : [],
                effects: {},
                warnings: [],
                undo,
              },
              result: dryRun
                ? null
                : { applied: changes, changed: changes ? [path] : [], undo, backups: [] },
            },
          };
        }
        default:
          throw new Error(
            `Unimplemented fixture command: plus_ctl task ${argv.join(" ")}`,
          );
      }
    },
  };
}

export type TaskWorld = ReturnType<typeof createTaskWorld>;
