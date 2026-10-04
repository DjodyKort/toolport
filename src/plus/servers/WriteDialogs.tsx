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
import { JobProgress, PlanPreview, TypedConfirmDialog, planOf } from "../ui";
import type { WriteControl, WriteSpec } from "./useWrite";

function shown(spec: WriteSpec, data: unknown, done: boolean): unknown {
  const plan = planOf(data) ?? spec.adapt?.(data, done) ?? null;
  return plan ? { plan } : data;
}

/** The dialogs of a `useWrite`: the preview in progress, the plan to confirm (with the typed
 * confirmation for a destructive tier) and the result. */
export function WriteDialogs({ write }: { write: WriteControl }) {
  const { flow, spec, refused } = write;
  const { dialog, preview, apply } = flow;
  const closeable = !flow.busy;
  const previewOk = !!preview.state.result?.envelope?.ok;
  const previewing =
    apply.state.phase === "idle" &&
    (preview.state.phase === "running" || (preview.state.phase === "done" && !previewOk));
  const applying = apply.state.phase !== "idle";
  return (
    <>
      {refused && (
        <Callout variant="warning" role="status">
          {refused}
        </Callout>
      )}
      {spec && dialog === "confirm" && <ConfirmStep write={write} spec={spec} />}
      {spec && dialog === "review" && previewOk && (
        <ReviewStep
          write={write}
          spec={spec}
          data={shown(spec, preview.state.result?.envelope?.data, false)}
        />
      )}
      {spec && previewing && (
        <Dialog open onOpenChange={(open) => !open && closeable && flow.reset()}>
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
                <Button variant="ghost" onClick={flow.reset}>
                  Close
                </Button>
              </DialogFooter>
            )}
          </DialogContent>
        </Dialog>
      )}
      {spec && applying && (
        <Dialog open onOpenChange={(open) => !open && closeable && flow.reset()}>
          <DialogContent aria-describedby={undefined} className="sm:max-w-2xl">
            <DialogHeader>
              <DialogTitle>{spec.title}</DialogTitle>
            </DialogHeader>
            <JobProgress
              state={apply.state}
              onCancel={() => void apply.cancel()}
              title="Applying"
              doneLabel="Done"
              renderResult={(data) => <PlanPreview data={shown(spec, data, true)} />}
            />
            {closeable && (
              <DialogFooter>
                <Button onClick={flow.reset}>Close</Button>
              </DialogFooter>
            )}
          </DialogContent>
        </Dialog>
      )}
    </>
  );
}

function ConfirmStep({ write, spec }: { write: WriteControl; spec: WriteSpec }) {
  const { flow } = write;
  const flowSpec = flow.spec;
  if (!flowSpec) return null;
  const body = (
    <div className="flex flex-col gap-3">
      {flowSpec.detail}
      <CommandLine line={flowSpec.line} />
      <Callout variant="warning">
        This command has no preview. It makes its changes as soon as you confirm.
      </Callout>
    </div>
  );
  return flowSpec.tier === "destructive" ? (
    <TypedConfirmDialog
      open
      onOpenChange={(open) => !open && flow.closeDialog()}
      title={`${spec.title}?`}
      phrase={flowSpec.phrase}
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
      contentClassName="sm:max-w-xl"
      description={body}
      confirmLabel={spec.confirmLabel ?? "Apply"}
      onConfirm={flow.confirm}
    />
  );
}

function ReviewStep({
  write,
  spec,
  data,
}: {
  write: WriteControl;
  spec: WriteSpec;
  data: unknown;
}) {
  const { flow } = write;
  const flowSpec = flow.spec;
  if (!flowSpec) return null;
  const body = (
    <div className="flex flex-col gap-3">
      <PlanPreview data={data} />
      <CommandLine line={flowSpec.line} />
    </div>
  );
  return flowSpec.tier === "destructive" ? (
    <TypedConfirmDialog
      open
      onOpenChange={(open) => !open && flow.closeDialog()}
      title={`${spec.title}?`}
      phrase={flowSpec.phrase}
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
