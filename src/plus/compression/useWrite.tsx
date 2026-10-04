import { useCallback, useEffect, useRef, useState } from "react";
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
import type { CommandRow, CommandsData } from "../bridge/data";
import { commandLine } from "../allcommands/model";
import { CommandLine } from "../allcommands/RunFlow";
import { useRunFlow, type RunFlowControl } from "../allcommands/useRunFlow";
import {
  JobProgress,
  PlanPreview,
  TypedConfirmDialog,
  outcomeOf,
  useCtlQuery,
  type JobState,
  type PlanV1,
} from "../ui";
import { policyOf } from "./model";

export function useRegistryRows(): CommandRow[] | null {
  return useCtlQuery<CommandsData>(["commands"]).data?.commands ?? null;
}

/** One write of the screen: preview, confirm, apply, result (D-059). */
export interface WriteSpec {
  /** The registry id the policy is read from, e.g. `compression set-provider`. */
  command: string;
  title: string;
  /** The apply argv. */
  argv: string[];
  /** The preview argv when it is not the apply argv plus the registry's preview flag. */
  previewArgv?: string[];
  confirmLabel?: string;
  /** What is typed to confirm a destructive tier. */
  phrase: string;
  /** Forces the typed confirmation although the policy tier is lower. */
  typed?: boolean;
  /** Reads a preview or a result as a plan; without it the data is listed as it is. */
  plan?: (data: unknown) => PlanV1 | null;
  /** A line shown above the plan, such as "this downloads the engine". */
  notice?: string;
  /** What to do next, per error code of the CLI, shown under a failure. */
  hints?: Record<string, string>;
}

export interface WriteControl {
  flow: RunFlowControl;
  spec: WriteSpec | null;
  refused: string | null;
  begin: (spec: WriteSpec) => void;
  dismiss: () => void;
}

export function useWrite(rows: CommandRow[] | null, onApplied: () => void): WriteControl {
  const flow = useRunFlow();
  const { begin: beginFlow, reset } = flow;
  const [spec, setSpec] = useState<WriteSpec | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const handled = useRef<unknown>(null);
  const applied = useRef(onApplied);
  useEffect(() => {
    applied.current = onApplied;
  });

  const begin = useCallback(
    (next: WriteSpec) => {
      const policy = policyOf(rows, next.command);
      if (!policy || policy.terminal) {
        setRefused(
          policy
            ? `\`${next.command}\` needs a terminal. Copy its command line and run it there.`
            : rows
              ? `Toolport does not know how safe \`${next.command}\` is, so it does not run it from here.`
              : "The command list has not loaded yet. Try again in a moment.",
        );
        return;
      }
      const flag = next.previewArgv ? null : policy.previewFlag;
      const previewArgv = next.previewArgv ?? (flag ? [...next.argv, flag] : undefined);
      setRefused(null);
      setSpec({
        ...next,
        notice:
          next.notice ?? (policy.network ? "This command uses the network." : undefined),
      });
      beginFlow({
        title: next.title,
        line: commandLine(next.argv),
        tier: next.typed ? "destructive" : policy.tier,
        phrase: next.phrase,
        mode: previewArgv ? "preview" : "direct",
        confirmFirst: false,
        argv: next.argv,
        previewArgv,
      });
    },
    [rows, beginFlow],
  );

  const state = flow.apply.state;
  useEffect(() => {
    if (state.phase !== "done" || handled.current === state) return;
    handled.current = state;
    if (outcomeOf(state)?.kind === "ok") applied.current();
  }, [state]);

  const dismiss = useCallback(() => {
    setRefused(null);
    setSpec(null);
    reset();
  }, [reset]);

  return { flow, spec, refused, begin, dismiss };
}

function Hint({ spec, state }: { spec: WriteSpec; state: JobState }) {
  const outcome = outcomeOf(state);
  const hint =
    outcome?.kind === "error" && outcome.code ? spec.hints?.[outcome.code] : undefined;
  return hint ? <Callout variant="info">{hint}</Callout> : null;
}

const shown = (spec: WriteSpec, data: unknown) => {
  const plan = spec.plan?.(data);
  return plan ? { plan } : data;
};

