import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  CopyButton,
  DataView,
  JobProgress,
  PlanPreview,
  TypedConfirmDialog,
} from "../ui";
import type { FlowSpec, RunFlowControl } from "./useRunFlow";

export function CommandLine({ line }: { line: string }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <code
        aria-label="Command line"
        className="min-w-0 rounded bg-muted px-2 py-1 font-mono text-xs break-all"
      >
        {line}
      </code>
      <CopyButton text={line} label="Copy command" />
    </div>
  );
}

/** The dialogs and the progress of a `useRunFlow`. */
export function RunFlowView({ flow }: { flow: RunFlowControl }) {
  const { spec, dialog, preview, apply } = flow;
  const previewed = preview.state.phase === "done" && !!spec;
  const previewOk = previewed && !!preview.state.result?.envelope?.ok;
  return (
    <div className="flex flex-col gap-4">
      {preview.state.phase !== "idle" && (
        <section aria-label="Preview result" className="flex flex-col gap-2">
          <JobProgress
            state={preview.state}
            onCancel={() => void preview.cancel()}
            title="Previewing"
            doneLabel="Preview ready"
            renderResult={(data) => <PlanPreview data={data} />}
          />
          {previewOk && apply.state.phase === "idle" && dialog === "none" && (
            <div>
              <Button type="button" size="sm" onClick={flow.reviewAgain}>
                Review and apply
              </Button>
            </div>
          )}
        </section>
      )}
      {apply.state.phase !== "idle" && (
        <section aria-label="Run result" className="flex flex-col gap-2">
          <JobProgress
            state={apply.state}
            onCancel={() => void apply.cancel()}
            title={spec?.mode === "run" ? "Running" : "Applying"}
            renderResult={(data) => <DataView data={data} />}
          />
        </section>
      )}
      {spec && dialog === "confirm" && <ConfirmStep flow={flow} spec={spec} />}
      {spec && dialog === "review" && previewOk && (
        <ReviewStep flow={flow} spec={spec} data={preview.state.result?.envelope?.data} />
      )}
    </div>
  );
}

function ConfirmStep({ flow, spec }: { flow: RunFlowControl; spec: FlowSpec }) {
  const body = (
    <div className="flex flex-col gap-3">
      <CommandLine line={spec.line} />
      {spec.detail}
      {spec.confirmFirst && (
        <Callout variant="warning">This can use paid tokens or quota.</Callout>
      )}
      {spec.mode === "direct" && (
        <Callout variant="warning">
          This command has no preview. It makes its changes as soon as you confirm.
        </Callout>
      )}
    </div>
  );
  const typed = spec.tier === "destructive";
  return typed ? (
    <TypedConfirmDialog
      open
      onOpenChange={(open) => !open && flow.closeDialog()}
      title={`Run ${spec.title}?`}
      phrase={spec.phrase}
      confirmLabel="Run"
      onConfirm={flow.confirm}
    >
      {body}
    </TypedConfirmDialog>
  ) : (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && flow.closeDialog()}
      title={`Run ${spec.title}?`}
      contentClassName="sm:max-w-lg"
      description={body}
      confirmLabel="Run"
      onConfirm={flow.confirm}
    />
  );
}

function ReviewStep({
  flow,
  spec,
  data,
}: {
  flow: RunFlowControl;
  spec: FlowSpec;
  data: unknown;
}) {
  const body = (
    <div className="flex flex-col gap-3">
      <PlanPreview data={data} />
      <CommandLine line={spec.line} />
      {spec.detail}
    </div>
  );
  return spec.tier === "destructive" ? (
    <TypedConfirmDialog
      open
      onOpenChange={(open) => !open && flow.closeDialog()}
      title={`Apply ${spec.title}?`}
      phrase={spec.phrase}
      confirmLabel="Apply"
      onConfirm={flow.confirm}
    >
      {body}
    </TypedConfirmDialog>
  ) : (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && flow.closeDialog()}
      title={`Apply ${spec.title}?`}
      contentClassName="sm:max-w-2xl"
      description={body}
      confirmLabel="Apply"
      onConfirm={flow.confirm}
    />
  );
}
