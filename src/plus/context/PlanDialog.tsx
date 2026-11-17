import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { useLoad } from "../servers/useLoad";
import type { Plan } from "./data";
import { tokens } from "./model";

const SIGN: Record<string, string> = { create: "+", update: "~", delete: "−", note: "·" };

export function PlanView({ plan }: { plan: Plan }) {
  const effect = plan.effects.tokens;
  return (
    <div className="flex flex-col gap-3 text-meta">
      <p className="text-sm font-medium">{plan.summary}</p>
      <ul className="flex flex-col gap-1.5" aria-label="Plan">
        {plan.steps.map((step, index) => (
          <li key={`${step.path}-${index}`} className="flex flex-col gap-0.5">
            <span>
              <span className="mr-2 font-mono">{SIGN[step.op] ?? step.op}</span>
              {step.detail}
            </span>
            <span className="break-all font-mono text-muted-foreground">{step.path}</span>
            {step.diff && (
              <pre className="overflow-x-auto whitespace-pre-wrap rounded-md bg-muted p-2 font-mono">
                {step.diff.after}
              </pre>
            )}
          </li>
        ))}
      </ul>
      {effect && (
        <p>
          Effect: ≈ {tokens(effect.before)} → ≈ {tokens(effect.after)} tokens (
          {effect.basis}).
        </p>
      )}
      {plan.warnings.map((warning) => (
        <p key={warning} className="text-muted-foreground">
          {warning}
        </p>
      ))}
    </div>
  );
}

export interface PlanDialogProps {
  title: string;
  description?: string;
  confirmLabel: string;
  destructive?: boolean;
  /** Runs the same command with `--dry-run`: what the dialog shows before anything is written. */
  preview: () => Promise<{ plan: Plan }>;
  confirm: () => Promise<unknown>;
  onClose: () => void;
  onDone: () => void;
}

/** The plan first, then the write: Enter confirms the primary action, Esc cancels. */
export default function PlanDialog({
  title,
  description,
  confirmLabel,
  destructive,
  preview,
  confirm,
  onClose,
  onDone,
}: PlanDialogProps) {
  const plan = useLoad(preview, title);
  const [busy, setBusy] = useState(false);

  async function go() {
    setBusy(true);
    try {
      await confirm();
      onDone();
      onClose();
    } catch (error) {
      toastError("Nothing was changed", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>
            {description ?? "The plan comes first. Nothing is written until you confirm."}
          </DialogDescription>
        </DialogHeader>
        {plan.error && <p role="alert">{plan.error}</p>}
        {!plan.data && !plan.error && <p className="text-meta">Preparing the plan…</p>}
        {plan.data && <PlanView plan={plan.data.plan} />}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant={destructive ? "destructive" : "default"}
            data-confirm
            autoFocus
            disabled={!plan.data || busy}
            onClick={() => void go()}
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
