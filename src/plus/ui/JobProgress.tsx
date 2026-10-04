import { type ReactNode } from "react";
import { Loader2, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { CopyButton } from "./CopyButton";
import { DataView } from "./PlanPreview";
import { firstAddress, outcomeOf, type JobState } from "./useCtlJob";

/** A running `plus_ctl` job: stderr as it arrives, Cancel, and the final envelope as done,
 * failed or cancelled. Give it the state of `useCtlJob`. */
export function JobProgress({
  state,
  onCancel,
  title = "Running",
  doneLabel = "Done",
  renderResult,
  className,
}: {
  state: JobState;
  onCancel: () => void;
  title?: string;
  /** The heading of a successful result, e.g. "Preview ready". */
  doneLabel?: string;
  /** How a successful `data` is shown; the default is a readable list. */
  renderResult?: (data: unknown) => ReactNode;
  className?: string;
}) {
  const outcome = outcomeOf(state);
  const address = firstAddress(state.lines);
  const running = state.phase === "running";
  return (
    <div className={cn("flex flex-col gap-3", className)}>
      {running && (
        <div role="status" className="flex flex-col gap-2">
          <p className="flex items-center gap-2 text-sm font-medium">
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            {state.cancelling ? "Cancelling…" : `${title}…`}
          </p>
          <div
            aria-hidden="true"
            className="h-1.5 overflow-hidden rounded-full bg-muted motion-reduce:hidden"
          >
            <div className="h-full w-1/3 animate-pulse rounded-full bg-primary" />
          </div>
        </div>
      )}
      {address && (
        <div className="flex flex-wrap items-center gap-2">
          <code className="min-w-0 rounded bg-muted px-2 py-1 font-mono text-xs break-all">
            {address}
          </code>
          <CopyButton text={address} label="Copy address" />
        </div>
      )}
      {state.lines.length > 0 && (
        <pre
          role="log"
          aria-label="Output"
          aria-live="polite"
          className="max-h-48 overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap"
        >
          {state.lines.join("\n")}
        </pre>
      )}
      {outcome?.kind === "ok" && (
        <Callout variant="success" role="status" className="flex flex-col gap-2">
          <p className="font-medium">{doneLabel}</p>
          <div className="text-foreground">
            {renderResult ? renderResult(outcome.data) : <DataView data={outcome.data} />}
          </div>
        </Callout>
      )}
      {outcome?.kind === "error" && (
        <Callout variant="danger" role="alert" className="flex flex-col gap-1">
          <p className="font-medium">Failed</p>
          <p className="text-sm break-words">
            {outcome.code && (
              <code className="mr-1.5 font-mono text-xs">{outcome.code}</code>
            )}
            {outcome.message}
          </p>
        </Callout>
      )}
      {outcome?.kind === "cancelled" && (
        <Callout variant="info" role="status">
          Cancelled. Nothing more was started.
        </Callout>
      )}
      {running && (
        <div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={onCancel}
            disabled={state.cancelling}
          >
            <X /> Cancel
          </Button>
        </div>
      )}
    </div>
  );
}
