import type { TaskDefinition, TaskLsData, TaskRun, TaskRunStatus } from "../types/tasks";

export type Tone = "success" | "warning" | "destructive" | "secondary" | "info";
export type LsTask = TaskLsData["tasks"][number];
export type Step = TaskDefinition["steps"][number];
export type StepType = Step["type"];
export type Triggers = TaskDefinition["triggers"];

export const ID_PATTERN = /^[a-z0-9][a-z0-9-]{0,63}$/;
/** The task file goes to the child's stdin (it holds no secret value, only names). */
export const STDIN_FILE = "-";

const STAMP = new Intl.DateTimeFormat("en", {
  month: "short",
  day: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});

export function formatWhen(iso: string | null | undefined): string {
  if (!iso) return "never";
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : STAMP.format(date);
}

export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return "—";
  if (ms < 1000) return `${ms} ms`;
  const seconds = ms / 1000;
  if (seconds < 10) return `${seconds.toFixed(1)} s`;
  if (seconds < 60) return `${Math.round(seconds)} s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes} min ${Math.round(seconds - minutes * 60)} s`;
}

const WEEKDAYS = ["Sundays", "Mondays", "Tuesdays", "Wednesdays", "Thursdays", "Fridays"];

/** A cron expression in words when it is one of the simple daily or weekly shapes; the
 * expression itself otherwise. */
export function cronText(cron: string): string {
  const parts = cron.trim().split(/\s+/);
  if (parts.length !== 5) return cron;
  const [minute, hour, dom, month, dow] = parts;
  if (!/^\d+$/.test(minute) || !/^\d+$/.test(hour) || dom !== "*" || month !== "*")
    return cron;
  const time = `${hour.padStart(2, "0")}:${minute.padStart(2, "0")}`;
  if (dow === "*") return `Every day at ${time}`;
  if (/^[0-6]$/.test(dow)) return `${WEEKDAYS[Number(dow)] ?? "Saturdays"} at ${time}`;
  return cron;
}

export const RUN_STATUS: Record<TaskRunStatus, { label: string; tone: Tone }> = {
  running: { label: "Running", tone: "info" },
  waiting: { label: "Needs you", tone: "warning" },
  ok: { label: "ok", tone: "success" },
  failed: { label: "Failed", tone: "destructive" },
  cancelled: { label: "Cancelled", tone: "secondary" },
};

export const STEP_STATUS: Record<string, string> = {
  pending: "Waiting to start",
  running: "Running",
  waiting: "Waiting for you",
  ok: "Done",
  failed: "Failed",
  cancelled: "Cancelled",
  skipped: "Skipped",
};

export const TRIGGER_LABEL: Record<TaskRun["trigger"], string> = {
  manual: "button",
  cli: "CLI",
  selfMcp: "Claude",
  schedule: "schedule",
  onAuthFailure: "login failed",
};

export const STEP_TYPES: Array<{ id: StepType; label: string }> = [
  { id: "needs-you", label: "Needs you" },
  { id: "mcp", label: "Call a server tool" },
  { id: "routine", label: "Routine or script" },
  { id: "exec", label: "Run a program" },
  { id: "prompt", label: "Ask Claude" },
  { id: "secret-set", label: "Store a secret" },
  { id: "restart-server", label: "Restart a server" },
];

export function stepKindLabel(type: StepType): string {
  return type === "needs-you" ? "Needs you" : "Auto";
}

/** What a step does in one line. A secret step names the key and never a value. */
export function stepText(step: Step): string {
  switch (step.type) {
    case "needs-you":
      return step.instructions || step.title;
    case "mcp":
      return `Call ${step.server ?? "?"}/${step.tool ?? "?"}`;
    case "routine":
      return step.routineId ? `Run the routine ${step.routineId}` : "Run a script";
    case "exec":
      return `Run ${[step.program, ...(Array.isArray(step.args) ? step.args : [])].join(" ")}`;
    case "prompt":
      return "Ask Claude (uses a part of your plan)";
    case "secret-set":
      return `Write the secret ${step.server ?? "?"}/${step.key ?? "?"} from a captured value`;
    case "restart-server":
      return `Restart ${step.server ?? "?"}`;
  }
}

export function needsYou(task: TaskDefinition): boolean {
  return task.steps.some((step) => step.type === "needs-you");
}

export function taskState(task: LsTask): { label: string; tone: Tone } {
  if (task.waiting) return { label: "Needs you", tone: "warning" };
  if (!task.enabled) return { label: "Off", tone: "secondary" };
  const last = task.lastRun?.status;
  if (last === "running") return { label: "Running", tone: "info" };
  if (last === "failed") return { label: "Last run failed", tone: "destructive" };
  if (task.triggers.schedule) return { label: "Scheduled", tone: "success" };
  return { label: "Manual", tone: "secondary" };
}

export interface Chip {
  key: string;
  label: string;
}

