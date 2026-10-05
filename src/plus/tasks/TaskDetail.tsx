import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Callout } from "@/components/Callout";
import { Card, Kv, Tag } from "../system/atoms";
import { AsyncView } from "../ui";
import type { CtlQuery } from "../ui";
import type { TaskDefinition, TaskRun, TaskShowData } from "../types/tasks";
import { RunBadge } from "./RunView";
import {
  TRIGGER_LABEL,
  cronText,
  formatDuration,
  formatWhen,
  stepKindLabel,
  stepText,
  type LsTask,
  taskState,
} from "./model";

function Chips({ items, none }: { items: string[]; none: string }) {
  if (items.length === 0) return <span className="text-muted-foreground">{none}</span>;
  return (
    <span className="flex flex-wrap gap-1">
      {items.map((item) => (
        <code key={item} className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs">
          {item}
        </code>
      ))}
    </span>
  );
}

function madeFrom(task: TaskDefinition): string {
  const from = task.createdFrom;
  if (!from) return "Written by hand";
  if (from.kind === "command")
    return `A Claude command${from.path ? `: ${from.path}` : ""}`;
  if (from.kind === "script") return `A script${from.path ? `: ${from.path}` : ""}`;
  return "Written by hand";
}

function Triggers({ task }: { task: TaskDefinition }) {
  const t = task.triggers;
  const rows: Array<[string, string, boolean]> = [
    ["Button in the app", "Run it from this screen", true],
    ["CLI", `toolportctl task run ${task.id}`, t.cli],
    [
      "Claude may ask",
      "Through the self-management MCP; you approve every run",
      t.selfMcp.enabled,
    ],
    [
      "Schedule",
      t.schedule
        ? `${cronText(t.schedule.cron)}${t.schedule.autoRun ? "" : ". Asks you first"}`
        : "Not set",
      t.schedule !== null,
    ],
    [
      "When the login fails",
      t.onAuthFailure.length > 0
        ? `On a failed login of ${t.onAuthFailure.join(", ")}: notify me and offer to run it`
        : "Not set",
      t.onAuthFailure.length > 0,
    ],
  ];
  return (
    <ul aria-label="Triggers" className="flex flex-col gap-2">
      {rows.map(([label, text, on]) => (
        <li key={label} className="flex items-center justify-between gap-3 text-sm">
          <div className="min-w-0">
            <b className="font-medium">{label}</b>
            <div className="text-xs text-muted-foreground break-words">{text}</div>
          </div>
          <Badge variant={on ? "success" : "secondary"}>{on ? "On" : "Off"}</Badge>
        </li>
      ))}
    </ul>
  );
}

function Runs({ runs, onLog }: { runs: TaskRun[]; onLog: (run: TaskRun) => void }) {
  if (runs.length === 0)
    return <span className="text-sm text-muted-foreground">Never run in Toolport</span>;
  return (
    <ul aria-label="Recent runs" className="flex flex-col divide-y rounded-lg border">
      {runs.map((run) => (
        <li
          key={run.id}
          className="flex flex-wrap items-center gap-2 px-3 py-1.5 text-sm"
        >
          <span className="min-w-0 flex-1">{formatWhen(run.startedAt)}</span>
          <span className="font-mono text-xs text-muted-foreground">
            {formatDuration(run.durationMs)}
          </span>
          <RunBadge status={run.status} />
          <span className="rounded-full border px-2 text-xs text-muted-foreground">
            {TRIGGER_LABEL[run.trigger]}
          </span>
          <Button
            size="xs"
            variant="ghost"
            aria-label={`Log of the run at ${formatWhen(run.startedAt)}`}
            onClick={() => onLog(run)}
          >
            Log
          </Button>
        </li>
      ))}
    </ul>
  );
}

export interface DetailActions {
  onRun: (task: TaskDefinition) => void;
  onEdit: (task: TaskDefinition) => void;
  onDuplicate: (task: TaskDefinition) => void;
  onDelete: (task: TaskDefinition) => void;
  onLog: (run: TaskRun) => void;
  onOpenRun: (task: TaskDefinition, run: TaskRun) => void;
}

