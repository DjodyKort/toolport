import type { ReactNode } from "react";
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
import type { WriteControl, WriteSpec } from "./hooks";

function shown(spec: WriteSpec, data: unknown, done: boolean): unknown {
  if (done && spec.planned) {
    return { plan: { ...spec.planned, summary: spec.done ?? spec.planned.summary } };
  }
  const plan =
    planOf(data) ??
    (data && typeof data === "object"
      ? spec.adapt?.(data as Record<string, unknown>, done)
      : null) ??
    null;
  return plan ? { plan } : data;
}

function Confirm({
  write,
  spec,
  body,
}: {
  write: WriteControl;
  spec: WriteSpec;
  body: ReactNode;
}) {
  const { flow } = write;
  const flowSpec = flow.spec;
  if (!flowSpec) return null;
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
      {spec && flow.spec && dialog === "confirm" && (
        <Confirm
          write={write}
          spec={spec}
          body={
            <div className="flex flex-col gap-3">
              {flow.spec.detail}
              <CommandLine line={flow.spec.line} />
              <Callout variant="warning">
                This command has no preview. It makes its changes as soon as you confirm.
              </Callout>
            </div>
          }
        />
      )}
      {spec && flow.spec && dialog === "review" && previewOk && (
        <Confirm
          write={write}
          spec={spec}
          body={
            <div className="flex flex-col gap-3">
              <PlanPreview
                data={shown(spec, preview.state.result?.envelope?.data, false)}
              />
              <CommandLine line={flow.spec.line} />
            </div>
          }
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
              renderResult={(data) => <PlanPreview data={shown(spec, data, true)} />}
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