export function triggerChips(triggers: Triggers): Chip[] {
  const chips: Chip[] = [{ key: "manual", label: "Button" }];
  if (triggers.cli) chips.push({ key: "cli", label: "CLI" });
  if (triggers.selfMcp.enabled) chips.push({ key: "selfMcp", label: "Claude may ask" });
  if (triggers.schedule)
    chips.push({
      key: "schedule",
      label: `${cronText(triggers.schedule.cron)}${triggers.schedule.autoRun ? "" : " (asks you)"}`,
    });
  for (const server of triggers.onAuthFailure)
    chips.push({ key: `auth:${server}`, label: `Login fails: ${server}` });
  return chips;
}

export function summarize(tasks: LsTask[]) {
  const failed = tasks.find((task) => task.enabled && task.lastRun?.status === "failed");
  return {
    total: tasks.length,
    enabled: tasks.filter((task) => task.enabled).length,
    waiting: tasks.filter((task) => task.waiting),
    scheduled: tasks.filter((task) => task.enabled && task.triggers.schedule),
    failed: failed ?? null,
  };
}

/** The enabled task that renews the login of a server: the one that starts when that
 * server's login fails. */
export function refreshTaskFor(tasks: LsTask[] | null, server: string): LsTask | null {
  return tasks?.find((task) => task.enabled && task.refreshes.includes(server)) ?? null;
}

export function slugFromPath(path: string): string {
  const base = path.split(/[\\/]/).pop() ?? "";
  return base
    .replace(/\.[^.]+$/, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);
}

const TASK_LIKE = /token|login|log-in|sign-?in|refresh|auth|credential|renew/i;

export function looksLikeTask(name: string): boolean {
  return TASK_LIKE.test(name);
}

export interface StepForm {
  id: string;
  title: string;
  type: StepType;
  instructions: string;
  waitKind: "" | "secret-unset" | "url" | "manual";
  timeoutSec: string;
  server: string;
  tool: string;
  args: string;
  capture: string;
  routineId: string;
  script: string;
  program: string;
  argv: string;
  prompt: string;
  allowedTools: string;
  model: string;
  key: string;
  from: string;
}

export interface TaskForm {
  id: string;
  title: string;
  description: string;
  enabled: boolean;
  servers: string;
  commands: string;
  secrets: string;
  cli: boolean;
  selfMcp: boolean;
  schedule: boolean;
  cron: string;
  autoRun: boolean;
  onAuthFailure: string;
  steps: StepForm[];
  createdFrom: TaskDefinition["createdFrom"];
}

export const blankStep = (type: StepType = "needs-you"): StepForm => ({
  id: "",
  title: "",
  type,
  instructions: "",
  waitKind: "",
  timeoutSec: "",
  server: "",
  tool: "",
  args: "",
  capture: "",
  routineId: "",
  script: "",
  program: "",
  argv: "",
  prompt: "",
  allowedTools: "",
  model: "",
  key: "",
  from: "",
});

export const blankForm = (): TaskForm => ({
  id: "",
  title: "",
  description: "",
  enabled: false,
  servers: "",
  commands: "",
  secrets: "",
  cli: false,
  selfMcp: false,
  schedule: false,
  cron: "0 8 * * *",
  autoRun: false,
  onAuthFailure: "",
  steps: [blankStep()],
  createdFrom: { kind: "manual" },
});

const list = (text: string) => text.split(/[\s,]+/).filter(Boolean);
const joined = (items: string[] | undefined) => (items ?? []).join(", ");

export function formFromTask(task: TaskDefinition): TaskForm {
  return {
    id: task.id,
    title: task.title,
    description: task.description,
    enabled: task.enabled,
    servers: joined(task.requires.servers),
    commands: joined(task.requires.commands),
    secrets: task.writesSecrets.map((s) => `${s.server}/${s.key}`).join("\n"),
    cli: task.triggers.cli,
    selfMcp: task.triggers.selfMcp.enabled,
    schedule: task.triggers.schedule !== null,
    cron: task.triggers.schedule?.cron ?? "0 8 * * *",
    autoRun: task.triggers.schedule?.autoRun ?? false,
    onAuthFailure: joined(task.triggers.onAuthFailure),
    createdFrom: task.createdFrom,
    steps: task.steps.map((step) => ({
      ...blankStep(step.type),
      id: step.id,
      title: step.title,
      instructions: step.instructions ?? "",
      waitKind: step.waitFor?.kind ?? "",
      timeoutSec: step.waitFor ? String(step.waitFor.timeoutSec) : "",
      server: step.server ?? "",
      tool: step.tool ?? "",
      args:
        step.args === undefined || step.type === "exec" ? "" : JSON.stringify(step.args),
      capture: joined(step.capture),
      routineId: step.routineId ?? "",
      script: step.script ?? "",
      program: step.program ?? "",
      argv: Array.isArray(step.args) ? step.args.join("\n") : "",
      prompt: step.prompt ?? "",
      allowedTools: joined(step.allowedTools),
      model: step.model ?? "",
      key: step.key ?? "",
      from: step.from ?? "",
    })),
  };
}

