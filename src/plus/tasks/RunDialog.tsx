import { useEffect, useMemo, useRef, useState } from "react";
import { Loader2, X } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { CommandLine } from "../allcommands/RunFlow";
import { commandLine } from "../allcommands/model";
import { ctlData } from "../bridge/ctl";
import type { CommandsData } from "../bridge/data";
import { Kv } from "../system/atoms";
import { policyOf } from "../system/model";
import {
  ErrorState,
  JobProgress,
  PlanPreview,
  errorText,
  outcomeOf,
  useCtlJob,
  useCtlQuery,
} from "../ui";
import type { TaskDefinition, TaskRunData } from "../types/tasks";
import { RUN_POLL_MS, useRunPoll } from "./hooks";
import { isActive, needsYou } from "./model";
import { RunSteps, RunSummary } from "./RunView";

function Names({ items, none }: { items: string[]; none: string }) {
  if (items.length === 0) return <span className="text-muted-foreground">{none}</span>;
  return (
    <span className="flex flex-wrap gap-1">
      {items.map((item) => (
        <code key={item} className="rounded bg-muted px-1.5 font-mono text-xs">
          {item}
        </code>
      ))}
    </span>
  );
}

function Review({
  data,
  task,
  line,
}: {
  data: TaskRunData;
  task: TaskDefinition | null | undefined;
  line: string;
}) {
  const waits = task
    ? needsYou(task)
    : data.plan.steps.some((step) => /\(needs-you\)/.test(step.detail));
  return (
    <div className="flex flex-col gap-3">
      <PlanPreview data={{ plan: data.plan }} />
      <Kv
        rows={[
          [
            "May write",
            <Names
              key="w"
              items={data.writesSecrets.map((s) => `${s.key} (${s.server})`)}
              none="no secrets"
            />,
          ],
          [
            "Needs",
            <Names
              key="n"
              items={[...data.requires.servers, ...data.requires.commands]}
              none="nothing"
            />,
          ],
          [
            "Waits for you",
            waits ? "Yes: it pauses at a step that needs you" : "No: it runs on its own",
          ],
        ]}
      />
      <p className="text-xs text-muted-foreground">
        Secret values are never shown or logged. The run is kept in History with a
        redacted log.
      </p>
      <CommandLine line={line} />
    </div>
  );
}

/** Runs one task the safe way (D-059): the dry run first, its plan to read, a confirmation,
 * then the run with its steps updating as the runner works. A step that waits for you shows
 * its instructions and a Continue button; Cancel stops the run. Closing the window leaves a
 * running task running; its record is in History. */
