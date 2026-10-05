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
import {
  DataView,
  JobProgress,
  PlanPreview,
  TypedConfirmDialog,
  planOf as planV1Of,
  resultOf,
} from "../ui";
import { contextPlan } from "./plans";
import type { WriteControl, WriteSpec } from "./hooks";

function shown(spec: WriteSpec, data: unknown, done: boolean): unknown {
  const plan = planV1Of(data) ?? contextPlan(spec.command, data, done);
  return plan ? { plan } : data;
}

function Confirm({
  write,
  spec,
  data,
}: {
  write: WriteControl;
  spec: WriteSpec;
  data: unknown;
}) {
  const { flow } = write;
  const line = flow.spec?.line ?? "";
  const body = (
    <div className="flex flex-col gap-3">
      {data === null ? flow.spec?.detail : <PlanPreview data={data} />}
      <CommandLine line={line} />
      {data === null && spec.warnNoPreview !== false && (
        <Callout variant="warning">
          This command has no preview. It makes its changes as soon as you confirm.
        </Callout>
      )}
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

/** The result of a command that answers with a plan and a `ResultV1`: what it did, the files
 * it changed and the command that reverts it. */
function ResultBody({ data }: { data: unknown }) {
  const plan = planV1Of(data);
  return (
    <section aria-label="Result" className="flex flex-col gap-2">
      {plan && <p className="text-sm font-medium">{plan.summary}</p>}
      <DataView data={{ result: resultOf(data) }} />
    </section>
  );
}

/** The dialogs of a `useWrite`: the preview in progress, the plan to confirm (with the typed
 * confirmation for a destructive tier) and the result. */
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
      {spec && dialog === "confirm" && flow.spec && (
        <Confirm write={write} spec={spec} data={null} />
      )}
      {spec && dialog === "review" && previewOk && (
        <Confirm
          write={write}
          spec={spec}
          data={shown(spec, preview.state.result?.envelope?.data, false)}
        />
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
              renderResult={(data) =>
                spec.renderResult ? (
                  spec.renderResult(data)
                ) : resultOf(data) ? (
                  <ResultBody data={data} />
                ) : (
                  <PlanPreview data={shown(spec, data, true)} />
                )
              }
            />
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
