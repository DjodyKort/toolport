import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { previewConfig, setConfig } from "./commands";
import type { ConfigSetData } from "./data";
import { weeklySchedule } from "./model";
import { DialogError, SpendDialog } from "./SpendDialog";

export interface WeeklyMeasureDialogProps {
  onClose: () => void;
  onTurnedOn: (result: ConfigSetData) => void;
}

/** Turning the weekly measure on enables a task that then runs on its own, so the person sees
 * what it is and for which folder first. Turn on is the approval. */
export function WeeklyMeasureDialog({ onClose, onTurnedOn }: WeeklyMeasureDialogProps) {
  const preview = useLoad(() => previewConfig({ weeklyMeasure: true }), "preview");
  const [typed, setTyped] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const plan = preview.data?.weeklyMeasure;
  const folder = typed ?? plan?.folder ?? "";

  async function turnOn() {
    setBusy(true);
    setFailure(null);
    try {
      onTurnedOn(await setConfig({ weeklyMeasure: true, measureFolder: folder }));
    } catch (error) {
      setFailure(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <SpendDialog
      title="Measure my context every week"
      description="Adds a weekly task that measures what a session starts with, so Spend can show how it drifts. Turning it on asks for your approval here."
      busy={busy}
      onClose={onClose}
      footer={
        <>
          <span className="mr-auto text-meta text-muted-foreground">
            CLI:{" "}
            <code className="font-mono">
              toolportctl spend config set --weekly-measure on
            </code>
          </span>
          <Button variant="outline" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button
            disabled={!plan || busy || folder.trim() === ""}
            onClick={() => void turnOn()}
          >
            Turn on
          </Button>
        </>
      }
    >
      {preview.error && !plan && (
        <DialogError title="The plan could not be made" detail={preview.error} />
      )}
      {!plan && !preview.error && (
        <p role="status" className="text-meta text-muted-foreground">
          Preparing the plan…
        </p>
      )}
      {failure && (
        <DialogError title="The weekly measure was not turned on" detail={failure} />
      )}
      {plan && (
        <div className="flex flex-col gap-3 text-sm">
          <label className="flex flex-col gap-1 text-meta">
            <span className="font-semibold text-foreground">Folder to measure</span>
            <Input
              aria-label="Folder to measure"
              className="font-mono"
              value={folder}
              onChange={(event) => setTyped(event.target.value)}
            />
            <span className="text-muted-foreground">
              The full path of one project folder. The task measures that folder only.
            </span>
          </label>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-meta">
            <dt className="text-muted-foreground">Task</dt>
            <dd>
              <code className="font-mono">{plan.task}</code>, which runs{" "}
              <code className="font-mono">context measure</code>
            </dd>
            <dt className="text-muted-foreground">When</dt>
            <dd>{weeklySchedule(plan.cron)}</dd>
            <dt className="text-muted-foreground">Cost</dt>
            <dd>
              One small request to the <b>{plan.model}</b> model each time.
            </dd>
            <dt className="text-muted-foreground">Undo</dt>
            <dd>
              Switch the weekly measure off in Settings, or disable the task in Tasks.
            </dd>
          </dl>
        </div>
      )}
    </SpendDialog>
  );
}
