import { useEffect, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  CircleCheck,
  Clock,
  Loader2,
  MoreHorizontal,
  Pause,
  Play,
  Square,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { StatusCard } from "@/components/ui/status-card";
import { Switch } from "@/components/ui/switch";
import { toastError } from "@/lib/toast";
import { CtlError } from "../bridge/ctl";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import {
  AlreadyRunning,
  cancelRun,
  removeTask,
  resumeRun,
  runTask,
  setEnabled,
  showTask,
  previewRun,
} from "./commands";
import type { RunPlanData, RemoveData, RunView, TaskDefinition, TaskStep } from "./data";
import {
  clockTime,
  describeCron,
  duration,
  elapsed,
  inWords,
  kindLabel,
  lastRunLine,
  ms,
  plural,
  scheduleText,
  stamp,
  startedBy,
  stepOf,
  timeline,
} from "./model";
import { RunLog, RunRow, SkippedRow } from "./RunRows";
import { liveRunOf, type TasksState } from "./useTasks";
import { Chip, Group, Mono, Quiet } from "./ui";
import { useNow } from "./useNow";

export interface TaskDetailProps {
  id: string;
  state: TasksState;
  onBack: () => void;
  onEdit: (id: string, fix?: boolean) => void;
  onDuplicate: (definition: TaskDefinition) => void;
  onOpenSettings: () => void;
  onShowHistory: (id: string) => void;
  /** Opens the plan before a run as soon as the page opens (Run now on a list row). */
  planFirst?: boolean;
}

function describeStep(step: TaskStep): string {
  switch (step.type) {
    case "exec":
      return `Run ${[step.program, ...(step.args ?? [])].join(" ")}`;
    case "mcp":
      return `Call ${step.server}/${step.tool}`;
    case "needs-you":
      return step.instructions;
    case "prompt":
      return `Ask Claude: ${step.prompt}`;
    case "secret-set":
      return `Store the secret ${step.server}/${step.key} from a kept result`;
    case "restart-server":
      return `Restart ${step.server}`;
    default:
      return step.routineId
        ? `Names the routine ${step.routineId}. Routines were removed in Toolport 2.3, so this step cannot run.`
        : "Run an inline script (needs Code Mode)";
  }
}

export default function TaskDetail({
  id,
  state,
  onBack,
  onEdit,
  onDuplicate,
  onOpenSettings,
  onShowHistory,
  planFirst = false,
}: TaskDetailProps) {
  const now = useNow(1000);
  const shown = useLoad(() => showTask(id), `${id}:${state.updatedAt}`);
  const [plan, setPlan] = useState<
    | { kind: "loading" }
    | { kind: "ready"; data: RunPlanData }
    | { kind: "error"; message: string }
    | null
  >(null);
  const [starting, setStarting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [goneText, setGone] = useState<string | null>(null);
  const [menu, setMenu] = useState(false);
  const [removal, setRemoval] = useState<
    { kind: "ready"; data: RemoveData } | { kind: "error"; message: string } | null
  >(null);
  const [typed, setTyped] = useState("");
  const [log, setLog] = useState<string | null>(null);

  const task = shown.data?.task;
  const problems = shown.data?.problems ?? [];
  const row = state.list?.tasks.find((t) => t.id === id);
  const live = liveRunOf(state.runs, id);
  const gone = live ? goneText : null;
  const mine = state.runs.filter((r) => r.task === id);
  const last = mine[0];
  const missed = state.missed.filter((m) => m.task === id);
  const items = timeline(mine, missed, true);
  const enabled = task?.enabled ?? row?.enabled ?? true;
  const cannotRun = problems.length > 0;
  const nextRun = ms(row?.nextRun);

  useEffect(() => {
    if (planFirst) void openPlan();
    // Once, when the page opens from a list row.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function openPlan() {
    setPlan({ kind: "loading" });
    try {
      setPlan({ kind: "ready", data: await previewRun(id) });
    } catch (error) {
      setPlan({ kind: "error", message: errorText(error) });
    }
  }

  async function start() {
    setStarting(true);
    try {
      await runTask(id);
      setPlan(null);
    } catch (error) {
      if (error instanceof AlreadyRunning) {
        toast.message(`${task?.title ?? id} is already running`, {
          description: `Showing the live run ${error.busy.run}.`,
        });
        setPlan(null);
      } else
        toastError(`${task?.title ?? id} did not start`, {
          description: errorText(error),
        });
    } finally {
      setStarting(false);
      state.reload();
    }
  }

  async function stop(run: RunView) {
    setStopping(true);
    try {
      await cancelRun(run.id);
      setGone(null);
    } catch (error) {
      toastError("The run could not be stopped", { description: errorText(error) });
    } finally {
      setStopping(false);
      state.reload();
    }
  }

  async function resume(run: RunView) {
    try {
      await resumeRun(run.id);
      setGone(null);
      state.reload();
    } catch (error) {
      if (error instanceof CtlError && error.code === "runner_gone")
        setGone(error.message);
      else toastError("The run could not continue", { description: errorText(error) });
    }
  }

  async function toggle(next: boolean) {
    try {
      await setEnabled(id, next);
      toast.message(`${task?.title ?? id} is ${next ? "on" : "off"}`, {
        action: {
          label: "Undo",
          onClick: () =>
            void setEnabled(id, !next)
              .catch((e: unknown) =>
                toastError("Undo failed", { description: errorText(e) }),
              )
              .finally(state.reload),
        },
      });
    } catch (error) {
      toastError(`${task?.title ?? id} could not be turned ${next ? "on" : "off"}`, {
        description: errorText(error),
      });
    } finally {
      state.reload();
    }
  }

  async function openRemoval() {
    setMenu(false);
    setTyped("");
    try {
      setRemoval({ kind: "ready", data: await removeTask(id, true) });
    } catch (error) {
      setRemoval({ kind: "error", message: errorText(error) });
    }
  }

  async function remove() {
    try {
      await removeTask(id, false);
      toast.success(`Deleted ${task?.title ?? id}`);
      state.reload();
      onBack();
    } catch (error) {
      setRemoval({ kind: "error", message: errorText(error) });
    }
  }

  if (shown.error && !task)
    return (
      <div className="grid gap-4">
        <Button variant="ghost" size="sm" className="w-fit" onClick={onBack}>
          <ArrowLeft /> Tasks
        </Button>
        <StatusCard
          variant="danger"
          icon={<AlertTriangle />}
          title="The task could not be read"
          detail={shown.error}
          actions={
            <Button size="sm" variant="outline" onClick={shown.reload}>
              Retry
            </Button>
          }
        />
      </div>
    );
  if (!task) return <Quiet role="status">Reading the task…</Quiet>;

  const liveStep = live ? stepOf(live) : null;
  const waiting = live?.status === "waiting";
  const started = live ? (ms(live.startedAt) ?? now) : 0;
  const seconds = live ? Math.max(0, Math.floor((now - started) / 1000)) : 0;
  const autoRuns = task.enabled && task.triggers.schedule?.autoRun === true && !cannotRun;
  const stepWord = (i: number) => {
    const s = live?.steps[i];
    if (!s) return "";
    if (s.status === "ok")
      return `Done in ${duration((ms(s.endedAt) ?? 0) - (ms(s.startedAt) ?? 0))}`;
    if (s.status === "running")
      return `Running for ${Math.max(1, Math.floor((now - (ms(s.startedAt) ?? now)) / 1000))} s`;
    if (s.status === "waiting") return "Waits until you press Continue";
    return `Waits for step ${i}`;
  };

  return (
    <div className="grid gap-4">
      <Button variant="ghost" size="sm" className="w-fit" onClick={onBack}>
        <ArrowLeft /> Tasks
      </Button>

      <div className="flex flex-wrap items-start gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-lg font-semibold break-words">{task.title}</h2>
          <Mono>{task.id}</Mono>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {live ? (
            <>
              {waiting ? (
                <Button size="sm" onClick={() => void resume(live)}>
                  <Play /> Continue
                </Button>
              ) : (
                <Button size="sm" variant="outline" disabled>
                  <Loader2 className="animate-spin" /> Running {elapsed(seconds)}
                </Button>
              )}
              <Button
                size="sm"
                variant="outline"
                disabled={stopping}
                onClick={() => void stop(live)}
              >
                <Square /> {stopping ? "Stopping…" : "Stop run"}
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              disabled={!enabled || cannotRun || starting || plan !== null}
              title={
                cannotRun
                  ? "Cannot run: replace the routine step first."
                  : !enabled
                    ? "Turn the task on to run it."
                    : undefined
              }
              onClick={() => void openPlan()}
            >
              {starting ? (
                <>
                  <Loader2 className="animate-spin" /> Starting…
                </>
              ) : (
                <>
                  <Play /> Run now
                </>
              )}
            </Button>
          )}
          <Button
            size="sm"
            variant="outline"
            disabled={Boolean(live)}
            title={live ? "Stop the run before editing." : undefined}
            onClick={() => onEdit(id)}
          >
            Edit
          </Button>
          <div className="relative">
            <Button
              size="icon"
              variant="outline"
              aria-label="More"
              aria-haspopup="menu"
              aria-expanded={menu}
              onClick={() => setMenu((value) => !value)}
            >
              <MoreHorizontal />
            </Button>
            {menu && (
              <div
                role="menu"
                className="absolute right-0 z-10 mt-1 grid min-w-44 gap-0.5 rounded-lg border bg-popover p-1 shadow-md"
              >
                <Button
                  role="menuitem"
                  variant="ghost"
                  size="sm"
                  className="justify-start"
                  onClick={() => {
                    setMenu(false);
                    onDuplicate(task);
                  }}
                >
                  Duplicate
                </Button>
                <Button
                  role="menuitem"
                  variant="ghost"
                  size="sm"
                  className="justify-start text-destructive"
                  disabled={Boolean(live)}
                  title={live ? `It is running. Stop the run first.` : undefined}
                  onClick={() => void openRemoval()}
                >
                  Delete task
                </Button>
              </div>
            )}
          </div>
        </div>
      </div>
      {!enabled && !live && (
        <Quiet>Run now is off because the task is off. Turn the task on to run it.</Quiet>
      )}

      {plan && (
        <PlanCard
          plan={plan}
          starting={starting}
          onRun={() => void start()}
          onCancel={() => setPlan(null)}
        />
      )}

      {removal && (
        <section
          aria-label="Delete task"
          className="grid gap-3 rounded-xl border bg-card p-4"
        >
          {removal.kind === "error" ? (
            <>
              <p className="text-sm font-semibold">Cannot delete {task.title}</p>
              <Quiet role="alert">{removal.message}</Quiet>
              <Button
                size="sm"
                variant="outline"
                className="w-fit"
                onClick={() => setRemoval(null)}
              >
                Close
              </Button>
            </>
          ) : (
            <>
              <p className="text-sm font-semibold">Delete {task.title}?</p>
              {removal.data.plan.steps.map((s) => (
                <Quiet key={s.detail}>
                  {s.detail}
                  {s.path ? (
                    <>
                      {" "}
                      · <Mono>{s.path}</Mono>
                    </>
                  ) : null}
                </Quiet>
              ))}
              <Quiet>Backs up the file first. Run history stays.</Quiet>
              <label className="grid gap-1.5 text-sm">
                Type <Mono>{task.id}</Mono> to confirm
                <Input value={typed} onChange={(e) => setTyped(e.target.value)} />
              </label>
              <div className="flex gap-2">
                <Button
                  size="sm"
                  variant="destructive"
                  data-confirm={typed === task.id ? "" : undefined}
                  disabled={typed !== task.id}
                  onClick={() => void remove()}
                >
                  Delete task
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setRemoval(null)}>
                  Cancel
                </Button>
              </div>
            </>
          )}
        </section>
      )}

      {gone && (
        <StatusCard
          variant="danger"
          role="alert"
          icon={<AlertTriangle />}
          title="The run cannot continue"
          detail={gone}
          actions={
            live && (
              <Button size="sm" variant="outline" onClick={() => void stop(live)}>
                Stop run
              </Button>
            )
          }
        />
      )}

      <StatusBlock
        task={task}
        problems={problems}
        live={live}
        last={last}
        previous={mine[1]}
        liveStep={liveStep}
        seconds={seconds}
        stopping={stopping}
        now={now}
        nextRun={nextRun}
        enabled={enabled}
        onToggle={(next) => void toggle(next)}
        onFix={() => onEdit(id, true)}
        rowLast={row ? lastRunLine(row, last, now) : ""}
      />

      {autoRuns && state.autostart !== "on" && state.autostart !== "loading" && (
        <StatusCard
          variant="warning"
          icon={<Clock />}
          title={
            state.autostart === "off"
              ? "Launch at login is off"
              : "Launch at login is unknown"
          }
          detail={
            state.autostart === "off"
              ? `This task starts on its own only while Toolport is open. If it is closed at ${nextRun === null ? "the next slot" : clockTime(nextRun)}, one catch-up run starts when it opens (up to 24 hours later).`
              : "Toolport could not read the OS setting, so it cannot say whether this task starts on its own."
          }
          actions={
            <>
              {state.autostart === "unknown" && (
                <Button size="sm" variant="outline" onClick={state.retryAutostart}>
                  Retry
                </Button>
              )}
              <Button size="sm" variant="outline" onClick={onOpenSettings}>
                Open Settings
              </Button>
            </>
          }
        />
      )}

      <section aria-label="How it starts" className="grid gap-2">
        <SectionTitle>How it starts</SectionTitle>
        <Group>
          <TriggerRow title="Button in the app" on always>
            Run now, from this screen. The run is recorded as Manual.
          </TriggerRow>
          <TriggerRow title="From the terminal" on={task.triggers.cli}>
            <Mono>{`toolportctl task run ${task.id}`}</Mono>
          </TriggerRow>
          <TriggerRow title="Claude may ask" on={task.triggers.selfMcp.enabled}>
            Through Toolport&apos;s own MCP. You approve every run.
            {task.triggers.selfMcp.enabled ? "" : ` Off for this task.`}
          </TriggerRow>
          <TriggerRow title="On a schedule" on={task.triggers.schedule !== null}>
            {task.triggers.schedule ? (
              <>
                {scheduleText(task.triggers.schedule.cron, nextRun)}. Cron{" "}
                <Mono>{task.triggers.schedule.cron}</Mono> is read in UTC.{" "}
                {task.triggers.schedule.autoRun
                  ? "Runs without asking."
                  : "Asks you first."}
              </>
            ) : (
              "Not set."
            )}
          </TriggerRow>
          <TriggerRow
            title="When a login fails"
            on={task.triggers.onAuthFailure.length > 0}
          >
            {task.triggers.onAuthFailure.length > 0
              ? `${task.triggers.onAuthFailure.join(", ")}. Saved, but nothing starts this yet: it waits for the login check in Attention.`
              : "Not set."}
          </TriggerRow>
        </Group>
        <Quiet>Change how it starts with Edit.</Quiet>
      </section>

      <section aria-label="What it does" className="grid gap-2">
        <SectionTitle count={task.steps.length}>What it does</SectionTitle>
        <Group>
          {task.steps.map((step, i) => {
            const broken =
              step.type === "routine" && "routineId" in step && step.routineId;
            return (
              <div key={step.id} className="flex gap-3 px-4 py-3">
                <span className="w-5 shrink-0 text-meta text-muted-foreground tabular-nums">
                  {i + 1}
                </span>
                <div className="min-w-0 flex-1 text-sm">
                  <p className="flex flex-wrap items-center gap-2">
                    <Chip
                      tone={
                        step.type === "needs-you" ? "warning" : broken ? "danger" : "off"
                      }
                    >
                      {step.type === "needs-you"
                        ? "Needs you"
                        : broken
                          ? "Cannot run"
                          : "Auto"}
                    </Chip>
                    <span className="font-medium">{step.title}</span>
                    <span className="text-meta text-muted-foreground">
                      {kindLabel(step.type)}
                    </span>
                    {live && (
                      <span className="text-meta text-muted-foreground">
                        {stepWord(i)}
                      </span>
                    )}
                  </p>
                  <Quiet>{describeStep(step)}</Quiet>
                  {broken && (
                    <Button
                      size="xs"
                      variant="outline"
                      className="mt-1"
                      onClick={() => onEdit(id, true)}
                    >
                      Replace with a script
                    </Button>
                  )}
                </div>
              </div>
            );
          })}
        </Group>
        <Quiet>
          Steps run in order. A step marked Needs you pauses the run until you continue; a
          task with one never starts by schedule on its own.
        </Quiet>
      </section>

      <section aria-label="Runs" className="grid gap-2">
        <div className="flex items-center gap-3">
          <SectionTitle count={mine.length}>Runs</SectionTitle>
          <Button
            size="xs"
            variant="ghost"
            className="ml-auto"
            onClick={() => onShowHistory(id)}
          >
            All history
          </Button>
        </div>
        {items.length === 0 ? (
          <Quiet>No run yet. Run now starts the first one.</Quiet>
        ) : (
          <Group>
            {items
              .slice(0, 12)
              .map((item) =>
                item.kind === "run" ? (
                  <RunRow
                    key={item.run.id}
                    run={item.run}
                    now={now}
                    onOpen={() => setLog(item.run.id)}
                    onRunAgain={() => void openPlan()}
                  />
                ) : (
                  <SkippedRow
                    key={`${item.record.at}-${item.record.outcome}`}
                    record={item.record}
                    now={now}
                    onRunNow={() => void openPlan()}
                  />
                ),
              )}
          </Group>
        )}
        {log && (
          <RunLog
            id={log}
            close={() => setLog(null)}
            onRunAgain={() => void openPlan()}
          />
        )}
        <Quiet>
          Newest first. Dimmed rows are slots that did not start a run; the ledger keeps
          the last 50. Logs hide secret values.
        </Quiet>
      </section>
    </div>
  );
}

function SectionTitle({ children, count }: { children: string; count?: number }) {
  return (
    <h3 className="text-xs font-semibold tracking-[0.05em] text-muted-foreground uppercase">
      {children}
      {count !== undefined && (
        <span className="ml-2 font-normal tabular-nums">{count}</span>
      )}
    </h3>
  );
}

function TriggerRow({
  title,
  on,
  always,
  children,
}: {
  title: string;
  on: boolean;
  always?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-start gap-3 px-4 py-3">
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium">{title}</p>
        <Quiet>{children}</Quiet>
      </div>
      <Chip tone={on ? "success" : "off"}>{always ? "Always" : on ? "On" : "Off"}</Chip>
    </div>
  );
}

interface StatusBlockProps {
  task: TaskDefinition;
  problems: Array<{ message: string }>;
  live: RunView | undefined;
  last: RunView | undefined;
  previous: RunView | undefined;
  liveStep: number | null;
  seconds: number;
  stopping: boolean;
  now: number;
  nextRun: number | null;
  enabled: boolean;
  onToggle: (next: boolean) => void;
  onFix: () => void;
  rowLast: string;
}

function StatusBlock(p: StatusBlockProps) {
  const { task, live, last } = p;
  let card: React.ReactNode;
  if (p.problems.length > 0) {
    card = (
      <StatusCard
        variant="danger"
        icon={<AlertTriangle />}
        title="Cannot run"
        detail={p.problems.map((x) => x.message).join(" ")}
        actions={
          <Button size="sm" variant="outline" onClick={p.onFix}>
            Fix
          </Button>
        }
      />
    );
  } else if (live) {
    const waiting = live.status === "waiting";
    const since = ms(live.startedAt) ?? p.now;
    card = (
      <StatusCard
        variant={waiting ? "warning" : "info"}
        icon={waiting ? <Pause /> : <Loader2 className="animate-spin" />}
        title={
          p.stopping
            ? "Stopping…"
            : waiting
              ? `Waiting for you: step ${p.liveStep} of ${live.steps.length}`
              : `Running: step ${p.liveStep ?? 1} of ${live.steps.length}`
        }
        detail={
          <>
            Started {clockTime(since)} by {startedBy(live.trigger)} ·{" "}
            {waiting
              ? `waiting ${inWords(p.now - since)} · no time limit`
              : elapsed(p.seconds)}
            . Run <Mono>{live.id}</Mono>
          </>
        }
      />
    );
  } else if (last?.status === "failed") {
    card = (
      <StatusCard
        variant="danger"
        icon={<X />}
        title="Last run failed"
        detail={
          <>
            {p.rowLast.replace(/^Last run failed /, "")}.
            {p.previous && p.previous.status === "ok"
              ? ` The run before it (${clockTime(ms(p.previous.startedAt) ?? p.now)}) was ok.`
              : ""}
          </>
        }
      />
    );
  } else if (last) {
    card = (
      <StatusCard
        variant={last.status === "ok" ? "success" : "info"}
        icon={last.status === "ok" ? <CircleCheck /> : <Square />}
        title={
          last.status === "ok"
            ? "Last run ok"
            : last.status === "cancelled"
              ? "Cancelled"
              : "Not running"
        }
        detail={
          last.status === "cancelled"
            ? `Stopped by you on ${stamp(ms(last.startedAt) ?? p.now, p.now)}. Step ${last.steps.findIndex((s) => s.status === "cancelled") + 1 || "?"} did not finish.`
            : `${p.rowLast.replace(/^Last run ok /, "")}.`
        }
      />
    );
  } else {
    card = (
      <StatusCard
        icon={<Clock />}
        title="Not running"
        detail="No run yet. The task has never run in Toolport."
      />
    );
  }
  const written = task.writesSecrets.map((s) => `${s.server}/${s.key}`);
  return (
    <section aria-label="Status" className="grid gap-3">
      {card}
      <Group>
        <div className="flex items-center gap-3 px-4 py-3">
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium">On</p>
            <Quiet>A task that is off cannot run, by any trigger.</Quiet>
          </div>
          <Switch
            aria-label={`${task.title} is ${p.enabled ? "on" : "off"}`}
            checked={p.enabled}
            onCheckedChange={p.onToggle}
          />
        </div>
        <Line label="Next run">
          {p.nextRun === null ? (
            "None"
          ) : (
            <>
              {stamp(p.nextRun, p.now)}, in {inWords(p.nextRun - p.now)} (
              {new Date(p.nextRun).toISOString().slice(11, 16)} UTC)
            </>
          )}
        </Line>
        {task.triggers.schedule && (
          <Line label="Schedule">
            {describeCron(task.triggers.schedule.cron, p.nextRun) ?? "Custom"}{" "}
            <Mono>{task.triggers.schedule.cron}</Mono> in UTC
          </Line>
        )}
        <Line label="Needs">
          {[...task.requires.commands, ...task.requires.servers].join(", ") || "Nothing"}
        </Line>
        <Line label="May write">{written.join(", ") || "No secrets"}</Line>
        <Line label="Made from">
          {task.createdFrom?.kind === "command"
            ? `A Claude command (${task.createdFrom.path ?? ""})`
            : "Written by hand"}
        </Line>
      </Group>
    </section>
  );
}

function Line({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-wrap gap-x-4 gap-y-0.5 px-4 py-2.5 text-sm">
      <span className="w-28 shrink-0 text-muted-foreground">{label}</span>
      <span className="min-w-0 flex-1 break-words">{children}</span>
    </div>
  );
}

function PlanCard({
  plan,
  starting,
  onRun,
  onCancel,
}: {
  plan:
    | { kind: "loading" }
    | { kind: "ready"; data: RunPlanData }
    | { kind: "error"; message: string };
  starting: boolean;
  onRun: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    const close = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    document.addEventListener("keydown", close);
    return () => document.removeEventListener("keydown", close);
  }, [onCancel]);
  if (plan.kind === "loading") return <Quiet role="status">Preparing the plan…</Quiet>;
  if (plan.kind === "error")
    return (
      <StatusCard
        variant="danger"
        role="alert"
        icon={<AlertTriangle />}
        title="The plan could not be made"
        detail={plan.message}
        actions={
          <Button size="sm" variant="outline" onClick={onCancel}>
            Close
          </Button>
        }
      />
    );
  const { data } = plan;
  const steps = data.plan.steps.filter((s) => /^\d+\./.test(s.detail));
  const pauses = steps.filter((s) => s.detail.includes("pause for you"));
  const own = steps.length - pauses.length;
  const warnings = data.plan.warnings.filter((w) => !w.includes("is already running"));
  return (
    <section
      aria-label="Plan before the run"
      className="grid gap-3 rounded-xl border bg-card p-4"
      tabIndex={-1}
      ref={(node) => node?.focus()}
    >
      <p className="text-sm font-semibold">
        {data.plan.summary.replace(/^Run task (\S+)/, "Run $1 now?")}
      </p>
      <Quiet>
        {plural(steps.length, "step")}: {own} run on their own
        {pauses.length > 0
          ? `, ${pauses.length} pause${pauses.length === 1 ? "s" : ""} for you`
          : ""}
      </Quiet>
      <Quiet>
        {data.writesSecrets.length === 0
          ? "Writes no secrets"
          : `May write ${data.writesSecrets.map((s) => `${s.server}/${s.key}`).join(", ")}`}
      </Quiet>
      {(data.requires.commands.length > 0 || data.requires.servers.length > 0) && (
        <Quiet>
          Needs {[...data.requires.commands, ...data.requires.servers].join(", ")}
        </Quiet>
      )}
      <ol className="grid gap-1 text-sm">
        {data.plan.steps.map((s) => (
          <li key={s.detail} className="break-words text-muted-foreground">
            {s.detail}
          </li>
        ))}
      </ol>
      {warnings.map((w) => (
        <p key={w} role="alert" className="flex gap-2 text-sm text-warning">
          <AlertTriangle className="mt-0.5 size-4 shrink-0" /> {w}
        </p>
      ))}
      <Quiet>Recorded as Manual. Stop it any time from this page.</Quiet>
      <div className="flex gap-2">
        <Button size="sm" disabled={starting} onClick={onRun}>
          {warnings.length > 0 ? "Run anyway" : "Run now"}
        </Button>
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
      </div>
      <Quiet>
        Preview of <Mono>{`task run ${data.task} --dry-run`}</Mono>. Nothing has started.
      </Quiet>
    </section>
  );
}
