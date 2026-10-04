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
import { JobProgress, PlanPreview, TypedConfirmDialog, planOf as planV1Of } from "../ui";
import { planOf } from "./plans";
import type { WriteControl, WriteSpec } from "./hooks";

function shown(spec: WriteSpec, data: unknown, done: boolean): unknown {
  const plan =
    planV1Of(data) ??
    (data && typeof data === "object"
      ? planOf(spec.command, data as Record<string, unknown>, done)
      : null);
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
  const body = (
    <div className="flex flex-col gap-3">
      <PlanPreview data={data} />
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

function Blocked({
  write,
  spec,
  data,
  reason,
  override,
}: {
  write: WriteControl;
  spec: WriteSpec;
  data: unknown;
  reason: string;
  override?: WriteSpec;
}) {
  return (
    <Dialog open onOpenChange={(open) => !open && write.dismiss()}>
      <DialogContent aria-describedby={undefined} className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{spec.title} is blocked</DialogTitle>
        </DialogHeader>
        <Callout variant="danger" role="alert">
          {reason}
        </Callout>
        <PlanPreview data={data} />
        <DialogFooter>
          <Button variant="ghost" onClick={write.dismiss}>
            Close
          </Button>
          {override && (
            <Button variant="outline" onClick={() => write.begin(override)}>
              {override.title}…
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function Review({
  write,
  spec,
  raw,
}: {
  write: WriteControl;
  spec: WriteSpec;
  raw: unknown;
}) {
  const data = shown(spec, raw, false);
  const gate = spec.gate?.(raw) ?? null;
  return gate ? (
    <Blocked write={write} spec={spec} data={data} {...gate} />
  ) : (
    <Confirm write={write} spec={spec} data={data} />
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
