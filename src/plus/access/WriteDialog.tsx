import { useState } from "react";
import { TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { errorText } from "../settings/useCtl";
import { useLoad } from "../servers/useLoad";
import type { WriteOutcome } from "./data";
import { nothingToChange } from "./model";
import type { AccessChange } from "./registryChange";

/** One change to the access sets: the writer is run twice, first as a plan and, once the person
 * confirms, for real. */
export interface Write {
  title: string;
  description?: string;
  confirmLabel: string;
  destructive?: boolean;
  /** Plans with `true`, writes with `false`. */
  run: (dryRun: boolean) => Promise<WriteOutcome>;
  /** What the write did to the registry, for upstream's Access page. */
  change: (outcome: WriteOutcome) => AccessChange | null;
  /** What the toast says once it is done. */
  done: string;
}

export interface WriteDialogProps {
  write: Write;
  onClose: () => void;
  onApplied: (write: Write, outcome: WriteOutcome) => void;
}

/** The plan first, then the write: nothing is written until Confirm, and a refusal or a failed
 * write stays on screen with its reason. */
export default function WriteDialog({ write, onClose, onApplied }: WriteDialogProps) {
  const plan = useLoad(() => write.run(true), write);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const outcome = plan.data;
  const idle = outcome !== null && nothingToChange(outcome);

  async function apply() {
    setBusy(true);
    setFailure(null);
    try {
      onApplied(write, await write.run(false));
      onClose();
    } catch (error) {
      setFailure(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{write.title}</DialogTitle>
          <DialogDescription>
            {write.description ??
              "The plan comes first. Nothing is written until you confirm."}
          </DialogDescription>
        </DialogHeader>
        {plan.error && (
          <p role="alert" className="text-meta text-destructive">
            {plan.error}
          </p>
        )}
        {!outcome && !plan.error && (
          <p role="status" className="text-meta text-muted-foreground">
            Preparing the plan…
          </p>
        )}
        {outcome && (
          <div className="flex flex-col gap-3 text-meta">
            <p className="text-sm font-medium">{outcome.plan.summary}</p>
            <ul aria-label="Plan" className="flex list-disc flex-col gap-1 pl-5">
              {outcome.plan.steps.map((step) => (
                <li key={step.detail}>{step.detail}</li>
              ))}
            </ul>
            {outcome.plan.warnings.map((warning) => (
              <p key={warning} className="flex items-start gap-1.5 text-warning">
                <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
                <span>{warning}</span>
              </p>
            ))}
            {idle ? (
              <p role="status" className="text-muted-foreground">
                Nothing to change.
              </p>
            ) : (
              <p className="text-muted-foreground">
                After applying, the toast offers Undo. To put it back by hand:{" "}
                <code className="font-mono break-all">{outcome.plan.undo}</code>
              </p>
            )}
          </div>
        )}
        {failure && (
          <p role="alert" className="text-meta text-destructive">
            Nothing was changed: {failure}
          </p>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant={write.destructive ? "destructive" : "default"}
            data-confirm
            autoFocus
            disabled={!outcome || idle || busy}
            onClick={() => void apply()}
          >
            {write.confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
