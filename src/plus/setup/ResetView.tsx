import { useCallback, useEffect, useState } from "react";
import { CircleCheck, Copy, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { SectionHeader } from "@/components/ui/section-header";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { ctlData } from "../bridge/ctl";
import { errorText } from "../settings/useCtl";
import {
  ageInDays,
  ageText,
  PART_LABEL,
  plural,
  sinceLine,
  size,
  type CheckData,
  type CheckFinding,
} from "./model";

function copy(text: string) {
  void navigator.clipboard?.writeText(text).then(
    () => toast.success("Copied"),
    () =>
      toastError("Could not copy", { description: "The clipboard is not available." }),
  );
}

function Finding({
  finding,
  red,
  onAck,
}: {
  finding: CheckFinding;
  red: boolean;
  onAck: (id: string) => void;
}) {
  return (
    <li className="flex flex-col gap-1 px-4 py-2 text-sm">
      <div className="flex items-start gap-3">
        <span className="min-w-0 flex-1">
          <span className="break-all font-medium">{finding.repo}</span>
          <span className="block text-meta text-muted-foreground">{finding.note}</span>
        </span>
        {!finding.acked && !red && (
          <Button variant="outline" size="sm" onClick={() => onAck(finding.id)}>
            Acknowledge
          </Button>
        )}
        {finding.acked && (
          <span className="text-meta text-muted-foreground">Acknowledged</span>
        )}
      </div>
      {finding.commands.length > 0 && (
        <div className="flex items-start gap-2">
          <pre className="min-w-0 flex-1 overflow-x-auto rounded-md bg-muted p-2 text-meta whitespace-pre-wrap break-all">
            {finding.commands.join("\n")}
          </pre>
          <Button
            variant="outline"
            size="sm"
            aria-label={`Copy the commands for ${finding.repo}, ${finding.kind}`}
            onClick={() => copy(finding.commands.join("\n"))}
          >
            <Copy /> Copy
          </Button>
        </div>
      )}
    </li>
  );
}

/** What the last export covers, how old it is, and what changed on this Mac since. */
function Covered({
  check,
  now,
  onExport,
}: {
  check: CheckData;
  now: number;
  onExport: () => void;
}) {
  const last = check.lastExport;
  const days = last ? ageInDays(last.madeAt, now) : null;
  return (
    <section aria-label="Covered by the export" className="flex flex-col gap-2">
      <SectionHeader>Covered by the export</SectionHeader>
      {!last ? (
        <StatusCard
          variant="info"
          icon={<TriangleAlert />}
          title="No export yet"
          detail="The checklist works without one. Make the first export to see what a file would cover."
          actions={
            <Button variant="outline" size="sm" onClick={onExport}>
              Make the first export
            </Button>
          }
        />
      ) : (
        <>
          <p className="text-meta text-muted-foreground">
            In <span className="break-all">{last.file}</span>
            {days !== null && <>, {ageText(days)}</>} · {size(last.bytes)}.
          </p>
          <ul className="toolport-list" aria-label="What the export covers">
            {Object.entries(last.parts).map(([id, part]) => (
              <li key={id} className="flex items-center gap-3 px-4 py-2 text-sm">
                <span className="min-w-0 flex-1">{PART_LABEL[id] ?? id}</span>
                <span className="text-meta text-muted-foreground">
                  {id === "secrets"
                    ? `${plural(part.count, "value", "values")}, encrypted`
                    : id === "history"
                      ? `${plural(part.count, "session", "sessions")}, encrypted`
                      : part.count}
                </span>
              </li>
            ))}
          </ul>
          {check.since ? (
            <p className="text-meta text-muted-foreground">
              {sinceLine(check.since.parts)}
            </p>
          ) : (
            <p className="text-meta text-muted-foreground">
              What changed since is read when you press Check again.
            </p>
          )}
          {check.since && check.since.parts.length > 0 && (
            <Button variant="outline" size="sm" onClick={onExport}>
              Export again
            </Button>
          )}
        </>
      )}
    </section>
  );
}

/** The checklist before a reset: work that exists only here, what the export covers, what you carry. */
export function ResetView({
  onExport,
  onExportSessions,
}: {
  onExport: () => void;
  onExportSessions: () => void;
}) {
  const [check, setCheck] = useState<CheckData | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [checkedAt, setCheckedAt] = useState(() => Date.now());

  const load = useCallback(async (extra: string[] = []) => {
    setBusy(true);
    try {
      const data = await ctlData<CheckData>(["setup", "check", ...extra]);
      setCheckedAt(Date.now());
      setCheck(data);
      setLoadError(null);
    } catch (error) {
      setLoadError(errorText(error));
      toastError("Could not check the repositories", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- Runs the check once when the view opens.
    void load();
  }, [load]);

  if (!check && loadError)
    return (
      <div className="flex flex-col items-start gap-2">
        <p role="alert" className="text-meta text-destructive">
          Could not check the repositories: {loadError}
        </p>
        <Button variant="outline" size="sm" onClick={() => void load()}>
          Try again
        </Button>
      </div>
    );
  if (!check) return <p className="text-meta text-muted-foreground">Checking…</p>;
  const findings = [
    ...check.blockers.map((finding) => ({ finding, red: true })),
    ...check.amber.map((finding) => ({ finding, red: false })),
  ];
  const sessions = check.sessions;
  return (
    <div className="flex flex-col gap-4">
      <StatusCard
        variant={check.ready ? "success" : "warning"}
        icon={check.ready ? <CircleCheck /> : <TriangleAlert />}
        title={check.ready ? "Ready for a reset" : "Not ready for a reset"}
        detail={`Checked ${plural(check.repos, "repository", "repositories")} just now. ${plural(check.open.blockers, "blocker", "blockers")} and ${plural(check.open.carry, "thing", "things")} to carry over are open. Toolport does not wipe anything; it tells you what a wipe would cost.`}
        actions={
          <>
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() => void load()}
            >
              Check again
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="h-auto min-h-7 max-w-full shrink py-1 text-left whitespace-normal"
              disabled={busy}
              onClick={() => void load(["--since"])}
            >
              Check what changed since the export
            </Button>
          </>
        }
      />
      <section aria-label="Blockers" className="flex flex-col gap-2">
        <SectionHeader count={findings.length}>
          Work that exists only on this Mac
        </SectionHeader>
        {findings.length === 0 ? (
          <p className="text-meta text-muted-foreground">
            All {plural(check.repos, "repository", "repositories")} are clean and pushed.
          </p>
        ) : (
          <ul className="toolport-list">
            {findings.map(({ finding, red }) => (
              <Finding
                key={finding.id}
                finding={finding}
                red={red}
                onAck={(id) => void load(["--ack", id])}
              />
            ))}
          </ul>
        )}
      </section>
      <section aria-label="Sessions" className="flex flex-col gap-2">
        <SectionHeader>Claude Code history</SectionHeader>
        {sessions.count === 0 ? (
          <p className="text-meta text-muted-foreground">
            No past conversations were found for these repositories.
          </p>
        ) : (
          <>
            <p className="text-meta text-muted-foreground">
              {plural(sessions.count, "session", "sessions")} in{" "}
              {plural(sessions.repos.length, "repository", "repositories")},{" "}
              {size(sessions.fileSize)}. They go with the wipe unless you tick Sessions in
              the export (off by default, always encrypted, per repository). Claude Code
              also deletes old sessions on its own.
            </p>
            <div>
              <Button variant="outline" size="sm" onClick={onExportSessions}>
                Choose sessions to export
              </Button>
            </div>
          </>
        )}
      </section>
      <Covered check={check} now={checkedAt} onExport={onExport} />
      <section aria-label="Carry over" className="flex flex-col gap-2">
        <SectionHeader>Carry over yourself</SectionHeader>
        <ul className="toolport-list">
          {check.carry.map((item) => (
            <li key={item.id} className="flex items-center gap-3 px-4 py-2">
              <input
                type="checkbox"
                id={`carry-${item.id}`}
                checked={item.ticked}
                onChange={(e) =>
                  void load([e.target.checked ? "--tick" : "--untick", item.id])
                }
              />
              <label htmlFor={`carry-${item.id}`} className="text-sm">
                {item.label}
              </label>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
