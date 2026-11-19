import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { CircleCheck, Info, ShieldAlert } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SectionHeader } from "@/components/ui/section-header";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { ctlData } from "../bridge/ctl";
import { errorText } from "../settings/useCtl";
import {
  exportArgv,
  needsPassphrase,
  NO_CHOICE,
  partsArgv,
  plural,
  SECRETS_PART,
  SESSIONS_PART,
  size,
  SUB_CHOICES,
  type ExportChoice,
  type ExportData,
  type HeldBack,
  type PartsData,
  type PeekData,
  type SessionRepo,
  type SetupPart,
} from "./model";

function Tick({
  label,
  help,
  checked,
  onChange,
  disabled,
}: {
  label: string;
  help?: string;
  checked: boolean;
  onChange: (on: boolean) => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="flex items-start gap-2">
      <input
        type="checkbox"
        id={id}
        className="mt-0.5 shrink-0"
        checked={checked}
        disabled={disabled}
        aria-labelledby={`${id}-label`}
        aria-describedby={help ? `${id}-help` : undefined}
        onChange={(e) => onChange(e.target.checked)}
      />
      <label htmlFor={id} className="min-w-0 text-sm">
        <span id={`${id}-label`} className="break-words">
          {label}
        </span>
        {help && (
          <span id={`${id}-help`} className="block text-meta text-muted-foreground">
            {help}
          </span>
        )}
      </label>
    </div>
  );
}

function infoNumber(part: SetupPart | undefined, key: string): number {
  const value = part?.info?.[key];
  return typeof value === "number" ? value : 0;
}

function SourceRoots({
  detected,
  named,
  onChange,
}: {
  detected: string[];
  named: string[];
  onChange: (roots: string[]) => void;
}) {
  const [draft, setDraft] = useState("");
  const add = () => {
    const root = draft.trim();
    if (!root || named.includes(root)) return;
    onChange([...named, root]);
    setDraft("");
  };
  return (
    <section aria-label="Source folders" className="flex flex-col gap-2">
      <SectionHeader>Source folders</SectionHeader>
      <p className="text-meta text-muted-foreground">
        {named.length === 0
          ? "Repositories are found by walking the folders Toolport already knows. Name folders to search those instead."
          : "Only these folders are searched for repositories."}
      </p>
      <ul className="toolport-list">
        {(named.length > 0 ? named : detected).map((root) => (
          <li key={root} className="flex items-center gap-3 px-4 py-2 text-sm">
            <span className="min-w-0 flex-1 break-all">{root}</span>
            {named.includes(root) && (
              <Button
                variant="ghost"
                size="sm"
                aria-label={`Remove ${root}`}
                onClick={() => onChange(named.filter((r) => r !== root))}
              >
                Remove
              </Button>
            )}
          </li>
        ))}
        {named.length === 0 && detected.length === 0 && (
          <li className="px-4 py-2 text-meta text-muted-foreground">
            No source folder is known yet.
          </li>
        )}
      </ul>
      <div className="flex flex-wrap gap-2">
        <Input
          className="min-w-0 flex-1"
          aria-label="Add a source folder"
          placeholder="~/code"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && add()}
        />
        <Button variant="outline" disabled={!draft.trim()} onClick={add}>
          Add folder
        </Button>
        {named.length > 0 && (
          <Button variant="ghost" onClick={() => onChange([])}>
            Use the usual folders
          </Button>
        )}
      </div>
    </section>
  );
}

