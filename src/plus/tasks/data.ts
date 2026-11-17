/** The `data` of the toolportctl `task` envelopes the Tasks screen reads. The Rust goldens under
 * plus/fixtures/ctl-envelopes are the source; data.test.ts parses every task golden against these
 * shapes. */

export type RunTrigger =
  "manual" | "cli" | "selfMcp" | "schedule" | "catch-up" | "onAuthFailure";

export type RunStatus =
  "pending" | "running" | "waiting" | "ok" | "failed" | "cancelled" | "skipped";

export interface StepRunView {
  id: string;
  title: string;
  type: string;
  status: RunStatus;
  startedAt: string | null;
  endedAt: string | null;
  /** Only the step instructions of a step that waits for the person. */
  instructions?: string;
  /** Redacted log; only in the view of one run (`history --run`). */
  output?: string;
}

/** What a catch-up run covers: the scheduled slots that came due while nothing ticked. */
export interface MissedNote {
  slots: number;
  firstDue: string;
  lastDue: string;
}

export interface RunView {
  id: string;
  task: string;
  trigger: RunTrigger;
  status: RunStatus;
  startedAt: string;
  endedAt: string | null;
  durationMs: number | null;
  error: string | null;
  steps: StepRunView[];
  action?: { label: string; command: string[] };
  missed?: MissedNote;
}

export interface WaitFor {
  kind: "secret-unset" | "url" | "manual";
  timeoutSec: number;
}

export type TaskStep =
  | {
      id: string;
      title: string;
      type: "needs-you";
      instructions: string;
      waitFor?: WaitFor;
    }
  | {
      id: string;
      title: string;
      type: "mcp";
      server: string;
      tool: string;
      args?: unknown;
      capture?: string[];
    }
  | {
      id: string;
      title: string;
      type: "script" | "routine";
      script?: string;
      args?: unknown;
      capture?: string[];
      /** A 1.x step that names a saved routine, which Toolport 2.3 no longer has. */
      routineId?: string;
    }
  | { id: string; title: string; type: "exec"; program: string; args?: string[] }
  | {
      id: string;
      title: string;
      type: "prompt";
      prompt: string;
      allowedTools?: string[];
      model?: string;
    }
  | {
      id: string;
      title: string;
      type: "secret-set";
      server: string;
      key: string;
      from: string;
    }
  | { id: string; title: string; type: "restart-server"; server: string };

export interface TaskTriggers {
  manual: boolean;
  cli: boolean;
  selfMcp: { enabled: boolean; approval: "every-run" };
  schedule: { cron: string; autoRun: boolean } | null;
  onAuthFailure: string[];
}

export interface TaskDefinition {
  id: string;
  title: string;
  description: string;
  enabled: boolean;
  requires: { servers: string[]; commands: string[] };
  writesSecrets: Array<{ server: string; key: string }>;
  steps: TaskStep[];
  triggers: TaskTriggers;
  createdFrom: { kind: "command" | "script" | "manual"; path?: string } | null;
}

/** A 1.x step that cannot run any more; `index` is the step's position. */
export interface TaskProblem {
  code: "routine-step";
  step: string;
  index: number;
  routine: string;
  message: string;
}

export interface TaskRow {
  id: string;
  title: string;
  enabled: boolean;
  triggers: TaskTriggers;
  lastRun: {
    runId: string;
    status: RunStatus;
    startedAt: string;
    durationMs: number | null;
  } | null;
  nextRun: string | null;
  refreshes: string[];
  waiting: boolean;
  kind: "login" | "script";
  problems?: TaskProblem[];
}

export interface Heartbeat {
  at: string;
  by: "app" | "gateway";
  intervalSecs: number;
}

export interface TaskListData {
  tasks: TaskRow[];
  invalid: Array<{ id: string; error: string }>;
  /** Absent until a process has looked at the schedule. */
  scheduler?: Heartbeat;
}

export interface TaskShowData {
  task: TaskDefinition;
  runs: RunView[];
  problems?: TaskProblem[];
}

export interface TaskHistoryData {
  runs: RunView[];
}

export interface TaskRunLogData {
  run: RunView;
}

export type MissedOutcome = "caught-up" | "too-old" | "busy";

export interface MissedRecord {
  task: string;
  slots: number;
  firstDue: string;
  lastDue: string;
  outcome: MissedOutcome;
  at: string;
  run?: string;
}

export interface TaskMissedData {
  missed: MissedRecord[];
}

export interface PlanStep {
  op: string;
  detail: string;
  path?: string;
  diff?: { before: string; after: string };
}

export interface Plan {
  summary: string;
  steps: PlanStep[];
  effects: Record<string, unknown>;
  warnings: string[];
  undo: string;
}

export interface ApplyResult {
  applied: boolean;
  changed: string[];
  undo: string;
  backups: string[];
}

export interface RunPlanData {
  dryRun: boolean;
  task: string;
  plan: Plan;
  writesSecrets: Array<{ server: string; key: string }>;
  requires: { servers: string[]; commands: string[] };
  run: RunView | null;
  result: ApplyResult | null;
}

export interface RunStateData {
  run: RunView;
  result: ApplyResult;
}

/** `add`, `edit` and (without a task body) `enable` and `disable` answer in this shape. */
export interface SavePlanData {
  dryRun: boolean;
  task: string;
  path: string;
  created: boolean;
  plan: Plan;
  result: ApplyResult | null;
  /** The next five slots of the schedule in UTC; absent without a schedule. */
  nextRuns?: string[];
}

export interface EnableData {
  dryRun: boolean;
  task: string;
  enabled: boolean;
  path: string;
  plan: Plan;
  result: ApplyResult | null;
}

export interface RemoveData {
  dryRun: boolean;
  task: string;
  plan: Plan;
  result: ApplyResult | null;
}

/** The data a refused second start carries. */
export interface BusyData {
  task: string;
  run: string;
  status: RunStatus;
}
