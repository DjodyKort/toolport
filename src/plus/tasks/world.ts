import { CtlReplyFailure } from "../fixtures/ctlReply";
import type { SourceItem } from "../bridge/data";
import type { PlanV1 } from "../ui";
import type { TaskDefinition, TaskLsData, TaskRun } from "../types/tasks";
import { commandsData, STAMP, stockRuns, stockTasks, runLog } from "./fixtures";
import { stepText } from "./model";

/** A made-up secret the fake runner "captures". It lives in this module only: no reply, plan
 * or log of the world ever holds it, and the tests look for it on the page. */
export const CANARY = "canary-token-0123456789-do-not-show";

export interface TasksState {
  tasks: TaskDefinition[];
  runs: TaskRun[];
  /** Step logs by run id and step id, as the redactor leaves them. */
  logs: Record<string, Record<string, string>>;
  commandFiles: Array<{ name: string; path: string }>;
  /** `server/KEY` of the secrets the tasks have written. */
  written: string[];
  seq: number;
}

const fail = (code: string, message: string) => new CtlReplyFailure(code, message);
const TASK_DIR = "/data/plus/tasks";

function initial(): TasksState {
  return {
    tasks: structuredClone(stockTasks),
    runs: stockRuns(),
    logs: { "run-fixture-ok": { say: runLog().steps[0].output ?? "report" } },
    commandFiles: [
      { name: "refresh-login", path: "/home/demo/.claude/commands/refresh-login.md" },
      { name: "fix-pr", path: "/home/demo/.claude/commands/fix-pr.md" },
    ],
    written: [],
    seq: 0,
  };
}

const tick = (state: TasksState) =>
  new Date(Date.parse(STAMP) + ++state.seq * 1_000).toISOString();

function plan(summary: string, steps: PlanV1["steps"], warnings: string[], undo: string) {
  return { summary, steps, effects: {}, warnings, undo };
}

function runPlan(task: TaskDefinition, state: TasksState): PlanV1 {
  const steps: PlanV1["steps"] = task.steps.map((step, i) => ({
    op:
      step.type === "secret-set"
        ? "update"
        : step.type === "restart-server" || step.type === "exec"
          ? "exec"
          : "note",
    detail: `${i + 1}. ${step.title} (${step.type}): ${stepText(step)}`,
  }));
  for (const s of task.writesSecrets)
    steps.push({
      op: "note",
      detail: `may write the secret ${s.server}/${s.key} (writesSecrets)`,
    });
  for (const server of task.requires.servers)
    steps.push({ op: "note", detail: `needs the server ${server}` });
  const warnings: string[] = [];
  if (!task.enabled)
    warnings.push(`task "${task.id}" is disabled: enable it first (task edit)`);
  const busy = activeRun(state, task.id);
  if (busy) warnings.push(`task "${task.id}" is already running as ${busy.id}`);
  const n = task.steps.length;
  return plan(
    `Run task ${task.id} (${n} ${n === 1 ? "step" : "steps"})`,
    steps,
    warnings,
    "toolportctl task cancel <run-id>",
  );
}

const activeRun = (state: TasksState, id: string) =>
  state.runs.find(
    (r) => r.task === id && (r.status === "running" || r.status === "waiting"),
  );

function lsRow(state: TasksState, task: TaskDefinition): TaskLsData["tasks"][number] {
  const last = state.runs.find((r) => r.task === task.id) ?? null;
  return {
    id: task.id,
    title: task.title,
    enabled: task.enabled,
    triggers: task.triggers,
    lastRun: last && {
      runId: last.id,
      status: last.status,
      startedAt: last.startedAt,
      durationMs: last.durationMs,
    },
    nextRun: task.triggers.schedule && task.enabled ? "2026-10-05T03:00:00Z" : null,
    waiting: last?.status === "waiting",
  };
}

function validate(task: TaskDefinition): CtlReplyFailure | null {
  for (const step of task.steps) {
    if (
      step.type === "secret-set" &&
      !task.writesSecrets.some((s) => s.server === step.server && s.key === step.key)
    )
      return fail(
        "invalid_task",
        `task "${task.id}" is not valid:\n  - ${step.server}/${step.key} is not listed in writesSecrets`,
      );
  }
  return null;
}

