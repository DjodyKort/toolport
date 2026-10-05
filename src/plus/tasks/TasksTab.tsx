import { useState, type ReactNode } from "react";
import { Plus, RefreshCw, ListPlus, Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { EmptyState } from "@/components/ui/empty-state";
import { cn } from "@/lib/utils";
import type { CommandsData } from "../bridge/data";
import { Tag } from "../system/atoms";
import { WriteDialogs } from "../system/WriteDialogs";
import { AsyncView, useCtlQuery } from "../ui";
import type { TaskDefinition, TaskRun, TaskShowData } from "../types/tasks";
import { FromCommandDialog } from "./FromCommandDialog";
import { useTaskList, useTaskWrite } from "./hooks";
import { LogDialog } from "./LogDialog";
import {
  blankForm,
  formFromTask,
  formatWhen,
  saveArgv,
  summarize,
  taskState,
  triggerChips,
  type LsTask,
  type TaskForm,
} from "./model";
import { RunTaskDialog } from "./RunDialog";
import { TaskDetail, type DetailActions } from "./TaskDetail";
import { TaskFormDialog } from "./TaskFormDialog";

function Stat({ label, value, note }: { label: string; value: ReactNode; note: string }) {
  return (
    <div className="flex min-w-0 flex-col rounded-lg border bg-card px-3 py-2">
      <span className="text-xs text-muted-foreground">{label}</span>
      <b className="text-xl font-semibold">{value}</b>
      <span className="truncate text-xs text-muted-foreground">{note}</span>
    </div>
  );
}

function Strip({ tasks }: { tasks: LsTask[] }) {
  const s = summarize(tasks);
  return (
    <div
      role="group"
      aria-label="Summary"
      className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4"
    >
      <Stat label="Tasks" value={s.total} note={`${s.enabled} on`} />
      <Stat
        label="Needs you"
        value={s.waiting.length}
        note={s.waiting.length ? s.waiting.map((t) => t.id).join(", ") : "nothing waits"}
      />
      <Stat
        label="Scheduled"
        value={s.scheduled.length}
        note={s.scheduled.map((t) => t.id).join(", ") || "none"}
      />
      <Stat
        label="Last failure"
        value={s.failed ? s.failed.id : "none"}
        note={
          s.failed?.lastRun ? formatWhen(s.failed.lastRun.startedAt) : "no failed run"
        }
      />
    </div>
  );
}

function Row({
  task,
  selected,
  onSelect,
}: {
  task: LsTask;
  selected: boolean;
  onSelect: () => void;
}) {
  const state = taskState(task);
  return (
    <li>
      <button
        type="button"
        aria-current={selected ? "true" : undefined}
        onClick={onSelect}
        className={cn(
          "flex w-full flex-col gap-1.5 rounded-lg border bg-card px-3 py-2 text-left text-sm outline-none hover:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/50",
          selected && "border-primary",
        )}
      >
        <span className="flex flex-wrap items-center gap-2">
          <b className="min-w-0 flex-1 font-semibold break-words">{task.title}</b>
          <Tag tone={state.tone}>{state.label}</Tag>
        </span>
        <code className="font-mono text-xs text-muted-foreground">{task.id}</code>
        <span className="flex flex-wrap gap-1">
          {triggerChips(task.triggers).map((chip) => (
            <span
              key={chip.key}
              className="rounded-full border px-2 py-0.5 text-xs text-muted-foreground"
            >
              {chip.label}
            </span>
          ))}
        </span>
        <span className="text-xs text-muted-foreground">
          {task.waiting
            ? "Waiting for you"
            : task.lastRun
              ? `Last run ${task.lastRun.status}, ${formatWhen(task.lastRun.startedAt)}`
              : "Never run"}
          {task.nextRun && ` · next ${formatWhen(task.nextRun)}`}
        </span>
      </button>
    </li>
  );
}

function Selected({ listed, actions }: { listed: LsTask; actions: DetailActions }) {
  const query = useCtlQuery<TaskShowData>(["task", "show", listed.id]);
  return <TaskDetail listed={listed} query={query} actions={actions} />;
}

/** The task list with the detail of the selected one, and every write on it: run, add, edit,
 * duplicate, delete and "create from a command". Each write is previewed first (D-059). */
export function TasksTab({
  initialTask,
  pollMs,
}: {
  initialTask?: string;
  pollMs?: number;
}) {
  const list = useTaskList();
  const registry = useCtlQuery<CommandsData>(["commands"]);
  const rows = registry.data?.commands ?? null;
  const [sel, setSel] = useState<string | null>(initialTask ?? null);
  const [version, setVersion] = useState(0);
  const [run, setRun] = useState<TaskDefinition | null>(null);
  const [log, setLog] = useState<string | null>(null);
  const [form, setForm] = useState<{ mode: "add" | "edit"; form: TaskForm } | null>(null);
  const [fromCommand, setFromCommand] = useState(false);
  const { reload } = list;
  const refresh = () => {
    reload();
    setVersion((n) => n + 1);
  };
  const write = useTaskWrite(rows, () => {
    refresh();
    setForm(null);
    setFromCommand(false);
  });

  const tasks = list.data?.tasks ?? [];
  const current = tasks.find((task) => task.id === sel) ?? tasks[0] ?? null;
  const actions = {
    onRun: setRun,
    onEdit: (task: TaskDefinition) => setForm({ mode: "edit", form: formFromTask(task) }),
    onDuplicate: (task: TaskDefinition) =>
      setForm({
        mode: "add",
        form: {
          ...formFromTask(task),
          id: "",
          title: `${task.title} copy`,
          enabled: false,
          createdFrom: { kind: "manual" },
        },
      }),
    onDelete: (task: TaskDefinition) =>
      write.begin({
        command: "task rm",
        title: `Delete task ${task.id}`,
        argv: ["task", "rm", task.id],
        phrase: task.id,
        confirmLabel: "Delete task",
        onResult: (_data, previewed) => {
          if (!previewed) setSel(null);
        },
      }),
    onLog: (item: TaskRun) => setLog(item.id),
  };

  const startForm = () => setForm({ mode: "add", form: blankForm() });
  const newButtons = (
    <>
      <Button onClick={startForm}>
        <Plus /> New task
      </Button>
      <Button variant="outline" onClick={() => setFromCommand(true)}>
        <ListPlus /> Create from a command…
      </Button>
    </>
  );

  return (
    <div className="flex flex-col gap-4">
      <AsyncView
        query={list}
        errorTitle="Couldn't read the tasks"
        context="task ls --all"
        isEmpty={(data) => data.tasks.length === 0 && data.invalid.length === 0}
        empty={
          <EmptyState
            icon={<Play />}
            title="No tasks yet"
            description="A task is a named list of steps Toolport can run for you: on a button, on a schedule, when a login fails, or when Claude asks. Make one from a command you already have, or write one."
            action={
              <div className="flex flex-wrap justify-center gap-2">{newButtons}</div>
            }
          />
        }
      >
        {(data) => (
          <>
            <div className="flex flex-wrap items-center gap-2">
              {newButtons}
              <Button
                variant="ghost"
                size="sm"
                className="ml-auto"
                aria-label="Refresh the tasks"
                onClick={refresh}
              >
                <RefreshCw /> Refresh
              </Button>
            </div>
            <Strip tasks={data.tasks} />
            {data.invalid.length > 0 && (
              <Callout variant="warning" role="status">
                <p className="font-medium">
                  {data.invalid.length} task{" "}
                  {data.invalid.length === 1 ? "file" : "files"} could not be read
                </p>
                <ul className="list-disc pl-4 text-sm">
                  {data.invalid.map((bad) => (
                    <li key={bad.id} className="break-words">
                      <code className="font-mono text-xs">{bad.id}</code>: {bad.error}
                    </li>
                  ))}
                </ul>
              </Callout>
            )}
            <div className="grid gap-4 lg:grid-cols-[minmax(16rem,22rem)_minmax(0,1fr)]">
              <ul aria-label="Tasks" className="flex flex-col gap-2">
                {data.tasks.map((task) => (
                  <Row
                    key={task.id}
                    task={task}
                    selected={task.id === current?.id}
                    onSelect={() => setSel(task.id)}
                  />
                ))}
              </ul>
              {current && (
                <Selected
                  key={`${current.id}:${version}`}
                  listed={current}
                  actions={actions}
                />
              )}
            </div>
          </>
        )}
      </AsyncView>
      <WriteDialogs write={write} />
      {run && (
        <RunTaskDialog
          taskId={run.id}
          task={run}
          pollMs={pollMs}
          onChanged={refresh}
          onClose={() => setRun(null)}
        />
      )}
      {log && <LogDialog runId={log} onClose={() => setLog(null)} />}
      {form && !write.spec && (
        <TaskFormDialog
          mode={form.mode}
          form={form.form}
          onChange={(next) => setForm({ mode: form.mode, form: next })}
          onClose={() => setForm(null)}
          onReview={(task) =>
            write.begin({
              command: form.mode === "add" ? "task add" : "task edit",
              title: form.mode === "add" ? `Add task ${task.id}` : `Save task ${task.id}`,
              argv: saveArgv(form.mode === "add" ? "add" : "edit", task.id),
              stdin: JSON.stringify(task, null, 2),
              confirmLabel: form.mode === "add" ? "Add task" : "Save task",
              onResult: (_data, previewed) => {
                if (!previewed) setSel(task.id);
              },
            })
          }
        />
      )}
      {fromCommand && !write.spec && (
        <FromCommandDialog
          existing={tasks.map((task) => task.id)}
          onClose={() => setFromCommand(false)}
          onReview={(id, path) =>
            write.begin({
              command: "task add",
              title: `Create task ${id} from a command`,
              argv: ["task", "add", id, "--from-command", path],
              confirmLabel: "Create draft",
              onResult: (_data, previewed) => {
                if (!previewed) setSel(id);
              },
            })
          }
        />
      )}
    </div>
  );
}