function Review({
  write,
  spec,
  raw,
}: {
  write: WriteControl;
  spec: WriteSpec;
  raw: unknown;
}) {
  const { flow } = write;
  const body = (
    <div className="flex flex-col gap-3">
      {spec.notice && <Callout variant="warning">{spec.notice}</Callout>}
      <PlanPreview data={shown(spec, raw)} />
      <CommandLine line={flow.spec?.line ?? ""} />
    </div>
  );
  return flow.spec?.tier === "destructive" ? (
    <TypedConfirmDialog
      open
      onOpenChange={(open) => !open && flow.closeDialog()}
      title={`${spec.title}?`}
      phrase={spec.phrase}
      confirmLabel={spec.confirmLabel ?? "Apply"}
      onConfirm={flow.confirm}
    >
      {body}
    </TypedConfirmDialog>
  ) : (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && flow.closeDialog()}
      title={`${spec.title}?`}
      contentClassName="sm:max-w-2xl"
      description={body}
      confirmLabel={spec.confirmLabel ?? "Apply"}
      onConfirm={flow.confirm}
    />
  );
}

function Direct({ write, spec }: { write: WriteControl; spec: WriteSpec }) {
  const { flow } = write;
  return (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && flow.closeDialog()}
      title={`${spec.title}?`}
      contentClassName="sm:max-w-lg"
      confirmLabel={spec.confirmLabel ?? "Run"}
      onConfirm={flow.confirm}
      description={
        <div className="flex flex-col gap-3">
          {spec.notice && <Callout variant="warning">{spec.notice}</Callout>}
          <p>This command has no preview. It acts as soon as you confirm.</p>
          <CommandLine line={flow.spec?.line ?? ""} />
        </div>
      }
    />
  );
}

/** The dialogs of a `useWrite`: the preview in progress, the plan to confirm (typed for a
 * destructive tier) and the result. */
export function WriteDialogs({ write }: { write: WriteControl }) {
  const { flow, spec, refused } = write;
  const { dialog, preview, apply } = flow;
  const previewOk = !!preview.state.result?.envelope?.ok;
  const previewing =
    apply.state.phase === "idle" &&
    (preview.state.phase === "running" || (preview.state.phase === "done" && !previewOk));
  const applying = apply.state.phase !== "idle";
  const closeable = !flow.busy;
  return (
    <>
      {refused && (
        <Callout variant="warning" role="status">
          {refused}
        </Callout>
      )}
      {spec && dialog === "confirm" && <Direct write={write} spec={spec} />}
      {spec && dialog === "review" && previewOk && (
        <Review write={write} spec={spec} raw={preview.state.result?.envelope?.data} />
      )}
      {spec && previewing && (
        <Dialog open onOpenChange={(open) => !open && closeable && write.dismiss()}>
          <DialogContent aria-describedby={undefined} className="sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>{spec.title}</DialogTitle>
            </DialogHeader>
            <JobProgress
              state={preview.state}
              onCancel={() => void preview.cancel()}
              title="Previewing"
            />
            <Hint spec={spec} state={preview.state} />
            {closeable && (
              <DialogFooter>
                <Button variant="ghost" onClick={write.dismiss}>
                  Close
                </Button>
              </DialogFooter>
            )}
          </DialogContent>
        </Dialog>
      )}
      {spec && applying && (
        <Dialog open onOpenChange={(open) => !open && closeable && write.dismiss()}>
          <DialogContent aria-describedby={undefined} className="sm:max-w-2xl">
            <DialogHeader>
              <DialogTitle>{spec.title}</DialogTitle>
            </DialogHeader>
            <JobProgress
              state={apply.state}
              onCancel={() => void apply.cancel()}
              title="Applying"
              renderResult={(data) => <PlanPreview data={shown(spec, data)} />}
            />
            <Hint spec={spec} state={apply.state} />
            {closeable && (
              <DialogFooter>
                <Button onClick={write.dismiss}>Close</Button>
              </DialogFooter>
            )}
          </DialogContent>
        </Dialog>
      )}
    </>
  );
}
