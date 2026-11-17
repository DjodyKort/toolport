import { useState } from "react";
import { AlertTriangle, ChevronRight, Clock, ListTodo, Play } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusCard } from "@/components/ui/status-card";
import { Switch } from "@/components/ui/switch";
import { toastError } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { CtlError } from "../bridge/ctl";
import { errorText } from "../settings/useCtl";
import { resumeRun, setEnabled } from "./commands";
import type { RunView, TaskRow } from "./data";
import {
  clockTime,
  dayLabel,
  inWords,
  lastRunLine,
  ms,
  rowState,
  selfStarting,
  sortRows,
  stamp,
  stepOf,
  timeline,
  triggerParts,
  plural,
  type RowState,
} from "./model";
import { RunRow, SkippedRow } from "./RunRows";
import { SELECT_CLASS } from "./classes";
import { Chip, Group, Mono, Quiet } from "./ui";
import { useNow } from "./useNow";
import type { TasksState } from "./useTasks";

export interface ListActions {
  open: (id: string) => void;
  runNow: (id: string) => void;
  openSettings: () => void;
  newTask: () => void;
  fromCommand: () => void;
}

/** The banner rows above the list: a run that waits, and launch at login. */
export function Banners({ state, actions }: { state: TasksState; actions: ListActions }) {
  const now = useNow(30_000);
  const [gone, setGone] = useState<Record<string, string>>({});
  const waiting = state.runs.filter((r) => r.status === "waiting");
  const tasks = state.list?.tasks ?? [];
  const startsAlone = selfStarting(tasks);
  async function resume(run: RunView) {
    try {
      await resumeRun(run.id);
      state.reload();
    } catch (error) {
      if (error instanceof CtlError && error.code === "runner_gone")
        setGone((current) => ({ ...current, [run.id]: error.message }));
      else toastError("The run could not continue", { description: errorText(error) });
    }
  }
  return (
    <div className="grid gap-2">
      {waiting.map((run) => {
        const title = tasks.find((t) => t.id === run.task)?.title ?? run.task;
        const step = stepOf(run);
        const at = ms(run.startedAt) ?? now;
        return (
          <StatusCard
            key={run.id}
            variant="warning"
            icon={<Clock />}
            title={`${title} is waiting for you`}
            detail={
              gone[run.id] ?? (
                <>
                  Stopped at step {step} of {run.steps.length}:{" "}
                  {(run.steps[(step ?? 1) - 1]?.title ?? "").toLowerCase()}.{" "}
                  {inWords(now - at)} ago.
                </>
              )
            }
            actions={
              <>
                {!gone[run.id] && (
                  <Button size="sm" onClick={() => void resume(run)}>
                    Continue
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => actions.open(run.task)}
                >
                  Open
                </Button>
              </>
            }
          />
        );
      })}
      {startsAlone.length > 0 && state.autostart === "off" && (
        <StatusCard
          variant="warning"
          icon={<Clock />}
          title="Launch at login is off"
          detail={`${plural(startsAlone.length, "scheduled task")} start${startsAlone.length === 1 ? "s" : ""} on its own, but only while Toolport is open. Runs missed while it is closed are caught up when it starts (once, within 24 hours).`}
          actions={
            <Button size="sm" variant="outline" onClick={actions.openSettings}>
              Open Settings
            </Button>
          }
        />
      )}
      {startsAlone.length > 0 && state.autostart === "unknown" && (
        <StatusCard
          variant="warning"
          icon={<Clock />}
          title="Launch at login is unknown"
          detail="Toolport could not read the OS setting, so it cannot say whether scheduled tasks start on their own."
          actions={
            <>
              <Button size="sm" variant="outline" onClick={state.retryAutostart}>
                Retry
              </Button>
              <Button size="sm" variant="outline" onClick={actions.openSettings}>
                Open Settings
              </Button>
            </>
          }
        />
      )}
    </div>
  );
}

function TaskRowView({
  row,
  run,
  now,
  actions,
  reload,
}: {
  row: TaskRow;
  run: RunView | undefined;
  now: number;
  actions: ListActions;
  reload: () => void;
}) {
  const state: RowState = rowState(row);
  const next = ms(row.nextRun);
  const parts = triggerParts(row.triggers, next);
  const live = row.lastRun?.status === "running" || row.waiting;
  async function toggle(on: boolean) {
    try {
      await setEnabled(row.id, on);
      toast.message(`${row.title} is ${on ? "on" : "off"}`, {
        action: {
          label: "Undo",
          onClick: () =>
            void setEnabled(row.id, !on)
              .catch((e: unknown) =>
                toastError("Undo failed", { description: errorText(e) }),
              )
              .finally(reload),
        },
      });
    } catch (error) {
      toastError(`${row.title} could not be turned ${on ? "on" : "off"}`, {
        description: errorText(error),
      });
    } finally {
      reload();
    }
  }
  return (
    <div
      className={`flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3 ${row.enabled ? "" : "opacity-80"}`}
    >
      <button
        type="button"
        onClick={() => actions.open(row.id)}
        className="flex min-w-0 flex-1 basis-64 items-start gap-3 text-left focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none"
        aria-label={`Open ${row.title}`}
      >
        <span className="grid min-w-0 flex-1 gap-0.5">
          <span className="flex flex-wrap items-center gap-2 text-sm font-semibold">
            {row.title}
            <Mono>{row.id}</Mono>
          </span>
          <span className="text-meta break-words text-muted-foreground">
            {parts.join(" · ")}
            {next !== null && (
              <>
                {" "}
                · next {stamp(next, now)} (in {inWords(next - now)})
              </>
            )}
            {!row.enabled && row.triggers.schedule ? " · off, so no next run" : ""}
          </span>
          <span className="text-meta break-words text-muted-foreground">
            {state.key === "cannot-run"
              ? `Cannot run: ${row.problems?.[0]?.message ?? ""}`
              : row.waiting
                ? `Waiting for you since ${clockTime(ms(row.lastRun?.startedAt) ?? now)}${run ? ` · stopped at step ${stepOf(run)} of ${run.steps.length}` : ""}`
                : lastRunLine(row, run, now)}
          </span>
        </span>
        <ChevronRight
          className="mt-1 size-4 shrink-0 text-muted-foreground"
          aria-hidden="true"
        />
      </button>
      <span className="flex shrink-0 items-center gap-2">
        <Chip tone={state.tone}>{state.label}</Chip>
        {!live && row.enabled && state.key !== "cannot-run" && (
          <Button
            size="xs"
            variant="outline"
            aria-label={`Run ${row.title} now`}
            onClick={() => actions.runNow(row.id)}
          >
            <Play /> Run now
          </Button>
        )}
        <Switch
          size="sm"
          aria-label={`${row.title} is ${row.enabled ? "on" : "off"}`}
          checked={row.enabled}
          onCheckedChange={(on) => void toggle(on)}
        />
      </span>
    </div>
  );
}

export function TasksTab({
  state,
  actions,
}: {
  state: TasksState;
  actions: ListActions;
}) {
  const now = useNow(30_000);
  const rows = sortRows(state.list?.tasks ?? []);
  const invalid = state.list?.invalid ?? [];
  const beat = state.list?.scheduler;
  if (state.error && !state.list)
    return (
      <StatusCard
        variant="danger"
        role="alert"
        icon={<AlertTriangle />}
        title="Could not read the tasks"
        detail={`The command task ls --all failed: ${state.error}`}
        actions={
          <Button size="sm" variant="outline" onClick={state.reload}>
            Retry
          </Button>
        }
      />
    );
  if (!state.list) return <Quiet role="status">Reading the tasks…</Quiet>;
  if (rows.length === 0 && invalid.length === 0)
    return (
      <EmptyState
        icon={<ListTodo />}
        title="No tasks yet"
        description="A task is a named list of steps Toolport can run for you: from a button, on a schedule, when a login fails, or when Claude asks. Make one, or turn one of your Claude commands into a task."
        action={
          <div className="flex flex-wrap justify-center gap-2">
            <Button size="sm" onClick={actions.newTask}>
              New task
            </Button>
            <Button size="sm" variant="outline" onClick={actions.fromCommand}>
              New from a command…
            </Button>
          </div>
        }
      />
    );
  return (
    <div className="grid gap-3">
      {state.error && (
        <StatusCard
          variant="warning"
          role="alert"
          icon={<AlertTriangle />}
          title="The list could not be refreshed"
          detail={`Showing the last good list${state.updatedAt ? `, updated ${clockTime(state.updatedAt)}` : ""}. ${state.error}`}
          actions={
            <Button size="sm" variant="outline" onClick={state.reload}>
              Retry
            </Button>
          }
        />
      )}
      {invalid.length > 0 && (
        <StatusCard
          variant="warning"
          icon={<AlertTriangle />}
          title={`${plural(invalid.length, "task file")} could not be read`}
          detail={
            <>
              {invalid.map((f) => (
                <span key={f.id} className="block break-words">
                  <Mono>{f.id}.json</Mono>: {f.error.replace(/^.*?json: /, "")}
                </span>
              ))}
              The other tasks work.
            </>
          }
        />
      )}
      <p className="text-meta text-muted-foreground">
        Sorted by what needs you, then by next run
      </p>
      <Group dim={Boolean(state.error)}>
        {rows.map((row) => (
          <TaskRowView
            key={row.id}
            row={row}
            run={state.runs.find((r) => r.task === row.id)}
            now={now}
            actions={actions}
            reload={state.reload}
          />
        ))}
      </Group>
      {beat && (
        <Quiet>
          Scheduler last checked {clockTime(ms(beat.at) ?? now)} · ticks from the{" "}
          {beat.by === "app" ? "app" : "gateway"} every {beat.intervalSecs} s
        </Quiet>
      )}
    </div>
  );
}

type Filter = "all" | "failed" | "waiting" | "catch-up";

export function HistoryTab({
  state,
  actions,
  task,
  onTask,
}: {
  state: TasksState;
  actions: ListActions;
  task: string;
  onTask: (id: string) => void;
}) {
  const now = useNow(30_000);
  const [filter, setFilter] = useState<Filter>("all");
  const [skipped, setSkipped] = useState(true);
  const [range, setRange] = useState(30);
  const ids = new Map((state.list?.tasks ?? []).map((t) => [t.id, t.title]));
  const mine = (r: RunView) =>
    (task === "" || r.task === task) && ms(r.startedAt)! >= now - range * 86_400_000;
  const runs = state.runs.filter(mine);
  const count = (f: Filter) =>
    runs.filter((r) =>
      f === "failed"
        ? r.status === "failed"
        : f === "waiting"
          ? r.status === "waiting"
          : f === "catch-up"
            ? r.trigger === "catch-up"
            : true,
    ).length;
  const shown = runs.filter((r) =>
    filter === "failed"
      ? r.status === "failed"
      : filter === "waiting"
        ? r.status === "waiting"
        : filter === "catch-up"
          ? r.trigger === "catch-up"
          : true,
  );
  const records = state.missed.filter(
    (m) => (task === "" || m.task === task) && m.outcome !== "caught-up",
  );
  const items = timeline(shown, filter === "all" ? records : [], skipped);
  const chip = (f: Filter, label: string) => (
    <button
      key={f}
      type="button"
      aria-pressed={filter === f}
      onClick={() => setFilter(f)}
      className={`rounded-full border px-3 py-1 text-xs font-medium ${filter === f ? "bg-foreground text-background" : "bg-card"}`}
    >
      {label} {count(f)}
    </button>
  );
  const groups: Array<{ label: string; items: typeof items }> = [];
  for (const item of items) {
    const label = dayLabel(item.at, now);
    const last = groups[groups.length - 1];
    if (last?.label === label) last.items.push(item);
    else groups.push({ label, items: [item] });
  }
  return (
    <div className="grid gap-3">
      <div className="flex flex-wrap items-center gap-2">
        {chip("all", "All")}
        {chip("failed", "Failed")}
        {chip("waiting", "Waiting")}
        {chip("catch-up", "Catch-up")}
        <select
          aria-label="Task"
          className={cn(SELECT_CLASS, "ml-auto w-auto")}
          value={task}
          onChange={(e) => onTask(e.target.value)}
        >
          <option value="">All tasks</option>
          {[...ids].map(([id, title]) => (
            <option key={id} value={id}>
              {title}
            </option>
          ))}
        </select>
        <select
          aria-label="Period"
          className={cn(SELECT_CLASS, "w-auto")}
          value={range}
          onChange={(e) => setRange(Number(e.target.value))}
        >
          <option value={7}>Last 7 days</option>
          <option value={30}>Last 30 days</option>
          <option value={3650}>All time</option>
        </select>
        <label className="flex items-center gap-2 text-sm">
          <Switch
            size="sm"
            checked={skipped}
            onCheckedChange={setSkipped}
            aria-label="Skipped slots"
          />
          Skipped slots {records.length}
        </label>
      </div>
      {groups.length === 0 ? (
        <Quiet>No runs match.</Quiet>
      ) : (
        groups.map((g) => (
          <section key={g.label} className="grid gap-2" aria-label={g.label}>
            <h3 className="text-xs font-semibold tracking-[0.05em] text-muted-foreground uppercase">
              {g.label}
            </h3>
            <Group>
              {g.items.map((item) =>
                item.kind === "run" ? (
                  <RunRow
                    key={item.run.id}
                    run={item.run}
                    now={now}
                    taskId={item.run.task}
                    onOpen={() => actions.open(item.run.task)}
                    onRunAgain={() => actions.runNow(item.run.task)}
                  />
                ) : (
                  <SkippedRow
                    key={`${item.record.task}-${item.record.at}-${item.record.outcome}`}
                    record={item.record}
                    now={now}
                    taskId={item.record.task}
                    onRunNow={() => actions.runNow(item.record.task)}
                  />
                ),
              )}
            </Group>
          </section>
        ))
      )}
      <Quiet>
        Runs are kept on this Mac; nothing is pruned yet. The most recent{" "}
        {state.runs.length} are loaded. Skipped slots come from the scheduler ledger (the
        last 50 are kept).
      </Quiet>
    </div>
  );
}
