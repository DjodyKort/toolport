import { useId, useMemo, useState } from "react";
import { CircleCheck, Info, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SectionHeader } from "@/components/ui/section-header";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { CtlError, ctlData } from "../bridge/ctl";
import { errorText } from "../settings/useCtl";
import {
  PART_LABEL,
  plural,
  restoreArgv,
  STATE_LABEL,
  type InspectData,
  type InspectPart,
  type RestoreData,
  type RestoreRow,
  type RowState,
} from "./model";
import { UndoPanel } from "./UndoPanel";

const SHOWN = 6;
const SUMMARY: { state: RowState; title: string; hint: string }[] = [
  { state: "add", title: "To add", hint: "new here" },
  { state: "here", title: "Already here", hint: "left alone" },
  { state: "conflict", title: "Conflicts", hint: "yours is kept unless you choose" },
  { state: "needs", title: "Need you after", hint: "sign-in or secret" },
  { state: "skip", title: "Skipped", hint: "not restored" },
];
/** Sealed with a passphrase; sessions are in the file but a restore does not bring them back yet. */
const SECRETS = "secrets";
const SESSIONS = "history";

function standingNote(part: InspectPart): string | null {
  if (part.standing === "damaged") return "does not match its hash: left out";
  if (part.standing === "missing") return "is listed but not in the file";
  if (part.standing === "unknown") return "is from a newer Toolport and is skipped";
  if (part.id === SESSIONS)
    return "stays in the file: restoring sessions is not built yet";
  return null;
}

