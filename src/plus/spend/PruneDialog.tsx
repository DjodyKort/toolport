import { useState } from "react";
import { Button } from "@/components/ui/button";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { deleteTotals, pruneNow, prunePreview } from "./commands";
import type { PruneData } from "./data";
import { count, megabytes } from "./model";
import { DialogError, SpendDialog } from "./SpendDialog";

export interface PruneDialogProps {
  onClose: () => void;
  onPruned: (result: PruneData) => void;
}

/** Prune now shows exactly what goes and what stays before it runs. Events only, unless the
 * person also deletes the daily totals, which removes all history from the page. */
export function PruneDialog({ onClose, onPruned }: PruneDialogProps) {
  const preview = useLoad(prunePreview, "prune");
  const [totals, setTotals] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const data = preview.data;
  const nothing = data !== null && data.removedEvents === 0 && !totals;

  async function run() {
    setBusy(true);
    setFailure(null);
    try {
      onPruned(await (totals ? deleteTotals() : pruneNow()));
    } catch (error) {
      setFailure(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <SpendDialog
      title="Prune events"
      description="Removes stored OTel events. Nothing else is touched."
      busy={busy}
      onClose={onClose}
      footer={
        <>
          <Button variant="outline" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant={totals ? "destructive" : "default"}
            disabled={!data || busy || nothing}
            onClick={() => void run()}
          >
            {totals
              ? "Prune events and delete totals"
              : data
                ? `Prune ${count(data.removedEvents)} events`
                : "Prune"}
          </Button>
        </>
      }
    >
      {preview.error && !data && (
        <DialogError title="The preview could not be made" detail={preview.error} />
      )}
      {!data && !preview.error && (
        <p role="status" className="text-meta text-muted-foreground">
          Counting what would go…
        </p>
      )}
      {failure && <DialogError title="Nothing was pruned" detail={failure} />}
      {data && (
        <div className="flex flex-col gap-3 text-sm">
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-meta">
            <dt className="text-muted-foreground">Goes</dt>
            <dd>
              {count(data.removedEvents)} events, {megabytes(data.removedBytes)} MB: older
              than {data.retentionDays} days or over the{" "}
              {Math.round(data.maxBytes / 1048576)} MB cap.
            </dd>
            <dt className="text-muted-foreground">Stays</dt>
            <dd>
              {count(data.keptEvents)} events, {megabytes(data.keptBytes)} MB.
            </dd>
            <dt className="text-muted-foreground">Folded</dt>
            <dd>
              {data.foldedDays} {data.foldedDays === 1 ? "day goes" : "days go"} into the
              daily totals first, so the page keeps its history.
            </dd>
          </dl>
          {data.removedEvents === 0 && (
            <p className="text-meta text-muted-foreground">
              No event is older than the retention or over the size cap: nothing to prune.
            </p>
          )}
          <label className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-2 text-meta">
            <input
              type="checkbox"
              checked={totals}
              onChange={(event) => setTotals(event.target.checked)}
              className="mt-0.5"
            />
            <span>
              Also delete the daily totals (<b>removes all history from this page</b>).
              This cannot be undone.
            </span>
          </label>
        </div>
      )}
    </SpendDialog>
  );
}
