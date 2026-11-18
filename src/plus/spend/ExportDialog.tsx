import { useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { now } from "../tasks/clock";
import { exportCsv } from "./commands";
import type { ExportCsvData, RangeId } from "./data";
import { count, RANGES } from "./model";
import { DialogError, SpendDialog } from "./SpendDialog";

const COLUMNS =
  "date, folder, model, task, requests, input, output, cache_read, cache_write, cost, source (otel or transcript)";

export interface ExportDialogProps {
  range: RangeId;
  /** The page filters the report; the CSV covers every folder, model and task. */
  filtered?: boolean;
  onClose: () => void;
  onExported: (result: ExportCsvData & { out: string }) => void;
}

/** Export CSV: pick the range, see how many rows it has, then choose where the file goes. */
export function ExportDialog({
  range,
  filtered,
  onClose,
  onExported,
}: ExportDialogProps) {
  const [picked, setPicked] = useState<RangeId>(range);
  const rows = useLoad(() => exportCsv(picked), picked);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const name = `toolport-spend-${new Date(now()).toISOString().slice(0, 10)}.csv`;
  const total = rows.data?.rows ?? null;

  async function run() {
    setBusy(true);
    setFailure(null);
    try {
      const path = await save({
        title: "Export the spend as CSV",
        defaultPath: name,
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      if (!path) return;
      const done = await exportCsv(picked, path);
      onExported({ ...done, out: done.out ?? path });
    } catch (error) {
      setFailure(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <SpendDialog
      title="Export CSV"
      description="One row per day, project folder, model and task."
      busy={busy}
      onClose={onClose}
      footer={
        <>
          <Button variant="outline" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={total === null || busy} onClick={() => void run()}>
            {total === null
              ? "Export"
              : `Export ${count(total)} ${total === 1 ? "row" : "rows"}`}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3 text-sm">
        <fieldset className="flex flex-wrap items-center gap-x-4 gap-y-1 text-meta">
          <legend className="float-left mr-3">Range</legend>
          {RANGES.map((r) => (
            <label key={r.id} className="flex items-center gap-1.5">
              <input
                type="radio"
                name="export-range"
                checked={picked === r.id}
                onChange={() => setPicked(r.id)}
              />
              {r.label}
            </label>
          ))}
        </fieldset>
        {rows.error && !rows.data && (
          <DialogError title="The rows could not be counted" detail={rows.error} />
        )}
        {failure && <DialogError title="Nothing was exported" detail={failure} />}
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-meta">
          <dt className="text-muted-foreground">Rows</dt>
          <dd aria-live="polite">
            {total === null ? "Counting…" : count(total)} · columns:{" "}
            <span className="font-mono">{COLUMNS}</span>
          </dd>
          <dt className="text-muted-foreground">Not included</dt>
          <dd>Prompts, tool arguments, results. Nothing like that is stored.</dd>
          <dt className="text-muted-foreground">Saves to</dt>
          <dd>
            A place you choose when you press Export; suggested name{" "}
            <code className="font-mono">{name}</code>.
          </dd>
          {filtered && (
            <>
              <dt className="text-muted-foreground">Filters</dt>
              <dd>
                The filter on the page does not narrow the file: it covers every folder,
                model and task of the range.
              </dd>
            </>
          )}
        </dl>
      </div>
    </SpendDialog>
  );
}
