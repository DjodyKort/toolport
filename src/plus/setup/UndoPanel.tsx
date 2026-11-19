import { useState } from "react";
import { CircleCheck, CircleX, Info, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { SectionHeader } from "@/components/ui/section-header";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { ctlData } from "../bridge/ctl";
import { errorText } from "../settings/useCtl";
import { plural, undoArgv, type UndoData, type UndoResult } from "./model";

const RESULT_LABEL: Record<UndoResult, string> = {
  done: "Put back",
  kept: "Kept",
  failed: "Could not",
};

function StepList({ data }: { data: UndoData }) {
  return (
    <ul className="toolport-list" aria-label="Undo steps">
      {data.steps.map((step) => (
        <li key={step.what} className="flex items-start gap-3 px-4 py-2 text-sm">
          <span className="min-w-0 flex-1">
            <span className="break-all">{step.what}</span>
            <span className="block text-meta text-muted-foreground">{step.note}</span>
          </span>
          <span
            className={
              step.result === "failed"
                ? "text-meta font-medium text-destructive"
                : "text-meta text-muted-foreground"
            }
          >
            {RESULT_LABEL[step.result]}
          </span>
        </li>
      ))}
    </ul>
  );
}

/** Undo of a restore: what it would do first, then the result per step. A step that could not be
 * done keeps the run open, and trying again does only what is left. */
export function UndoPanel({ run }: { run: string | null }) {
  const [plan, setPlan] = useState<UndoData | null>(null);
  const [result, setResult] = useState<UndoData | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  async function show() {
    setBusy(true);
    setFailure(null);
    try {
      setResult(null);
      setPlan(await ctlData<UndoData>(undoArgv(run, true)));
    } catch (error) {
      setFailure(errorText(error));
      toastError("Could not undo", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  async function undo(target: string) {
    setBusy(true);
    setFailure(null);
    try {
      const data = await ctlData<UndoData>(undoArgv(target, false));
      setPlan(null);
      setResult(data);
      if (data.undone) toast.success("Restore undone");
    } catch (error) {
      setFailure(errorText(error));
      toastError("Could not undo", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  const open = result && !result.undone ? result : null;
  return (
    <section aria-label="Undo" className="flex flex-col gap-3">
      {failure && (
        <p role="alert" className="text-meta text-destructive">
          Could not undo: {failure}
        </p>
      )}
      {!plan && !result && (
        <div>
          <Button variant="outline" disabled={busy} onClick={() => void show()}>
            {run ? "Undo this restore" : "Undo the last restore"}
          </Button>
        </div>
      )}
      {plan && (
        <>
          <SectionHeader>What Undo would do</SectionHeader>
          <StepList data={plan} />
          {plan.steps.some((s) => s.result === "failed") && (
            <p role="alert" className="text-meta text-destructive">
              {plural(plan.failed.length, "step cannot", "steps cannot")} be undone as
              things stand.
            </p>
          )}
          <p className="text-meta text-muted-foreground">
            A file you changed after the restore is kept as it is. {plan.left.note}.
          </p>
          <div className="flex flex-wrap gap-2">
            <Button disabled={busy} onClick={() => void undo(plan.run)}>
              Undo
            </Button>
            <Button variant="outline" disabled={busy} onClick={() => setPlan(null)}>
              Cancel
            </Button>
          </div>
        </>
      )}
      {result && (
        <>
          <StatusCard
            variant={result.undone ? "success" : "warning"}
            icon={result.undone ? <CircleCheck /> : <TriangleAlert />}
            title={
              result.undone
                ? "Restore undone"
                : `Not everything was undone: ${plural(result.failed.length, "step", "steps")} failed`
            }
            detail={
              result.undone
                ? "Every step is put back or was kept on purpose."
                : "The restore stays open. Trying again does only the steps that are left."
            }
          />
          <StepList data={result} />
          {result.left.servers.length > 0 && (
            <p className="flex items-start gap-2 text-meta text-muted-foreground">
              <Info className="mt-0.5 size-4 shrink-0" />
              <span>
                {result.left.note}: {result.left.servers.join(", ")}.
              </span>
            </p>
          )}
          {open && (
            <div className="flex flex-wrap gap-2">
              <Button disabled={busy} onClick={() => void undo(open.run)}>
                <CircleX /> Try the failed steps again
              </Button>
            </div>
          )}
        </>
      )}
    </section>
  );
}