function PartPicker({
  info,
  chosen,
  onChange,
}: {
  info: InspectData;
  chosen: string[];
  onChange: (next: string[]) => void;
}) {
  return (
    <section aria-label="Parts to restore" className="flex flex-col gap-2">
      <SectionHeader>Parts to restore</SectionHeader>
      <ul className="toolport-list">
        {info.parts.map((part) => {
          const note = standingNote(part);
          const usable = part.standing === "ok" && part.id !== SESSIONS;
          const id = `restore-part-${part.id}`;
          return (
            <li key={part.id} className="flex items-start gap-3 px-4 py-2">
              <input
                type="checkbox"
                id={id}
                className="mt-0.5 shrink-0"
                disabled={!usable}
                checked={usable && chosen.includes(part.id)}
                aria-labelledby={`${id}-label`}
                onChange={(e) =>
                  onChange(
                    e.target.checked
                      ? [...chosen, part.id]
                      : chosen.filter((p) => p !== part.id),
                  )
                }
              />
              <label htmlFor={id} className="min-w-0 flex-1 text-sm">
                <span id={`${id}-label`}>{part.label}</span>
                <span className="block text-meta text-muted-foreground">
                  {part.sealed ? "encrypted" : plural(part.count, "item", "items")}
                  {note ? ` · ${note}` : ""}
                </span>
              </label>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function ConflictChoice({
  row,
  taken,
  onChange,
}: {
  row: RestoreRow;
  taken: boolean;
  onChange: (take: boolean) => void;
}) {
  const name = useId();
  return (
    <fieldset className="mt-1 flex flex-wrap gap-x-4 gap-y-1">
      <legend className="sr-only">{`Choose for ${row.label}`}</legend>
      <label className="flex items-center gap-1.5 text-meta">
        <input
          type="radio"
          name={name}
          checked={!taken}
          aria-label={`Keep mine: ${row.label}`}
          onChange={() => onChange(false)}
        />
        Keep mine
      </label>
      <label className="flex items-center gap-1.5 text-meta">
        <input
          type="radio"
          name={name}
          checked={taken}
          aria-label={`Use the file's: ${row.label}`}
          onChange={() => onChange(true)}
        />
        Use the file&rsquo;s
      </label>
    </fieldset>
  );
}

function PlanGroup({
  part,
  rows,
  take,
  onTake,
}: {
  part: string;
  rows: RestoreRow[];
  take: string[];
  onTake: (id: string, on: boolean) => void;
}) {
  const [all, setAll] = useState(false);
  // Conflicts first: they are the rows that wait for a choice.
  const sorted = [...rows].sort(
    (a, b) => Number(b.state === "conflict") - Number(a.state === "conflict"),
  );
  const shown = all ? sorted : sorted.slice(0, SHOWN);
  return (
    <section aria-label={PART_LABEL[part] ?? part}>
      <SectionHeader count={rows.length}>{PART_LABEL[part] ?? part}</SectionHeader>
      <ul className="toolport-list">
        {shown.map((row) => (
          <li key={row.id} className="flex items-start gap-3 px-4 py-2 text-sm">
            <span className="min-w-0 flex-1">
              <span className="break-all">{row.label}</span>
              <span className="block text-meta text-muted-foreground">{row.note}</span>
              {row.state === "conflict" && row.takeable && (
                <ConflictChoice
                  row={row}
                  taken={take.includes(row.id)}
                  onChange={(on) => onTake(row.id, on)}
                />
              )}
            </span>
            <span className="text-meta">
              {row.refused ? "Refused" : STATE_LABEL[row.state]}
            </span>
          </li>
        ))}
      </ul>
      {rows.length > SHOWN && (
        <Button variant="link" size="sm" onClick={() => setAll(!all)}>
          {all ? "Show fewer" : `Show all ${rows.length}`}
        </Button>
      )}
    </section>
  );
}

function Summary({ plan }: { plan: RestoreData }) {
  return (
    <div className="grid grid-cols-2 gap-2 @sm:grid-cols-3" aria-label="Plan summary">
      {SUMMARY.map(({ state, title, hint }) => (
        <div key={state} className="rounded-lg border p-3">
          <div className="text-meta text-muted-foreground">{title}</div>
          <div className="text-lg font-semibold">{plan.summary[state] ?? 0}</div>
          <div className="text-meta text-muted-foreground">{hint}</div>
        </div>
      ))}
    </div>
  );
}

function Result({ done, onReplan }: { done: RestoreData; onReplan: () => void }) {
  const failed = done.failed ?? 0;
  const steps = done.steps ?? [];
  return (
    <section aria-label="Result" className="flex flex-col gap-3">
      <StatusCard
        variant={failed > 0 ? "warning" : "success"}
        icon={failed > 0 ? <TriangleAlert /> : <CircleCheck />}
        title={
          failed > 0
            ? `Restore finished, ${plural(failed, "step", "steps")} failed`
            : "Setup restored"
        }
        detail={`${plural(steps.length, "step", "steps")}: ${steps.length - failed} done, ${failed} failed. Undo brings the files and settings back.`}
      />
      <ul className="toolport-list" aria-label="Steps">
        {steps.map((step) => (
          <li key={step.id} className="flex items-start gap-3 px-4 py-2 text-sm">
            <span className="min-w-0 flex-1">
              <span className="break-all">{step.label}</span>
              <span className="block text-meta text-muted-foreground">{step.note}</span>
            </span>
            <span
              className={
                step.ok
                  ? "text-meta text-muted-foreground"
                  : "text-meta font-medium text-destructive"
              }
            >
              {step.ok ? "Done" : "Failed"}
            </span>
          </li>
        ))}
      </ul>
      {failed > 0 && (
        <div>
          <p className="mb-2 text-meta text-muted-foreground">
            A failed step changed nothing. Plan again to see what is left, then apply it:
            the steps that worked show as already here.
          </p>
          <Button variant="outline" onClick={onReplan}>
            Plan the failed steps again
          </Button>
        </div>
      )}
      {(done.needsYou ?? []).length > 0 && (
        <div>
          <SectionHeader count={done.needsYou?.length}>Still to do by hand</SectionHeader>
          <ul className="toolport-list">
            {done.needsYou?.map((item) => (
              <li key={item.id} className="px-4 py-2 text-sm">
                <span className="break-all">{item.label}</span>
                <span className="block text-meta text-muted-foreground">{item.note}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

/** Restore a setup file: read it, choose the parts, see the plan, choose per conflict, apply. */
export function RestoreView() {
  const [file, setFile] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [info, setInfo] = useState<InspectData | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);
  const [take, setTake] = useState<string[]>([]);
  const [plan, setPlan] = useState<RestoreData | null>(null);
  const [done, setDone] = useState<RestoreData | null>(null);
  const [run, setRun] = useState<string | null>(null);
  const [wrong, setWrong] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const sealed = info?.parts.some((p) => p.id === SECRETS && p.sealed) ?? false;
  async function planRestore(next: {
    parts?: string[];
    take?: string[];
    passphrase?: string;
    from?: InspectData;
  }) {
    const parts = next.parts ?? chosen;
    const takes = next.take ?? take;
    const pass = next.passphrase ?? passphrase;
    const sends = pass !== "";
    const all = next.from ?? info;
    setBusy(true);
    setWrong(false);
    setFailure(null);
    try {
      const data = await ctlData<RestoreData>(
        restoreArgv(file, {
          sealed: sends,
          take: takes,
          parts:
            all && parts.length < all.parts.filter((p) => p.standing === "ok").length
              ? parts
              : undefined,
        }),
        { stdinSecret: sends ? pass : undefined },
      );
      setPlan(data);
      setDone(null);
    } catch (error) {
      setPlan(null);
      if (error instanceof CtlError && error.code === "passphrase") setWrong(true);
      else {
        setFailure(`Could not plan the restore: ${errorText(error)}`);
        toastError("Could not plan the restore", { description: errorText(error) });
      }
    } finally {
      setBusy(false);
    }
  }

  async function check() {
    setBusy(true);
    setPlan(null);
    setDone(null);
    setTake([]);
    setFailure(null);
    try {
      const data = await ctlData<InspectData>(["setup", "inspect", file]);
      setInfo(data);
      const ok = data.parts.filter((p) => p.standing === "ok" && p.id !== SESSIONS);
      const ids = ok.map((p) => p.id);
      setChosen(ids);
      const hasSealed = data.parts.some((p) => p.id === SECRETS && p.sealed);
      setBusy(false);
      // A file with an encrypted part waits for the passphrase or the choice to go without.
      if (!hasSealed)
        await planRestore({ parts: ids, take: [], passphrase: "", from: data });
    } catch (error) {
      setInfo(null);
      setFailure(`Could not read the file: ${errorText(error)}`);
      toastError("Could not read the file", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  async function apply() {
    if (!plan) return;
    setBusy(true);
    setFailure(null);
    try {
      const sends = passphrase !== "";
      const all = info?.parts.filter((p) => p.standing === "ok").length ?? 0;
      const result = await ctlData<RestoreData>(
        restoreArgv(file, {
          sealed: sends,
          plan: plan.plan,
          take,
          parts: chosen.length < all ? chosen : undefined,
        }),
        { stdinSecret: sends ? passphrase : undefined },
      );
      setDone(result);
      setRun(result.run ?? null);
      setPlan(null);
      if ((result.failed ?? 0) === 0)
        toast.success("Setup restored. Undo brings it back.");
    } catch (error) {
      setFailure(
        `Could not restore the setup: ${errorText(error)}. Nothing was changed.`,
      );
      toastError("Could not restore the setup", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  const changes = plan
    ? plan.rows.filter(
        (r) => r.state === "add" || (r.state === "conflict" && take.includes(r.id)),
      ).length
    : 0;
  const groups = useMemo(() => {
    const byPart = new Map<string, RestoreRow[]>();
    for (const row of plan?.rows ?? []) {
      byPart.set(row.part, [...(byPart.get(row.part) ?? []), row]);
    }
    return [...byPart.entries()];
  }, [plan]);

  return (
    <div className="@container flex flex-col gap-4">
      <section aria-label="File" className="flex flex-col gap-2">
        <Input
          aria-label="Setup file"
          placeholder="/path/to/toolport-setup.zip"
          value={file}
          onChange={(e) => {
            setFile(e.target.value);
            setInfo(null);
            setPlan(null);
          }}
        />
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" disabled={busy || !file} onClick={() => void check()}>
            Check the file
          </Button>
        </div>
      </section>
      {info && (
        <StatusCard
          variant={info.intact ? "info" : "warning"}
          icon={info.intact ? <Info /> : <TriangleAlert />}
          title={`Made on ${info.machine}, ${info.madeAt.slice(0, 10)}`}
          detail={`Toolport ${info.toolport}. ${
            info.intact
              ? `${info.parts.length} of ${info.parts.length} parts match their hashes.`
              : info.problems.join(" ")
          }`}
        />
      )}
      {info && sealed && !plan && (
        <section aria-label="Encrypted part" className="flex flex-col gap-2">
          <SectionHeader>This file has an encrypted part</SectionHeader>
          <p className="text-meta text-muted-foreground">
            Give the passphrase to include the secrets in the plan, or continue without
            them. Nothing is read from the encrypted part until you do.
          </p>
          <Input
            type="password"
            aria-label="Passphrase"
            autoComplete="off"
            placeholder="Passphrase"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
          />
          {wrong && (
            <p role="alert" className="text-meta text-destructive">
              That passphrase does not open the secrets part. Nothing was changed.
            </p>
          )}
          <div className="flex flex-wrap gap-2">
            <Button disabled={busy || !passphrase} onClick={() => void planRestore({})}>
              Unlock secrets
            </Button>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => {
                setPassphrase("");
                void planRestore({ passphrase: "" });
              }}
            >
              Continue without secrets
            </Button>
          </div>
        </section>
      )}
      {info && plan && (
        <PartPicker
          info={info}
          chosen={chosen}
          onChange={(next) => {
            setChosen(next);
            void planRestore({ parts: next });
          }}
        />
      )}
      {plan && (
        <>
          <StatusCard
            variant="info"
            icon={<Info />}
            title="This is a dry run"
            detail={`Nothing has been written, cloned or changed. Plan ${plan.plan} is tied to this Mac as it is now: if it changes before you apply, the apply refuses and you plan again.`}
          />
          <Summary plan={plan} />
          {(plan.refused ?? []).length > 0 && (
            <section aria-label="Refused" className="flex flex-col gap-2">
              <StatusCard
                variant="warning"
                icon={<TriangleAlert />}
                title={`${plural(plan.refused?.length ?? 0, "entry", "entries")} in the file asked for a place a restore never writes`}
                detail="They are left out. Nothing is written for them."
              />
              <ul className="toolport-list">
                {plan.refused?.map((item) => (
                  <li key={item.id} className="px-4 py-2 text-sm">
                    <span className="break-all">{item.label}</span>
                    <span className="block text-meta text-muted-foreground">
                      {item.note}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          )}
          <section aria-label="Plan" className="flex flex-col gap-4">
            {groups.map(([part, rows]) => (
              <PlanGroup
                key={part}
                part={part}
                rows={rows}
                take={take}
                onTake={(id, on) => {
                  const next = on ? [...take, id] : take.filter((t) => t !== id);
                  setTake(next);
                  void planRestore({ take: next });
                }}
              />
            ))}
            {plan.rows.length === 0 && (
              <p className="text-meta text-muted-foreground">
                The parts you chose hold nothing to restore.
              </p>
            )}
          </section>
          <p className="text-meta text-muted-foreground">
            Order: settings, servers, clients, tasks, then clones, then files. A step that
            fails does not stop the others. Default on every conflict is keep mine.
          </p>
          <div className="flex flex-wrap gap-2">
            <Button disabled={busy || changes === 0} onClick={() => void apply()}>
              {changes === 0
                ? "Restore"
                : `Restore ${plural(changes, "change", "changes")}`}
            </Button>
          </div>
        </>
      )}
      {done && <Result done={done} onReplan={() => void planRestore({})} />}
      <UndoPanel key={run ?? "last"} run={run} />
      {!info && !plan && !done && (
        <p className="text-meta text-muted-foreground">
          Restore always starts as a dry run. Existing repositories, servers and files are
          never replaced unless you choose the file&rsquo;s copy for one row.
        </p>
      )}
      {failure && (
        <p role="alert" className="text-meta text-destructive">
          {failure}
        </p>
      )}
      {wrong && !sealed && (
        <p role="alert" className="text-meta text-destructive">
          That passphrase does not open the secrets part. Nothing was changed.
        </p>
      )}
    </div>
  );
}