function RepoChoices({
  parts,
  choice,
  onChange,
}: {
  parts: PartsData;
  choice: ExportChoice;
  onChange: (next: Partial<ExportChoice>) => void;
}) {
  const [all, setAll] = useState(false);
  const repos = parts.repos ?? [];
  const noRemote = repos.filter((r) => r.noRemote).length;
  const shown = all ? repos : repos.slice(0, 5);
  return (
    <div className="flex flex-col gap-2">
      <Tick
        label="Include worktrees"
        help="Branch and path of the linked worktrees, so they can be recreated. Their files are never exported."
        checked={choice.worktrees}
        onChange={(worktrees) => onChange({ worktrees })}
      />
      <Tick
        label={`Leave out repositories without a remote (${noRemote})`}
        help="They cannot be cloned again. Otherwise they stay in the list, flagged."
        checked={choice.skipNoRemote}
        onChange={(skipNoRemote) => onChange({ skipNoRemote })}
      />
      {repos.length > 0 && (
        <div>
          <ul className="flex flex-col gap-1" aria-label="Repositories found">
            {shown.map((repo) => (
              <li key={repo.path} className="text-meta">
                <span className="font-medium break-all">{repo.name}</span>{" "}
                <span className="text-muted-foreground break-all">
                  {repo.path} · {repo.branch ?? "detached"}
                  {repo.noRemote ? " · no remote" : ""}
                  {repo.unpushed > 0
                    ? ` · ${plural(repo.unpushed, "unpushed commit", "unpushed commits")}`
                    : ""}
                </span>
              </li>
            ))}
          </ul>
          {repos.length > 5 && (
            <Button variant="link" size="sm" onClick={() => setAll(!all)}>
              {all ? "Show fewer" : `Show all ${repos.length}`}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}

function SessionChoices({
  repos,
  chosen,
  onChange,
}: {
  repos: SessionRepo[];
  chosen: string[] | null;
  onChange: (chosen: string[] | null) => void;
}) {
  if (repos.length === 0) {
    return (
      <p className="text-meta text-muted-foreground">
        No past conversations were found for these repositories.
      </p>
    );
  }
  const names = repos.map((r) => r.name);
  const on = chosen ?? names;
  return (
    <fieldset className="flex flex-col gap-2">
      <legend className="text-meta text-muted-foreground">
        Choose the repositories whose conversations go in
      </legend>
      {repos.map((repo) => (
        <Tick
          key={repo.path}
          label={`${repo.name} · ${plural(repo.sessions, "session", "sessions")}, ${size(repo.fileSize)}`}
          checked={on.includes(repo.name)}
          onChange={(isOn) => {
            const next = isOn ? [...on, repo.name] : on.filter((n) => n !== repo.name);
            onChange(next.length === names.length ? null : next);
          }}
        />
      ))}
      {on.length === 0 && (
        <p role="alert" className="text-meta text-destructive">
          Choose at least one repository, or untick Sessions.
        </p>
      )}
    </fieldset>
  );
}

function HeldBackList({
  held,
  sealed,
}: {
  held: HeldBack[];
  /** Secrets is ticked: the files travel in the encrypted part. */
  sealed: boolean;
}) {
  if (held.length === 0) return null;
  return (
    <section aria-label="Held back by the secret scan" className="flex flex-col gap-2">
      <SectionHeader count={held.length}>Held back by the secret scan</SectionHeader>
      <p className="text-meta text-muted-foreground">
        {sealed
          ? "These files look like they hold a secret. They go into the encrypted part and come back to their own places on a restore."
          : "These files look like they hold a secret, so they stay out of the file. Tick Secrets to carry them in the encrypted part."}
      </p>
      <ul className="toolport-list">
        {held.map((row) => (
          <li key={`${row.kind}:${row.file}`} className="px-4 py-2 text-sm">
            <span className="break-all">{row.file}</span>
            <span className="block text-meta text-muted-foreground">
              Looks like {row.reason} ·{" "}
              {sealed ? "goes into the encrypted part" : "stays out of the file"}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function Peek({ data }: { data: PeekData }) {
  return (
    <section aria-label="Peek" className="flex flex-col gap-2">
      <SectionHeader>Inside {data.peek.label}</SectionHeader>
      <ul className="toolport-list">
        {data.peek.files.map((file) => (
          <li key={file.name} className="px-4 py-2 text-sm">
            <span className="break-all">{file.name}</span>
            {file.fileSize !== undefined && (
              <span className="text-meta text-muted-foreground">
                {" "}
                · {size(file.fileSize)}
              </span>
            )}
            {file.json !== undefined && (
              <pre className="mt-1 max-h-56 overflow-auto rounded-md bg-muted p-2 text-meta whitespace-pre-wrap break-all">
                {JSON.stringify(file.json, null, 2)}
              </pre>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}

function PreviewCard({ data, secretsOn }: { data: ExportData; secretsOn: boolean }) {
  const held = data.parts.flatMap((p) => p.heldBack ?? []);
  return (
    <section aria-label="What goes in the file" className="flex flex-col gap-3">
      <SectionHeader>What goes in the file</SectionHeader>
      <StatusCard
        variant="info"
        icon={<Info />}
        title={`Would write ${data.name}`}
        detail={`${plural(data.parts.length, "part", "parts")}, about ${size(data.totalSize ?? data.fileSize ?? 0)}`}
      />
      {data.tree && (
        <ul className="toolport-list" aria-label="Files in the zip">
          {data.tree.map((row) => (
            <li key={row.name} className="flex items-center gap-3 px-4 py-2 text-sm">
              <span className="min-w-0 flex-1 break-all">{row.name}</span>
              <span className="text-meta text-muted-foreground">
                {row.sealed ? "encrypted" : size(row.fileSize)}
              </span>
            </li>
          ))}
        </ul>
      )}
      {(data.skipped ?? []).map((line) => (
        <p key={line} className="text-meta text-muted-foreground">
          Left out: {line}
        </p>
      ))}
      <HeldBackList held={held} sealed={data.heldBackSealed ?? secretsOn} />
      {data.notIncluded && (
        <div>
          <p className="text-meta font-medium">Not in the file</p>
          <ul className="list-disc pl-5 text-meta text-muted-foreground">
            {data.notIncluded.map((line) => (
              <li key={line}>{line}</li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

/** Export a setup file: the parts, the choices inside them, a preview, then the file. */
export function ExportView({
  seedParts,
  onOpenReset,
}: {
  /** Parts ticked on arrival, from the checklist that sent the person here. */
  seedParts: string[] | null;
  onOpenReset: () => void;
}) {
  const [parts, setParts] = useState<PartsData | null>(null);
  const [choice, setChoice] = useState<ExportChoice>(NO_CHOICE);
  const [out, setOut] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [repeat, setRepeat] = useState("");
  const [preview, setPreview] = useState<ExportData | null>(null);
  const [saved, setSaved] = useState<ExportData | null>(null);
  const [peek, setPeek] = useState<PeekData | null>(null);
  const [busy, setBusy] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const started = useRef(false);

  const roots = choice.roots;
  const load = useCallback(
    async (named: string[]) => {
      try {
        const data = await ctlData<PartsData>(partsArgv(named));
        setLoadError(null);
        setParts(data);
        if (!started.current) {
          started.current = true;
          const base = seedParts ?? data.parts.filter((p) => p.default).map((p) => p.id);
          setChoice((prev) => ({ ...prev, parts: base }));
        }
      } catch (error) {
        setLoadError(errorText(error));
        toastError("Could not read the setup parts", { description: errorText(error) });
      }
    },
    [seedParts],
  );

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- Reads the parts once when the view opens.
    void load([]);
  }, [load]);

  const change = (next: Partial<ExportChoice>) => {
    setChoice((prev) => ({ ...prev, ...next }));
    setPreview(null);
    setPeek(null);
    setSaved(null);
  };

  const tick = (id: string, on: boolean) => {
    const next = on ? [...choice.parts, id] : choice.parts.filter((p) => p !== id);
    change({ parts: next });
    if (parts && !needsPassphrase(next, parts.parts)) {
      setPassphrase("");
      setRepeat("");
    }
  };

  const changeRoots = (named: string[]) => {
    change({ roots: named, sessionRepos: null });
    void load(named);
  };

  const sealedChosen = parts ? needsPassphrase(choice.parts, parts.parts) : false;
  const secretsOn = choice.parts.includes(SECRETS_PART);
  const min = parts?.minPassphrase ?? 12;
  const short = sealedChosen && passphrase !== "" && passphrase.length < min;
  const mismatch = sealedChosen && passphrase !== "" && passphrase !== repeat;
  const passphraseOk =
    !sealedChosen || (passphrase.length >= min && passphrase === repeat);
  const sessionsPart = parts?.parts.find((p) => p.id === SESSIONS_PART);
  const sessionRepos = useMemo(
    () => (sessionsPart?.info?.repos as SessionRepo[] | undefined) ?? [],
    [sessionsPart],
  );
  const noSessionChosen =
    choice.parts.includes(SESSIONS_PART) &&
    sessionRepos.length > 0 &&
    choice.sessionRepos !== null &&
    choice.sessionRepos.length === 0;
  const held = (parts?.parts ?? [])
    .filter((p) => choice.parts.includes(p.id))
    .flatMap((p) => p.heldBack ?? []);

  async function run(mode: "preview" | "write" | { peek: string }) {
    if (!parts) return;
    setBusy(true);
    setFailure(null);
    try {
      const dryRun = mode === "preview";
      const peekPart = typeof mode === "object" ? mode.peek : undefined;
      const sealing = sealedChosen && passphrase !== "" && passphraseOk;
      const argv = exportArgv(choice, out, {
        passphrase: sealing,
        dryRun,
        peek: peekPart,
      });
      const data = await ctlData<ExportData & PeekData>(argv, {
        stdinSecret: sealing ? passphrase : undefined,
      });
      if (peekPart) setPeek(data);
      else if (dryRun) setPreview(data);
      else {
        setSaved(data);
        setPreview(null);
        toast.success(`Wrote ${data.file}`);
      }
    } catch (error) {
      setFailure(errorText(error));
      toastError("Could not export the setup", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  if (!parts && loadError)
    return (
      <div className="flex flex-col items-start gap-2">
        <p role="alert" className="text-meta text-destructive">
          Could not read the setup parts: {loadError}
        </p>
        <Button variant="outline" size="sm" onClick={() => void load(roots)}>
          Try again
        </Button>
      </div>
    );
  if (!parts)
    return <p className="text-meta text-muted-foreground">Loading the parts…</p>;

  const unreadable = parts.parts.filter((p) => p.error);
  return (
    <div className="flex flex-col gap-4">
      <SourceRoots detected={parts.roots ?? []} named={roots} onChange={changeRoots} />
      <section aria-label="Parts" className="flex flex-col gap-2">
        <SectionHeader>Parts</SectionHeader>
        <ul className="toolport-list">
          {parts.parts.map((part) => {
            const on = choice.parts.includes(part.id);
            const subs = SUB_CHOICES[part.id];
            return (
              <li key={part.id} className="flex flex-col gap-2 px-4 py-2">
                <Tick
                  label={part.label}
                  help={[
                    part.id === SECRETS_PART
                      ? `${plural(part.count, "value", "values")}, encrypted. Off by default.`
                      : part.id === SESSIONS_PART
                        ? `${plural(part.count, "session", "sessions")}, ${size(part.fileSize ?? 0)}. Off by default and always encrypted.`
                        : `${part.count} · ${size(part.fileSize ?? 0)}`,
                    ...part.notes,
                  ].join(" · ")}
                  checked={on}
                  onChange={(isOn) => tick(part.id, isOn)}
                />
                {on && (
                  <div className="ml-6 flex flex-col gap-2 border-l pl-3">
                    {part.id === "repos" && (
                      <RepoChoices parts={parts} choice={choice} onChange={change} />
                    )}
                    {part.id === "library" && (
                      <Tick
                        label={`Include the ${infoNumber(part, "uncommitted")} uncommitted files as a zip`}
                        help="Without it the library comes back at its last pushed commit."
                        checked={choice.libraryZip}
                        onChange={(libraryZip) => change({ libraryZip })}
                      />
                    )}
                    {subs?.map((sub) => (
                      <Tick
                        key={sub.id}
                        label={sub.label}
                        help={sub.help}
                        checked={!choice.without.includes(sub.id)}
                        onChange={(isOn) =>
                          change({
                            without: isOn
                              ? choice.without.filter((id) => id !== sub.id)
                              : [...choice.without, sub.id],
                          })
                        }
                      />
                    ))}
                    {part.id === SESSIONS_PART && (
                      <SessionChoices
                        repos={sessionRepos}
                        chosen={choice.sessionRepos}
                        onChange={(sessionChoice) =>
                          change({ sessionRepos: sessionChoice })
                        }
                      />
                    )}
                    {!part.sealed && (
                      <div>
                        <Button
                          variant="link"
                          size="sm"
                          disabled={busy}
                          aria-label={`Peek inside ${part.label}`}
                          onClick={() => void run({ peek: part.id })}
                        >
                          Peek inside
                        </Button>
                      </div>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
        {unreadable.map((part) => (
          <p key={part.id} role="alert" className="text-meta text-destructive">
            {part.label} could not be read: {part.error}
          </p>
        ))}
      </section>
      {peek && <Peek data={peek} />}
      <HeldBackList held={held} sealed={secretsOn} />
      {sealedChosen && (
        <section aria-label="Sealing" className="flex flex-col gap-2">
          <SectionHeader>Passphrase</SectionHeader>
          <p className="text-meta text-muted-foreground">
            Sessions and secrets only leave sealed with a passphrase of at least {min}{" "}
            characters. It is never stored and cannot be recovered; without it the sealed
            parts cannot be read by anyone. The rest of the file does not need it.
          </p>
          <Input
            type="password"
            aria-label="Passphrase"
            autoComplete="new-password"
            value={passphrase}
            onChange={(e) => setPassphrase(e.target.value)}
          />
          <Input
            type="password"
            aria-label="Repeat passphrase"
            autoComplete="new-password"
            value={repeat}
            onChange={(e) => setRepeat(e.target.value)}
          />
          {short && (
            <p role="alert" className="text-meta text-destructive">
              The passphrase is shorter than {min} characters.
            </p>
          )}
          {!short && mismatch && repeat !== "" && (
            <p role="alert" className="text-meta text-destructive">
              The two passphrases differ.
            </p>
          )}
        </section>
      )}
      <section aria-label="File" className="flex flex-col gap-2">
        <SectionHeader>File</SectionHeader>
        <Input
          aria-label="Write to"
          placeholder="/path/to/folder or file.zip"
          value={out}
          onChange={(e) => setOut(e.target.value)}
        />
      </section>
      {preview && <PreviewCard data={preview} secretsOn={secretsOn} />}
      {saved && (
        <StatusCard
          variant="success"
          icon={<CircleCheck />}
          title="Export saved"
          detail={`${saved.file} · ${size(saved.fileSize ?? 0)}. Checked after writing: the file was opened again and every part matched its hash.`}
          actions={
            <Button variant="outline" size="sm" onClick={onOpenReset}>
              Open the reset checklist
            </Button>
          }
        >
          {(saved.skipped ?? []).map((line) => (
            <p key={line} className="text-meta text-muted-foreground">
              Left out: {line}
            </p>
          ))}
          <p className="text-meta text-muted-foreground">
            Keep this file somewhere that survives the wipe.
            {secretsOn ? " The secrets part is only as safe as your passphrase." : ""}
          </p>
        </StatusCard>
      )}
      {!secretsOn && held.length > 0 && (
        <p className="flex items-center gap-2 text-meta text-muted-foreground">
          <ShieldAlert className="size-4 shrink-0" />
          {plural(held.length, "file stays", "files stay")} out of the file unless you
          tick Secrets.
        </p>
      )}
      {failure && (
        <p role="alert" className="text-meta text-destructive">
          Could not export the setup: {failure}. Nothing was written.
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          variant="outline"
          disabled={
            busy ||
            (passphrase !== "" && !passphraseOk) ||
            choice.parts.length === 0 ||
            noSessionChosen
          }
          onClick={() => void run("preview")}
        >
          Preview
        </Button>
        <Button
          disabled={
            busy || !passphraseOk || !out || choice.parts.length === 0 || noSessionChosen
          }
          onClick={() => void run("write")}
        >
          Write the file
        </Button>
      </div>
    </div>
  );
}
