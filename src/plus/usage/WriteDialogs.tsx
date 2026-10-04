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
import { JobProgress, PlanPreview, TypedConfirmDialog } from "../ui";
import type { WriteControl, WriteSpec } from "./useWrite";

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
      onOpenChange={(open) => !open && write.dismiss()}
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
