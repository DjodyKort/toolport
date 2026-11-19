import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { setupApply, setupPlan } from "./commands";
import type { PlanEntry, PlanProfile, SetupApplyData } from "./data";
import { profileName, tildePath, weeklySchedule } from "./model";
import {
  chosenProfiles,
  CONTENT_FLAGS,
  conflictKeys,
  conflictValue,
  diffLines,
  selectedProfiles,
  writeCount,
} from "./plan";
import { DialogError, SpendDialog } from "./SpendDialog";

const MARK = { "+": "text-success", "-": "text-destructive", " ": "" } as const;

function Diff({ entries, replace }: { entries: PlanEntry[]; replace: string[] }) {
  return (
    <pre className="overflow-x-auto rounded-md bg-muted p-2 font-mono text-meta leading-5">
      {entries.flatMap((entry) =>
        diffLines(entry, replace.includes(entry.key)).map((line, index) => (
          <div key={`${entry.key}-${index}`} className={MARK[line.sign]}>
            {line.sign} {line.text}
            {line.note && (
              <span className="text-muted-foreground">{`   (${line.note})`}</span>
            )}
          </div>
        )),
      )}
    </pre>
  );
}

export function ProfileDiffs({
  profiles,
  replace,
}: {
  profiles: PlanProfile[];
  replace: string[];
}) {
  return (
    <>
      {profiles.map((profile) => {
        const differs = profile.entries.filter((e) => e.action === "conflict").length;
        return (
          <div key={profile.profile} className="flex flex-col gap-1">
            <span className="break-all font-mono text-meta text-muted-foreground">
              {profileName(profile.profile)} · env
              {differs > 0 &&
                ` · ${differs} of ${profile.entries.length} keys ${differs === 1 ? "differs" : "differ"}`}
              {!profile.exists && " · file will be created"}
            </span>
            <Diff entries={profile.entries} replace={replace} />
          </div>
        );
      })}
    </>
  );
}

function ConflictChoice({
  name,
  current,
  replace,
  onChange,
}: {
  name: string;
  current: string;
  replace: boolean;
  onChange: (replace: boolean) => void;
}) {
  return (
    <fieldset className="flex flex-col gap-1.5 rounded-lg border p-3 text-meta">
      <legend className="px-1 font-mono">{name}</legend>
      <span className="text-muted-foreground">
        Already set to <code className="font-mono">{current}</code>.
      </span>
      <label className="flex items-start gap-2">
        <input
          type="radio"
          name={`choice-${name}`}
          checked={!replace}
          onChange={() => onChange(false)}
          className="mt-0.5"
        />
        <span>
          <b>Skip this key</b> (default). Claude Code keeps using your value, so Toolport
          receives nothing and Spend stays on transcripts.
        </span>
      </label>
      <label className="flex items-start gap-2">
        <input
          type="radio"
          name={`choice-${name}`}
          checked={replace}
          onChange={() => onChange(true)}
          className="mt-0.5"
        />
        <span>
          <b>Replace it.</b> Your value is kept in the backup and comes back with Undo.
          Whatever used it stops receiving Claude Code data.
        </span>
      </label>
    </fieldset>
  );
}

export interface SetupDialogProps {
  onClose: () => void;
  onApplied: (result: SetupApplyData, settingsWritten: number) => void;
}

/** The plan to turn collecting on: what runs, what is written where, what is never turned on,
 * and a choice for every key that already holds another value. Nothing is written before
 * Apply, and Apply is not the focused button. */
