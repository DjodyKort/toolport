import { useState } from "react";
import { Button } from "@/components/ui/button";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { setupUndo, setupUndoPreview, spendStatus } from "./commands";
import type { DataCounts, SetupUndoData } from "./data";
import { count, megabytes, monthDay, profileName } from "./model";
import { DialogError, SpendDialog } from "./SpendDialog";

async function readUndo() {
  const [status, preview] = await Promise.all([
    spendStatus(),
    setupUndoPreview({ deleteData: true }),
  ]);
  return { status, data: preview.deleteData ?? null };
}

const hasData = (data: DataCounts | null) =>
  data !== null && (data.events > 0 || data.dailyLines > 0);

function Result({ result }: { result: SetupUndoData }) {
  return (
    <div className="flex flex-col gap-3 text-sm" data-slot="undo-result">
      <p>
        <b>Collecting is off.</b> The receiver is stopped; sessions that are still open
        keep exporting to a port nobody listens on, harmlessly, until restarted.
      </p>
      <ul className="flex flex-col gap-2">
        {result.profiles.map((profile) => (
          <li key={profile.profile} className="text-meta">
            <span className="break-all font-mono">{profileName(profile.profile)}</span>:{" "}
            {profile.restored.length} {profile.restored.length === 1 ? "key" : "keys"}{" "}
            removed
            {profile.keptChanged.length > 0 && (
              <>
                ; left because you changed{" "}
                {profile.keptChanged.length === 1 ? "it" : "them"} since setup:{" "}
                {profile.keptChanged.map((key, index) => (
                  <span key={key}>
                    {index > 0 && ", "}
                    <code className="font-mono">{key}</code>
                  </span>
                ))}
              </>
            )}
          </li>
        ))}
      </ul>
      <p className="text-meta text-muted-foreground">
        {result.deleted
          ? `Deleted ${count(result.deleted.events)} events and ${count(result.deleted.dailyLines)} daily lines.`
          : "The collected data is kept and Spend still shows it."}
      </p>
    </div>
  );
}

export interface UndoDialogProps {
  onClose: () => void;
  onUndone: (result: SetupUndoData) => void;
}

/** Undo is its own dialog: the keys Toolport wrote are removed, a key changed since is left,
 * and the collected data is kept unless the person deletes it on purpose. */
export function UndoDialog({ onClose, onUndone }: UndoDialogProps) {
  const read = useLoad(readUndo, "undo");
  const [erase, setErase] = useState(false);
  const [sure, setSure] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [result, setResult] = useState<SetupUndoData | null>(null);

  const data = read.data?.data ?? null;
  const since = read.data?.status.setup
    .map((s) => s.appliedAt)
    .sort()
    .at(0);
  const deleting = erase && hasData(data);

  async function undo() {
    setBusy(true);
    setFailure(null);
    try {
      const done = await setupUndo({ deleteData: deleting });
      setResult(done);
      onUndone(done);
    } catch (error) {
      setFailure(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <SpendDialog
      title="Undo spend collecting setup"
      description={
        since
          ? `Puts Claude Code's settings back as they were before ${monthDay(since)}.`
          : "Puts Claude Code's settings back as they were."
      }
      busy={busy}
      onClose={onClose}
      footer={
        result ? (
          <Button onClick={onClose}>Done</Button>
        ) : (
          <>
            <Button variant="outline" disabled={busy} onClick={onClose}>
              Cancel
            </Button>
            <Button
              variant={deleting ? "destructive" : "default"}
              disabled={!read.data || busy || (deleting && !sure)}
              onClick={() => void undo()}
            >
              {deleting ? "Undo setup and delete data" : "Undo setup"}
            </Button>
          </>
        )
      }
    >
      {read.error && !read.data && (
        <DialogError title="The setup could not be read" detail={read.error} />
      )}
      {!read.data && !read.error && (
        <p role="status" className="text-meta text-muted-foreground">
          Reading what was set up…
        </p>
      )}
      {failure && <DialogError title="Nothing was undone" detail={failure} />}
      {result && <Result result={result} />}
      {read.data && !result && (
        <div className="flex flex-col gap-4 text-sm">
          <section
            aria-label="Remove what Toolport wrote"
            className="flex flex-col gap-1"
          >
            <h3 className="font-semibold">1. Remove what Toolport wrote</h3>
            <ul className="text-meta text-muted-foreground">
              {read.data.status.setup.map((s) => (
                <li key={s.profile}>
                  The five keys in{" "}
                  <code className="break-all font-mono">{profileName(s.profile)}</code>{" "}
                  are removed.
                </li>
              ))}
            </ul>
            <p className="text-meta text-muted-foreground">
              Only keys Toolport wrote and that are unchanged are removed. A key you
              edited since is left and listed afterwards.
            </p>
          </section>
          <section aria-label="Stop the receiver" className="flex flex-col gap-1">
            <h3 className="font-semibold">2. Stop the receiver</h3>
            <p className="text-meta text-muted-foreground">
              Sessions that are still open keep exporting to a port nobody listens on,
              harmlessly, until restarted.
            </p>
          </section>
          <fieldset className="flex flex-col gap-2">
            <legend className="font-semibold">3. Collected data</legend>
            {data && hasData(data) ? (
              <>
                <label className="flex items-start gap-2 text-meta">
                  <input
                    type="radio"
                    name="undo-data"
                    checked={!erase}
                    onChange={() => {
                      setErase(false);
                      setSure(false);
                    }}
                    className="mt-0.5"
                  />
                  <span>
                    <b>Keep it</b> (default): {count(data.events)} events,{" "}
                    {megabytes(data.bytes)} MB, and the daily totals. Spend still shows
                    them.
                  </span>
                </label>
                <label className="flex items-start gap-2 text-meta">
                  <input
                    type="radio"
                    name="undo-data"
                    checked={erase}
                    onChange={() => setErase(true)}
                    className="mt-0.5"
                  />
                  <span>
                    <b>Delete it:</b> events and daily totals. Spend goes back to
                    transcripts only.
                  </span>
                </label>
                {erase && (
                  <label className="ml-6 flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-2 text-meta">
                    <input
                      type="checkbox"
                      checked={sure}
                      onChange={(event) => setSure(event.target.checked)}
                      className="mt-0.5"
                    />
                    <span>
                      I understand this deletes {count(data.events)} events and{" "}
                      {count(data.dailyLines)} daily lines for good.
                    </span>
                  </label>
                )}
              </>
            ) : (
              <p className="text-meta text-muted-foreground">
                No spend data is stored, so there is nothing to keep or delete.
              </p>
            )}
          </fieldset>
        </div>
      )}
    </SpendDialog>
  );
}