function finish(
  run: TaskRun,
  status: TaskRun["status"],
  error: string | null,
  at: string,
) {
  run.status = status;
  run.endedAt = at;
  run.durationMs = Math.max(0, Date.parse(at) - Date.parse(run.startedAt));
  run.error = error;
}

/** One poll of an active run moves its runner one beat: a pending step starts (or, when it
 * needs you, makes the run wait), a running step ends. An `exec` of `false` fails. */
function advance(state: TasksState, run: TaskRun) {
  if (run.status !== "running") return;
  const task = state.tasks.find((t) => t.id === run.task);
  const i = run.steps.findIndex((s) => s.status !== "ok" && s.status !== "skipped");
  if (i < 0) return;
  const step = run.steps[i];
  const at = tick(state);
  if (step.status === "pending") {
    step.startedAt = at;
    if (step.type === "needs-you") {
      step.status = "waiting";
      run.status = "waiting";
    } else step.status = "running";
    return;
  }
  const def = task?.steps[i];
  step.endedAt = at;
  if (def?.type === "exec" && def.program === "false") {
    step.status = "failed";
    for (const rest of run.steps.slice(i + 1)) rest.status = "skipped";
    state.logs[run.id] = { ...state.logs[run.id], [step.id]: "exit status 1" };
    finish(run, "failed", `step ${i + 1} (${step.title}) failed: exit status 1`, at);
    return;
  }
  step.status = "ok";
  const log =
    def?.type === "mcp"
      ? `captured ${def.capture?.join(", ") ?? "result"} ([redacted])`
      : def?.type === "secret-set"
        ? `wrote ${def.server}/${def.key} from a captured value`
        : def?.type === "exec"
          ? (Array.isArray(def.args) ? def.args : []).join(" ")
          : "";
  if (log) state.logs[run.id] = { ...state.logs[run.id], [step.id]: log };
  if (def?.type === "secret-set") state.written.push(`${def.server}/${def.key}`);
  if (i === run.steps.length - 1) finish(run, "ok", null, at);
}

const view = (run: TaskRun, logs: TasksState["logs"], withLog: boolean): TaskRun => ({
  ...run,
  steps: run.steps.map((s) => {
    const rest = { ...s };
    delete rest.output;
    const output = withLog ? logs[run.id]?.[s.id] : undefined;
    return output === undefined ? rest : { ...rest, output };
  }),
});

function commandItems(state: TasksState): SourceItem[] {
  return state.commandFiles.map((file) => ({
    kind: "command",
    name: file.name,
    path: file.path,
    sourceId: "user",
    origin: { kind: "user", name: "user" },
    writable: true,
    lazy: false,
    shadowedBy: null,
    audit: "clean",
    tokens: { value: 120, basis: "estimate" },
  }));
}

/** A stateful stand-in for the `task` commands: an applied write changes the next read, and
 * a run advances through its steps as it is read, stops at a step that needs you until
 * `task resume`, and ends as `ok`, `failed` or `cancelled`. Everything it answers has the
 * shapes of the real goldens. */