function stepFromForm(form: StepForm, errors: string[], at: number): Step {
  const base = { id: form.id.trim(), title: form.title.trim() };
  const where = `Step ${at + 1}`;
  if (!base.id) errors.push(`${where} needs an id`);
  if (!base.title) errors.push(`${where} needs a title`);
  const need = (value: string, what: string) => {
    if (!value.trim()) errors.push(`${where} needs ${what}`);
    return value.trim();
  };
  const optional = <T>(key: string, value: T | "" | undefined) =>
    value === "" || value === undefined ? {} : { [key]: value };
  switch (form.type) {
    case "needs-you":
      return {
        ...base,
        type: "needs-you",
        instructions: need(form.instructions, "instructions"),
        ...(form.waitKind
          ? {
              waitFor: {
                kind: form.waitKind,
                timeoutSec: Number.parseInt(form.timeoutSec, 10) || 900,
              },
            }
          : {}),
      };
    case "mcp": {
      let args: unknown = {};
      if (form.args.trim()) {
        try {
          args = JSON.parse(form.args);
        } catch {
          errors.push(`${where}: the arguments are not valid JSON`);
        }
      }
      return {
        ...base,
        type: "mcp",
        server: need(form.server, "a server"),
        tool: need(form.tool, "a tool"),
        args,
        ...(list(form.capture).length ? { capture: list(form.capture) } : {}),
      };
    }
    case "routine":
      return {
        ...base,
        type: "routine",
        ...optional("routineId", form.routineId.trim()),
        ...optional("script", form.script.trim()),
        ...(list(form.capture).length ? { capture: list(form.capture) } : {}),
      };
    case "exec":
      return {
        ...base,
        type: "exec",
        program: need(form.program, "a program"),
        args: form.argv.split(/\r?\n/).filter((line) => line !== ""),
      };
    case "prompt":
      return {
        ...base,
        type: "prompt",
        prompt: need(form.prompt, "a prompt"),
        allowedTools: list(form.allowedTools),
        ...optional("model", form.model.trim()),
      };
    case "secret-set":
      return {
        ...base,
        type: "secret-set",
        server: need(form.server, "a server"),
        key: need(form.key, "a key"),
        from: need(form.from, "the captured variable"),
      };
    case "restart-server":
      return { ...base, type: "restart-server", server: need(form.server, "a server") };
  }
}

export type Built =
  { task: TaskDefinition; errors: [] } | { task: null; errors: string[] };

/** The definition the form describes, or what is wrong with it. The server checks the rest. */
export function taskFromForm(form: TaskForm): Built {
  const errors: string[] = [];
  const id = form.id.trim();
  if (!ID_PATTERN.test(id))
    errors.push(
      "The id uses 1 to 64 characters of a-z, 0-9 and -, starting with a letter or digit",
    );
  if (!form.title.trim()) errors.push("The task needs a title");
  if (form.steps.length === 0) errors.push("The task needs at least one step");
  const writesSecrets: Array<{ server: string; key: string }> = [];
  for (const line of form.secrets.split(/\r?\n/).filter((l) => l.trim())) {
    const [server, ...rest] = line.trim().split("/");
    const key = rest.join("/");
    if (!server || !key) errors.push(`"${line.trim()}" is not a secret as server/KEY`);
    else writesSecrets.push({ server, key });
  }
  const steps = form.steps.map((step, i) => stepFromForm(step, errors, i));
  const ids = steps.map((s) => s.id).filter(Boolean);
  if (new Set(ids).size !== ids.length) errors.push("Step ids must be different");
  if (form.schedule && form.cron.trim().split(/\s+/).length !== 5)
    errors.push("The schedule is a cron expression of five fields");
  if (errors.length > 0) return { task: null, errors };
  return {
    errors: [],
    task: {
      id,
      title: form.title.trim(),
      description: form.description.trim(),
      enabled: form.enabled,
      requires: { servers: list(form.servers), commands: list(form.commands) },
      writesSecrets,
      steps,
      triggers: {
        manual: true,
        cli: form.cli,
        selfMcp: { enabled: form.selfMcp, approval: "every-run" },
        schedule: form.schedule
          ? { cron: form.cron.trim(), autoRun: form.autoRun }
          : null,
        onAuthFailure: list(form.onAuthFailure),
      },
      createdFrom: form.createdFrom,
    },
  };
}

export function saveArgv(verb: "add" | "edit", id: string): string[] {
  return ["task", verb, id, "--file", STDIN_FILE];
}

export function isActive(run: Pick<TaskRun, "status">): boolean {
  return run.status === "running" || run.status === "waiting";
}