export function SetupDialog({ onClose, onApplied }: SetupDialogProps) {
  const plan = useLoad(() => setupPlan(), "plan");
  const [off, setOff] = useState<string[]>([]);
  const [replace, setReplace] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [weekly, setWeekly] = useState(false);
  const [weeklyFolder, setWeeklyFolder] = useState<string | null>(null);

  const data = plan.data;
  const measure = data?.weeklyMeasure;
  const folder = weeklyFolder ?? measure?.folder ?? "";
  const chosen = data ? selectedProfiles(data.profiles, off) : [];
  const keys = conflictKeys(chosen);
  const writes = writeCount(chosen, replace) + (weekly ? 1 : 0);

  async function apply() {
    if (!data) return;
    setBusy(true);
    setFailure(null);
    try {
      const result = await setupApply({
        profiles: chosenProfiles(data.profiles, off),
        overwrite: replace.filter((key) => keys.includes(key)),
        ...(weekly && { weeklyMeasure: true, measureFolder: folder }),
      });
      onApplied(result, writes);
    } catch (error) {
      setFailure(errorText(error));
      plan.reload();
    } finally {
      setBusy(false);
    }
  }

  return (
    <SpendDialog
      title="Turn on spend collecting"
      description="Nothing is written until you press Apply. Each step can be undone."
      busy={busy}
      onClose={onClose}
      footer={
        <>
          <span className="mr-auto text-meta text-muted-foreground">
            CLI: <code className="font-mono">toolportctl spend setup plan</code>
          </span>
          <Button variant="outline" disabled={busy} onClick={onClose}>
            Cancel
          </Button>
          <Button
            disabled={
              !data || busy || chosen.length === 0 || (weekly && folder.trim() === "")
            }
            onClick={() => void apply()}
          >
            {failure ? "Try again" : "Apply"}
            {writes > 0 && ` ${writes} ${writes === 1 ? "change" : "changes"}`}
          </Button>
        </>
      }
    >
      {plan.error && !data && (
        <DialogError title="The plan could not be made" detail={plan.error} />
      )}
      {!data && !plan.error && (
        <p role="status" className="text-meta text-muted-foreground">
          Preparing the plan…
        </p>
      )}
      {failure && <DialogError title="The plan was not applied" detail={failure} />}
      {data && (
        <div className="flex flex-col gap-4 text-sm">
          <section aria-label="Start the receiver" className="flex flex-col gap-1">
            <h3 className="font-semibold">1. Start the receiver in the Toolport app</h3>
            <p className="text-meta text-muted-foreground">
              Listens on <code className="font-mono">127.0.0.1:{data.port}</code>, this
              computer only; it is never reachable from the network. Needs the app
              running; closed days are filled from transcripts. Events are stored in{" "}
              <code className="font-mono">&lt;data dir&gt;/obs/events.jsonl</code> and
              pruned at the retention you set in Settings.
            </p>
          </section>
          <section
            aria-label="Tell Claude Code where to send its numbers"
            className="flex flex-col gap-2"
          >
            <h3 className="font-semibold">
              2. Tell Claude Code where to send its numbers
            </h3>
            <fieldset className="flex flex-col gap-1 text-meta">
              <legend className="mb-1">Applies to:</legend>
              {data.profiles.map((profile) => (
                <label key={profile.profile} className="flex items-center gap-1.5">
                  <input
                    type="checkbox"
                    checked={!off.includes(profile.profile)}
                    onChange={(event) =>
                      setOff((list) =>
                        event.target.checked
                          ? list.filter((p) => p !== profile.profile)
                          : [...list, profile.profile],
                      )
                    }
                  />
                  <span className="font-mono">{profileName(profile.profile)}</span>
                </label>
              ))}
            </fieldset>
            {chosen.length === 0 && (
              <p role="alert" className="text-meta text-destructive">
                Choose at least one profile.
              </p>
            )}
            <ProfileDiffs profiles={chosen} replace={replace} />
            {keys.map((key) => (
              <ConflictChoice
                key={key}
                name={key}
                current={conflictValue(chosen, key)}
                replace={replace.includes(key)}
                onChange={(on) =>
                  setReplace((list) =>
                    on ? [...list, key] : list.filter((k) => k !== key),
                  )
                }
              />
            ))}
            <p className="text-meta text-muted-foreground">
              Written with Toolport's settings writer: comments and your other keys stay,
              and a backup is saved next to each settings file.
            </p>
            <div className="rounded-lg border bg-muted/30 p-3 text-meta">
              <b>Never turned on.</b>{" "}
              {CONTENT_FLAGS.map((flag, index) => (
                <span key={flag}>
                  {index > 0 && ", "}
                  <code className="font-mono">{flag}</code>
                </span>
              ))}{" "}
              stay unset. Prompts, tool arguments and results are not exported, so they
              cannot be stored.
              <div className="mt-1 text-muted-foreground">
                <b>Stored per event:</b> time, session id, model, token counts, cost, tool
                and MCP server name, working folder. Long text values are cut and
                redacted.
              </div>
            </div>
          </section>
          {measure && (
            <section
              aria-label="Measure my context every week"
              className="flex flex-col gap-2"
            >
              <h3 className="font-semibold">
                3. Optional: measure my context every week
              </h3>
              {measure.enabled && measure.folder ? (
                <p className="text-meta text-muted-foreground">
                  Already on for{" "}
                  <code className="font-mono">{tildePath(measure.folder ?? "")}</code>.
                </p>
              ) : (
                <>
                  <label className="flex items-start gap-2 text-meta">
                    <input
                      type="checkbox"
                      checked={weekly}
                      onChange={(event) => setWeekly(event.target.checked)}
                      className="mt-0.5"
                    />
                    <span>
                      Add a weekly task <code className="font-mono">{measure.task}</code>.{" "}
                      {weeklySchedule(measure.cron)}, one small request to the{" "}
                      {measure.model} model; it makes growth visible. Applying adds it and
                      switches it on.
                    </span>
                  </label>
                  {weekly && (
                    <label className="flex flex-col gap-1 text-meta">
                      <span className="font-semibold text-foreground">
                        Folder to measure
                      </span>
                      <Input
                        aria-label="Folder to measure"
                        className="font-mono"
                        value={folder}
                        onChange={(event) => setWeeklyFolder(event.target.value)}
                      />
                    </label>
                  )}
                </>
              )}
            </section>
          )}
          <p className="text-meta">
            <b>Takes effect for new Claude Code sessions.</b> Sessions already open keep
            their old settings until restarted.
          </p>
        </div>
      )}
    </SpendDialog>
  );
}
