import type { ReactNode } from "react";
import { Ban, Check, Clock, Loader2, Minus, UserRound, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Callout } from "@/components/Callout";
import type { TaskRun } from "../types/tasks";
import {
  RUN_STATUS,
  STEP_STATUS,
  TRIGGER_LABEL,
  formatDuration,
  formatWhen,
  stepKindLabel,
} from "./model";

const ICON: Record<string, ReactNode> = {
  pending: <Clock className="text-muted-foreground" />,
  running: <Loader2 className="animate-spin text-primary motion-reduce:animate-none" />,
  waiting: <UserRound className="text-warning" />,
  ok: <Check className="text-success" />,
  failed: <X className="text-destructive" />,
  cancelled: <Ban className="text-muted-foreground" />,
  skipped: <Minus className="text-muted-foreground" />,
};

export function RunBadge({ status }: { status: TaskRun["status"] }) {
  const state = RUN_STATUS[status];
  return <Badge variant={state.tone}>{state.label}</Badge>;
}

/** The steps of a run in order, with what each one is doing, the instructions of a step that
 * waits for you and the redacted log of a step that wrote one. `actions` renders under the
 * waiting step (the Continue button). */
export function RunSteps({
  run,
  actions,
}: {
  run: TaskRun;
  actions?: (step: TaskRun["steps"][number]) => ReactNode;
}) {
  return (
    <ol aria-label="Steps" className="flex flex-col gap-2">
      {run.steps.map((step, i) => (
        <li
          key={step.id}
          aria-label={`${i + 1}. ${step.title}`}
          data-status={step.status}
          className="flex flex-col gap-1.5 rounded-lg border bg-card p-2.5"
        >
          <div className="flex flex-wrap items-center gap-2 text-sm">
            <span className="size-4 shrink-0 [&_svg]:size-4" aria-hidden="true">
              {ICON[step.status]}
            </span>
            <span className="font-mono text-xs text-muted-foreground">{i + 1}</span>
            <b className="font-medium">{step.title}</b>
            <Badge variant={step.type === "needs-you" ? "warning" : "secondary"}>
              {stepKindLabel(step.type)}
            </Badge>
            <span className="ml-auto text-xs text-muted-foreground">
              {STEP_STATUS[step.status] ?? step.status}
            </span>
          </div>
          {step.status === "waiting" && step.instructions && (
            <Callout variant="warning" role="status">
              {step.instructions}
            </Callout>
          )}
          {step.status === "waiting" && actions?.(step)}
          {step.output && (
            <pre
              aria-label={`Log of ${step.title}`}
              className="max-h-40 overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap"
            >
              {step.output}
            </pre>
          )}
        </li>
      ))}
    </ol>
  );
}

/** The result line of a run: status, when it started, how long it took and what failed. */
export function RunSummary({ run }: { run: TaskRun }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        <RunBadge status={run.status} />
        <code className="font-mono text-xs">{run.id}</code>
        <span className="text-muted-foreground">
          started {formatWhen(run.startedAt)} by {TRIGGER_LABEL[run.trigger]}
          {run.durationMs !== null && ` · took ${formatDuration(run.durationMs)}`}
        </span>
      </div>
      {run.status === "failed" && (
        <Callout variant="danger" role="alert">
          <p className="font-medium">The run failed</p>
          {run.error && <p className="text-sm break-words">{run.error}</p>}
        </Callout>
      )}
      {run.status === "ok" && (
        <Callout variant="success" role="status">
          The run finished.
        </Callout>
      )}
      {run.status === "cancelled" && (
        <Callout variant="info" role="status">
          The run was cancelled. Nothing more was started.
        </Callout>
      )}
    </div>
  );
}
