import {
  any,
  arr,
  bool,
  lit,
  nullable,
  num,
  obj,
  opt,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";
import { planV1, resultV1 } from "../bridge/data";

/** `data` of the `task` commands (contract section 8, MIG-AUTO-1), checked against the golden
 * envelopes by `data.test.ts`. The `tasks_*` tools answer with the same objects. Run views carry
 * no secret and no internal flag: a step `output` is already redacted when it is written. */

const stepType = lit(
  "needs-you",
  "mcp",
  "routine",
  "exec",
  "prompt",
  "secret-set",
  "restart-server",
);

/** One step as stored: the fields beyond `id`, `title` and `type` depend on the type. */
export const taskStep = obj({
  id: str,
  title: str,
  type: stepType,
  instructions: opt(str),
  waitFor: opt(obj({ kind: lit("secret-unset", "url", "manual"), timeoutSec: num })),
  server: opt(str),
  tool: opt(str),
  args: opt(any),
  capture: opt(arr(str)),
  routineId: opt(str),
  script: opt(str),
  program: opt(str),
  prompt: opt(str),
  allowedTools: opt(arr(str)),
  model: opt(str),
  key: opt(str),
  from: opt(str),
});

export const taskTriggers = obj({
  manual: bool,
  cli: bool,
  selfMcp: obj({ enabled: bool, approval: lit("every-run") }),
  schedule: nullable(obj({ cron: str, autoRun: bool })),
  onAuthFailure: arr(str),
});

const secretRef = obj({ server: str, key: str });

export const taskDefinition = obj({
  id: str,
  title: str,
  description: str,
  enabled: bool,
  requires: obj({ servers: arr(str), commands: arr(str) }),
  writesSecrets: arr(secretRef),
  steps: arr(taskStep),
  triggers: taskTriggers,
  createdFrom: nullable(
    obj({ kind: lit("command", "script", "manual"), path: opt(str) }),
  ),
});
export type TaskDefinition = Infer<typeof taskDefinition>;

const stepStatus = lit(
  "pending",
  "running",
  "waiting",
  "ok",
  "failed",
  "cancelled",
  "skipped",
);
export const runStatus = lit("running", "waiting", "ok", "failed", "cancelled");
export type TaskRunStatus = Infer<typeof runStatus>;

export const taskRun = obj({
  id: str,
  task: str,
  trigger: lit("manual", "cli", "selfMcp", "schedule", "onAuthFailure"),
  status: runStatus,
  startedAt: str,
  endedAt: nullable(str),
  durationMs: nullable(num),
  error: nullable(str),
  steps: arr(
    obj({
      id: str,
      title: str,
      type: stepType,
      status: stepStatus,
      startedAt: nullable(str),
      endedAt: nullable(str),
      instructions: opt(str),
      /** Only in `task history --run`, the one view that carries step logs. */
      output: opt(str),
    }),
  ),
});
export type TaskRun = Infer<typeof taskRun>;

export const taskLsData = obj({
  tasks: arr(
    obj({
      id: str,
      title: str,
      enabled: bool,
      triggers: taskTriggers,
      lastRun: nullable(
        obj({
          runId: str,
          status: runStatus,
          startedAt: str,
          durationMs: nullable(num),
        }),
      ),
      nextRun: nullable(str),
      waiting: bool,
    }),
  ),
  /** Task files that do not parse, with the reason. */
  invalid: arr(obj({ id: str, error: str })),
});
export type TaskLsData = Infer<typeof taskLsData>;

export const taskShowData = obj({
  task: taskDefinition,
  runs: arr(taskRun),
});
export type TaskShowData = Infer<typeof taskShowData>;

export const taskRunData = obj({
  dryRun: bool,
  task: str,
  plan: planV1,
  writesSecrets: arr(secretRef),
  requires: obj({ servers: arr(str), commands: arr(str) }),
  run: nullable(taskRun),
  result: nullable(resultV1),
});
export type TaskRunData = Infer<typeof taskRunData>;

/** `task resume` and `task cancel`. */
export const taskRunStateData = obj({ run: taskRun, result: resultV1 });
export type TaskRunStateData = Infer<typeof taskRunStateData>;

/** `task add` and `task edit`. */
export const taskSaveData = obj({
  dryRun: bool,
  task: str,
  path: str,
  created: bool,
  plan: planV1,
  result: nullable(resultV1),
});
export type TaskSaveData = Infer<typeof taskSaveData>;

export const taskRmData = obj({
  dryRun: bool,
  task: str,
  plan: planV1,
  result: nullable(resultV1),
});
export type TaskRmData = Infer<typeof taskRmData>;

export const taskHistoryData = obj({ runs: arr(taskRun) });
export type TaskHistoryData = Infer<typeof taskHistoryData>;

export const taskHistoryRunData = obj({ run: taskRun });
export type TaskHistoryRunData = Infer<typeof taskHistoryRunData>;

export const taskShapes: Record<string, Shape<unknown>> = {
  "task-add.apply": taskSaveData,
  "task-add.command": taskSaveData,
  "task-add.command-preview": taskSaveData,
  "task-add.preview": taskSaveData,
  "task-cancel.apply": taskRunStateData,
  "task-edit.apply": taskSaveData,
  "task-edit.preview": taskSaveData,
  "task-history.all": taskHistoryData,
  "task-history.limit": taskHistoryData,
  "task-history.run": taskHistoryRunData,
  "task-history.task": taskHistoryData,
  "task-ls.all": taskLsData,
  "task-ls.default": taskLsData,
  "task-resume.apply": taskRunStateData,
  "task-rm.apply": taskRmData,
  "task-rm.preview": taskRmData,
  "task-run.apply": taskRunData,
  "task-run.disabled": taskRunData,
  "task-run.preview": taskRunData,
  "task-show.scheduled": taskShowData,
  "task-show.waiting": taskShowData,
};