/** One task: what it does step by step (the steps that need you are marked), the secrets it
 * may write by name, what it needs, how it starts and its last runs. */
export function TaskDetail({
  listed,
  query,
  actions,
}: {
  listed: LsTask;
  query: CtlQuery<TaskShowData>;
  actions: DetailActions;
}) {
  const state = taskState(listed);
  return (
    <AsyncView
      query={query}
      errorTitle={`Couldn't read ${listed.id}`}
      context={`task show ${listed.id}`}
    >
      {({ task, runs }) => (
        <Card
          title={task.title}
          actions={
            <>
              <Tag tone="secondary">{task.id}</Tag>
              <Tag tone={state.tone}>{state.label}</Tag>
              <Tag tone={task.createdFrom?.kind === "command" ? "success" : "secondary"}>
                {task.createdFrom?.kind === "command" ? "From a command" : "By hand"}
              </Tag>
            </>
          }
        >
          {task.description && (
            <p className="text-sm text-muted-foreground">{task.description}</p>
          )}
          {runs[0]?.status === "waiting" || runs[0]?.status === "running" ? (
            <Callout
              variant="warning"
              role="status"
              className="flex flex-wrap items-center gap-2"
            >
              <span className="min-w-0 flex-1">
                {runs[0].status === "waiting"
                  ? "A run waits for you."
                  : "A run is in progress."}{" "}
                <code className="font-mono text-xs">{runs[0].id}</code>
              </span>
              <Button size="sm" onClick={() => actions.onOpenRun(task, runs[0])}>
                Open run
              </Button>
            </Callout>
          ) : null}
          <section aria-label="What it does" className="flex flex-col gap-2">
            <h4 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
              What it does
            </h4>
            <ol className="flex flex-col gap-1.5">
              {task.steps.map((step, i) => (
                <li key={step.id} className="flex items-start gap-2 text-sm">
                  <span className="w-4 font-mono text-xs text-muted-foreground">
                    {i + 1}
                  </span>
                  <Badge variant={step.type === "needs-you" ? "warning" : "secondary"}>
                    {stepKindLabel(step.type)}
                  </Badge>
                  <span className="min-w-0 break-words">
                    <b className="font-medium">{step.title}</b>
                    <span className="text-muted-foreground"> · {stepText(step)}</span>
                  </span>
                </li>
              ))}
            </ol>
          </section>
          <Kv
            rows={[
              [
                "Allowed to write",
                <Chips
                  key="w"
                  items={task.writesSecrets.map((s) => `${s.key} (${s.server})`)}
                  none="no secrets"
                />,
              ],
              [
                "Needs",
                <Chips
                  key="n"
                  items={[...task.requires.servers, ...task.requires.commands]}
                  none="nothing"
                />,
              ],
              ["Made from", madeFrom(task)],
            ]}
          />
          <section aria-label="Triggers section" className="flex flex-col gap-2">
            <h4 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
              Triggers
            </h4>
            <Triggers task={task} />
            <p className="text-xs text-muted-foreground">
              Change how it starts with Edit.
            </p>
          </section>
          <div className="flex flex-wrap gap-2">
            <Button
              size="sm"
              disabled={!task.enabled}
              title={
                task.enabled ? undefined : "Turn the task on with Edit before running it"
              }
              onClick={() => actions.onRun(task)}
            >
              Run now…
            </Button>
            <Button size="sm" variant="outline" onClick={() => actions.onEdit(task)}>
              Edit
            </Button>
            <Button size="sm" variant="outline" onClick={() => actions.onDuplicate(task)}>
              Duplicate
            </Button>
            <Button size="sm" variant="outline" onClick={() => actions.onDelete(task)}>
              Delete…
            </Button>
          </div>
          <section
            aria-label="Recent runs section"
            className="flex flex-col gap-2 border-t pt-3"
          >
            <h4 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
              Recent runs
            </h4>
            <Runs runs={runs} onLog={actions.onLog} />
          </section>
        </Card>
      )}
    </AsyncView>
  );
}