export function RunTaskDialog({
  taskId,
  task,
  title: given,
  onClose,
  onChanged,
  pollMs = RUN_POLL_MS,
}: {
  taskId: string;
  task?: TaskDefinition | null;
  title?: string;
  onClose: () => void;
  onChanged?: () => void;
  pollMs?: number;
}) {
  const title = task?.title ?? given ?? taskId;
  const registry = useCtlQuery<CommandsData>(["commands"]);
  const rows = registry.data?.commands ?? null;
  const policy = policyOf(rows, "task run");
  const preview = useCtlJob();
  const apply = useCtlJob();
  const { start: startPreview } = preview;
  const started = useRef(false);
  const confirmed = useRef(false);
  const [act, setAct] = useState<{ busy: boolean; error: unknown }>({
    busy: false,
    error: null,
  });
  const runArgv = useMemo(() => ["task", "run", taskId], [taskId]);

  useEffect(() => {
    if (!policy || started.current) return;
    started.current = true;
    void startPreview([...runArgv, "--dry-run"]);
  }, [policy, startPreview, runArgv]);

  const previewOutcome = outcomeOf(preview.state);
  const applyOutcome = outcomeOf(apply.state);
  const started_ =
    applyOutcome?.kind === "ok" ? (applyOutcome.data as TaskRunData) : null;
  const runId = started_?.run?.id ?? null;
  const poll = useRunPoll(runId, pollMs, started_?.run ?? undefined);
  const run = poll.run;
  const status = run?.status;
  const { refresh } = poll;

  useEffect(() => {
    if (status) onChanged?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);

  async function step(verb: "resume" | "cancel") {
    if (!runId) return;
    setAct({ busy: true, error: null });
    try {
      await ctlData(["task", verb, runId]);
      setAct({ busy: false, error: null });
      refresh();
    } catch (error) {
      setAct({ busy: false, error });
    }
  }

  const close = () => {
    preview.reset();
    onClose();
  };

  if (registry.status === "error" || (rows && !policy)) {
    return (
      <Dialog open onOpenChange={(open) => !open && close()}>
        <DialogContent aria-describedby={undefined} className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>Run {title}</DialogTitle>
          </DialogHeader>
          {registry.status === "error" ? (
            <ErrorState
              error={registry.error}
              title="Couldn't read the command list"
              onRetry={registry.reload}
            />
          ) : (
            <Callout variant="warning" role="status">
              Toolport does not know how safe `task run` is, so it does not run it from
              here.
            </Callout>
          )}
          <DialogFooter>
            <Button onClick={close}>Close</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    );
  }

  if (apply.state.phase === "idle" && previewOutcome?.kind === "ok") {
    return (
      <ConfirmDialog
        open
        onOpenChange={(open) => !open && !confirmed.current && close()}
        title={`Run ${title}?`}
        contentClassName="sm:max-w-2xl"
        confirmLabel="Run now"
        onConfirm={() => {
          confirmed.current = true;
          void apply.start(runArgv);
        }}
        description={
          <Review
            data={previewOutcome.data as TaskRunData}
            task={task}
            line={commandLine(runArgv)}
          />
        }
      />
    );
  }

  const starting = apply.state.phase === "running";
  const active = run ? isActive(run) : false;
  return (
    <Dialog open onOpenChange={(open) => !open && close()}>
      <DialogContent aria-describedby={undefined} className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Run {title}</DialogTitle>
        </DialogHeader>
        <div className="flex max-h-[60vh] flex-col gap-3 overflow-y-auto">
          {apply.state.phase === "idle" && (
            <JobProgress
              state={preview.state}
              onCancel={() => void preview.cancel()}
              title="Previewing"
              renderResult={() => null}
            />
          )}
          {starting && (
            <p role="status" className="flex items-center gap-2 text-sm font-medium">
              <Loader2 className="size-4 animate-spin" aria-hidden="true" /> Starting the
              run…
            </p>
          )}
          {applyOutcome?.kind === "error" && (
            <Callout variant="danger" role="alert">
              <p className="font-medium">The run did not start</p>
              <p className="text-sm break-words">
                {applyOutcome.code && (
                  <code className="mr-1.5 font-mono text-xs">{applyOutcome.code}</code>
                )}
                {applyOutcome.message}
              </p>
            </Callout>
          )}
          {run && (
            <>
              <RunSummary run={run} />
              <RunSteps
                run={run}
                actions={() => (
                  <div>
                    <Button
                      size="sm"
                      disabled={act.busy}
                      onClick={() => void step("resume")}
                    >
                      Continue
                    </Button>
                  </div>
                )}
              />
            </>
          )}
          {poll.error !== null && (
            <Callout variant="warning" role="status">
              Couldn&apos;t read the run just now: {errorText(poll.error).message}. It
              keeps trying.
            </Callout>
          )}
          {act.error !== null && (
            <Callout variant="danger" role="alert">
              {errorText(act.error).message}
            </Callout>
          )}
          {active && (
            <p className="text-xs text-muted-foreground">
              Closing this window does not stop the run. It stays in History.
            </p>
          )}
        </div>
        <DialogFooter>
          {active && (
            <Button
              variant="outline"
              disabled={act.busy}
              onClick={() => void step("cancel")}
            >
              <X /> Cancel run
            </Button>
          )}
          <Button variant={active ? "ghost" : "default"} onClick={close}>
            Close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