export function createTasksWorld(seed: Partial<TasksState> = {}) {
  const state: TasksState = { ...initial(), ...structuredClone(seed) };
  const find = (id: string) => state.tasks.find((t) => t.id === id);
  const run = (id: string) => state.runs.find((r) => r.id === id);

  function save(
    argv: string[],
    stdin: string | null,
    dry: boolean,
    verb: "add" | "edit",
  ) {
    const id = argv[2];
    const from = argv[argv.indexOf("--from-command") + 1];
    let task: TaskDefinition;
    if (argv.includes("--from-command")) {
      task = {
        id,
        title: `Run ${from.split("/").pop()?.replace(/\.md$/, "")}`,
        description: "",
        enabled: false,
        requires: { servers: [], commands: [] },
        writesSecrets: [],
        steps: [
          { id: "sign-in", title: "Sign in", type: "needs-you", instructions: "Sign in" },
          { id: "run-command", title: "Run the command", type: "prompt", prompt: from },
        ],
        triggers: {
          manual: true,
          cli: false,
          selfMcp: { enabled: false, approval: "every-run" },
          schedule: null,
          onAuthFailure: [],
        },
        createdFrom: { kind: "command", path: from },
      };
    } else {
      if (stdin === null) return fail("read", "cannot read /dev/stdin");
      try {
        task = JSON.parse(stdin) as TaskDefinition;
      } catch {
        return fail("invalid_task", "not a valid task definition");
      }
      if (task.id !== id)
        return fail("usage", `the file defines the task ${task.id}, not ${id}`);
    }
    const exists = find(id);
    if (verb === "add" && exists)
      return fail("conflict", `task "${id}" already exists: change it with task edit`);
    if (verb === "edit" && !exists) return fail("not_found", `no task "${id}"`);
    const bad = validate(task);
    if (bad) return bad;
    const path = `${TASK_DIR}/${id}.json`;
    const text = `${JSON.stringify(task, null, 2)}\n`;
    const warnings = [
      ...(task.enabled
        ? []
        : ["the task is disabled: enable it (enabled: true) before it runs"]),
      ...task.writesSecrets
        .filter(
          (s) =>
            !exists?.writesSecrets.some((o) => o.server === s.server && o.key === s.key),
        )
        .map((s) => `the task may write the secret ${s.server}/${s.key} when it runs`),
    ];
    const created = verb === "add";
    const steps = [
      {
        op: created ? ("create" as const) : ("update" as const),
        path,
        detail: created ? `write the new task ${id}` : `change task ${id}`,
        diff: {
          before: exists ? `${JSON.stringify(exists, null, 2)}\n` : "",
          after: text,
        },
      },
    ];
    const undo = created
      ? `toolportctl task rm ${id}`
      : `restore ${path} from the backup`;
    const body = plan(`${created ? "Add" : "Change"} task ${id}`, steps, warnings, undo);
    if (dry) return { dryRun: true, task: id, path, created, plan: body, result: null };
    if (exists) state.tasks.splice(state.tasks.indexOf(exists), 1, task);
    else state.tasks.push(task);
    return {
      dryRun: false,
      task: id,
      path,
      created,
      plan: body,
      result: { applied: true, changed: [path], undo, backups: [] },
    };
  }

  function reply(argv: string[], stdin: string | null = null): unknown {
    const dry = argv.includes("--dry-run");
    const [cmd, verb] = argv;
    const id = argv[2]?.startsWith("--") ? "" : (argv[2] ?? "");
    if (cmd === "commands") return commandsData;
    if (cmd === "sources" && verb === "ls")
      return {
        generatedAt: STAMP,
        partial: false,
        skipped: [],
        sources: [],
        items: commandItems(state),
      };
    if (cmd !== "task") return undefined;
    switch (verb) {
      case "ls":
        return {
          tasks: state.tasks
            .filter((t) => t.enabled || argv.includes("--all"))
            .map((t) => lsRow(state, t)),
          invalid: argv.includes("--all")
            ? [
                {
                  id: "broken",
                  error: "/data/plus/tasks/broken.json: not a valid task definition",
                },
              ]
            : [],
        };
      case "show": {
        const task = find(id);
        if (!task) return fail("not_found", `no task "${id}"`);
        return {
          task,
          runs: state.runs
            .filter((r) => r.task === id)
            .slice(0, 5)
            .map((r) => view(r, state.logs, false)),
        };
      }
      case "history": {
        const which = argv[argv.indexOf("--run") + 1];
        if (argv.includes("--run")) {
          const found = run(which);
          if (!found) return fail("not_found", `no run "${which}"`);
          advance(state, found);
          return { run: view(found, state.logs, true) };
        }
        const limit = argv.includes("--limit")
          ? Number(argv[argv.indexOf("--limit") + 1])
          : 20;
        return {
          runs: state.runs
            .filter((r) => !id || r.task === id)
            .slice(0, limit)
            .map((r) => view(r, state.logs, false)),
        };
      }
      case "run": {
        const task = find(id);
        if (!task) return fail("not_found", `no task "${id}"`);
        const body = runPlan(task, state);
        const common = {
          task: id,
          plan: body,
          writesSecrets: task.writesSecrets,
          requires: task.requires,
        };
        if (dry) return { dryRun: true, ...common, run: null, result: null };
        if (!task.enabled)
          return fail(
            "conflict",
            `task "${id}" is disabled: enable it first (task edit)`,
          );
        const busy = activeRun(state, id);
        if (busy)
          return fail("conflict", `task "${id}" is already running as ${busy.id}`);
        const record: TaskRun = {
          id: `run-${String(state.runs.length + 1).padStart(3, "0")}`,
          task: id,
          trigger: "manual",
          status: "running",
          startedAt: tick(state),
          endedAt: null,
          durationMs: null,
          error: null,
          steps: task.steps.map((s) => ({
            id: s.id,
            title: s.title,
            type: s.type,
            status: "pending" as const,
            startedAt: null,
            endedAt: null,
            ...(s.type === "needs-you" ? { instructions: s.instructions } : {}),
          })),
        };
        state.runs.unshift(record);
        const undo = `toolportctl task cancel ${record.id}`;
        return {
          dryRun: false,
          ...common,
          run: view(record, state.logs, false),
          result: {
            applied: true,
            changed: [`/data/plus/task-runs/${record.id}.json`],
            undo,
            backups: [],
          },
        };
      }
      case "resume": {
        const found = run(id);
        if (!found) return fail("not_found", `no run "${id}"`);
        if (found.status !== "waiting")
          return fail("conflict", `run ${id} is ${found.status}, not waiting for you`);
        const step = found.steps.find((s) => s.status === "waiting");
        if (step) {
          step.status = "ok";
          step.endedAt = tick(state);
        }
        found.status = found.steps.every((s) => s.status === "ok") ? "ok" : "running";
        if (found.status === "ok") finish(found, "ok", null, tick(state));
        return {
          run: view(found, state.logs, false),
          result: {
            applied: true,
            changed: [`/data/plus/task-runs/${id}.json`],
            undo: `toolportctl task cancel ${id}`,
            backups: [],
          },
        };
      }
      case "cancel": {
        const found = run(id);
        if (!found) return fail("not_found", `no run "${id}"`);
        if (found.status !== "running" && found.status !== "waiting")
          return fail("conflict", `run ${id} already ended`);
        for (const s of found.steps) if (s.status !== "ok") s.status = "cancelled";
        finish(found, "cancelled", "cancelled", tick(state));
        return {
          run: view(found, state.logs, false),
          result: {
            applied: true,
            changed: [`/data/plus/task-runs/${id}.json`],
            undo: `toolportctl task run ${found.task}`,
            backups: [],
          },
        };
      }
      case "add":
      case "edit":
        return save(argv, stdin, dry, verb);
      case "rm": {
        const task = find(id);
        if (!task) return fail("not_found", `no task "${id}"`);
        const busy = activeRun(state, id);
        if (busy)
          return fail(
            "conflict",
            `task "${id}" is running as ${busy.id}: cancel the run first`,
          );
        const path = `${TASK_DIR}/${id}.json`;
        const body = plan(
          `Remove task ${id}`,
          [
            {
              op: "delete",
              path,
              detail: `remove the task ${id} (its run history stays)`,
              diff: { before: `${JSON.stringify(task, null, 2)}\n`, after: "" },
            },
          ],
          [],
          "restore the task file from the backup the apply prints",
        );
        if (dry) return { dryRun: true, task: id, plan: body, result: null };
        state.tasks.splice(state.tasks.indexOf(task), 1);
        const undo = `cp /data/backups/${id}.json ${path}`;
        return {
          dryRun: false,
          task: id,
          plan: { ...body, undo },
          result: {
            applied: true,
            changed: [path],
            undo,
            backups: [`/data/backups/${id}.json`],
          },
        };
      }
    }
    return undefined;
  }

  return { state, reply };
}

export type TasksWorld = ReturnType<typeof createTasksWorld>;
