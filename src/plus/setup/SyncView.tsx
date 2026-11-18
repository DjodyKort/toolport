import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  CircleCheck,
  Eye,
  Info,
  Lock,
  RefreshCw,
  TriangleAlert,
  Undo2,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ErrorCallout, StatusCard } from "@/components/ui/status-card";
import { Input } from "@/components/ui/input";
import { SectionHeader } from "@/components/ui/section-header";
import { Switch } from "@/components/ui/switch";
import { toastError } from "@/lib/toast";
import { ctlData } from "../bridge/ctl";
import { errorText } from "../settings/useCtl";
import type { PartsData } from "./model";
import RepoHeads from "./RepoHeads";
import {
  changes,
  placeOf,
  pullArgv,
  rowName,
  setupArgv,
  SYNC_STATE_LABEL,
  whenText,
  type PushPreview,
  type SyncPlan,
  type SyncRow,
  type SyncStatus,
} from "./syncModel";

const DESCRIPTION: Record<string, string> = {
  servers:
    "Definitions in upstream's export format. Arrive held until you review them here.",
  access:
    "Access sets, client scopes and folder routing. Client config files are not synced; each machine reconnects its own.",
  tasks:
    "Definitions and schedules. Whether a task runs on this machine is a local choice and is not synced.",
  settings:
    "Safety, discovery, bundles, per-folder leave-outs. Ports and paths that belong to one machine are skipped.",
  memory:
    "Off by default: two machines remember different things, and merging notes written by Claude is a judgement call. Sessions never sync.",
  serverRepos:
    "Which branch and commit each local server checkout is on, with its remote's address and whether it is pushed. Read only; each machine writes its own record.",
  claudeUser:
    "CLAUDE.md, rules, settings.json with secrets filtered, plugin list. Off by default: settings.json carries hooks you should read before they run on another machine.",
  secrets:
    "Off by default. Values of servers you typed in Toolport, encrypted like everything else. Sign-in sessions never sync.",
};

const UNIT: Record<string, string> = {
  servers: "servers",
  access: "sets",
  tasks: "tasks",
  settings: "settings",
  claudeUser: "files",
  memory: "items",
  secrets: "values",
  serverRepos: "checkouts",
};

const MIN_PASSPHRASE = 12;

function Item({
  title,
  detail,
  children,
}: {
  title: ReactNode;
  detail?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <li className="flex items-center gap-3 px-4 py-3 @max-[560px]:flex-col @max-[560px]:items-start">
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium break-words">{title}</p>
        {detail && <p className="text-meta text-muted-foreground">{detail}</p>}
      </div>
      {children && (
        <div
          data-slot="item-actions"
          className="flex flex-wrap items-center gap-2 @max-[560px]:mt-1"
        >
          {children}
        </div>
      )}
    </li>
  );
}

function Radio({
  checked,
  disabled,
  onSelect,
  title,
  detail,
  badge,
}: {
  checked: boolean;
  disabled?: boolean;
  onSelect: () => void;
  title: string;
  detail: string;
  badge?: ReactNode;
}) {
  return (
    <li className="flex items-center gap-3 px-4 py-3">
      <button
        type="button"
        role="radio"
        aria-checked={checked}
        aria-label={title}
        disabled={disabled}
        onClick={onSelect}
        className="size-4 shrink-0 rounded-full border border-input aria-checked:border-4 aria-checked:border-primary disabled:opacity-50"
      />
      <div className="min-w-0 flex-1">
        <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
          {title} {badge}
        </p>
        <p className="text-meta text-muted-foreground">{detail}</p>
      </div>
    </li>
  );
}

function SetupForm({
  status,
  onSaved,
  onCancel,
  submitLabel,
  forgetOption,
}: {
  status: SyncStatus | null;
  onSaved: (status: SyncStatus) => void | Promise<void>;
  onCancel: () => void;
  submitLabel: string;
  forgetOption?: boolean;
}) {
  const current = status?.transport ?? null;
  const [kind, setKind] = useState<"git" | "folder">(current?.kind ?? "git");
  const [remote, setRemote] = useState(current ? placeOf(current) : "");
  const [branch, setBranch] = useState(current?.kind === "git" ? current.branch : "main");
  const [passphrase, setPassphrase] = useState("");
  const [repeat, setRepeat] = useState("");
  const [schedule, setSchedule] = useState(status?.schedule?.enabled ?? false);
  const [busy, setBusy] = useState(false);
  const needsPassphrase = status?.passphrase !== "stored";
  const short = passphrase !== "" && passphrase.length < MIN_PASSPHRASE;
  const mismatch = repeat !== "" && repeat !== passphrase;
  const incomplete =
    remote.trim() === "" ||
    (needsPassphrase && passphrase === "") ||
    (passphrase !== "" && repeat !== passphrase);

  async function save(extra: Parameters<typeof setupArgv>[0] = {}) {
    setBusy(true);
    try {
      const where =
        kind === "git" ? { git: remote.trim(), branch } : { folder: remote.trim() };
      const next = await ctlData<SyncStatus>(
        setupArgv({
          ...where,
          passphrase: passphrase !== "",
          schedule,
          ...extra,
        }),
        { stdinSecret: passphrase !== "" ? passphrase : undefined },
      );
      await onSaved(next);
    } catch (error) {
      toastError("Could not save the sync settings", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <section aria-label="Where to keep it">
        <SectionHeader count={3}>Where to keep it</SectionHeader>
        <ul className="toolport-list" role="radiogroup" aria-label="Where to keep it">
          <Radio
            checked={kind === "git"}
            onSelect={() => setKind("git")}
            title="A private Git repository"
            detail="Any remote you already use. History is the version store; a bad sync is one revert. Needs git and your own credentials."
            badge={<Badge variant="success">Recommended</Badge>}
          />
          <Radio
            checked={kind === "folder"}
            onSelect={() => setKind("folder")}
            title="A folder"
            detail="iCloud Drive, Syncthing, a network disk. One encrypted file per sync; the folder tool does the moving."
          />
          <Radio
            checked={false}
            disabled
            onSelect={() => {}}
            title="Toolport's hosted sync (upstream Pro)"
            detail="Servers only, readable by the service, a paid account. Disabled in this build."
            badge={<Badge variant="off">Not available here</Badge>}
          />
        </ul>
      </section>
      <section aria-label="Place" className="flex max-w-xl flex-col gap-2">
        <label className="text-meta text-muted-foreground" htmlFor="sync-remote">
          {kind === "git" ? "Repository URL" : "Folder"}
        </label>
        <Input
          id="sync-remote"
          value={remote}
          placeholder={
            kind === "git" ? "git@github.com:you/toolport-sync.git" : "/path/to/folder"
          }
          onChange={(e) => setRemote(e.target.value)}
        />
        {kind === "git" && (
          <Input
            aria-label="Branch"
            value={branch}
            onChange={(e) => setBranch(e.target.value)}
          />
        )}
        <p className="text-meta text-muted-foreground">
          {kind === "git"
            ? "Use a private repository: an encrypted bundle is still a file you do not want to publish."
            : "Pick a folder your sync tool already moves between machines."}
        </p>
      </section>
      <section aria-label="Passphrase and check" className="flex max-w-xl flex-col gap-2">
        <p className="text-sm font-semibold">Passphrase</p>
        <Input
          type="password"
          aria-label="Passphrase"
          placeholder={
            needsPassphrase ? "" : "Stored in the keychain; type one to change it"
          }
          value={passphrase}
          onChange={(e) => setPassphrase(e.target.value)}
        />
        <Input
          type="password"
          aria-label="Repeat the passphrase"
          value={repeat}
          onChange={(e) => setRepeat(e.target.value)}
        />
        {short && (
          <p role="alert" className="text-meta text-destructive">
            The passphrase is shorter than {MIN_PASSPHRASE} characters.
          </p>
        )}
        {mismatch && (
          <p role="alert" className="text-meta text-destructive">
            The two passphrases differ.
          </p>
        )}
        {passphrase !== "" && repeat === passphrase && !short && (
          <p className="text-meta text-success">Matches</p>
        )}
        <p className="text-meta text-muted-foreground">
          Needed once on every other machine. There is no recovery; the export file can
          use the same passphrase.
        </p>
      </section>
      <label className="flex items-center gap-3 text-sm">
        <Switch
          size="sm"
          aria-label="Check every 30 minutes"
          checked={schedule}
          onCheckedChange={setSchedule}
        />
        Check the remote every 30 minutes (only plans; nothing is applied)
      </label>
      <div className="flex flex-wrap gap-2">
        <Button disabled={busy || incomplete || short} onClick={() => void save()}>
          {submitLabel}
        </Button>
        <Button variant="outline" onClick={onCancel}>
          Cancel
        </Button>
        {forgetOption && (
          <Button
            variant="ghost"
            disabled={busy}
            onClick={() => {
              setBusy(true);
              void ctlData<SyncStatus>(setupArgv({ forgetPassphrase: true }))
                .then(onSaved, (error) =>
                  toastError("Could not remove the passphrase", {
                    description: errorText(error),
                  }),
                )
                .finally(() => setBusy(false));
            }}
          >
            Forget the passphrase on this machine
          </Button>
        )}
      </div>
    </div>
  );
}

function PlanCard({
  plan,
  taken,
  busy,
  onApply,
  onCancel,
}: {
  plan: SyncPlan;
  taken: string[];
  busy: boolean;
  onApply: () => void;
  onCancel: () => void;
}) {
  const writing = changes(plan, taken);
  const shown = plan.rows.filter((row) => row.state !== "here");
  return (
    <section aria-label="Plan" className="flex flex-col gap-2">
      <SectionHeader count={shown.length}>
        {plan.remote ? `Plan from ${plan.remote.machine}` : "What would change"}
      </SectionHeader>
      {shown.length === 0 ? (
        <p className="text-meta text-muted-foreground">Everything is already in step.</p>
      ) : (
        <ul className="toolport-list">
          {shown.map((row) => (
            <Item key={row.id} title={row.label} detail={row.note}>
              <span className="text-meta">
                {taken.includes(row.id)
                  ? "Other machine's copy"
                  : SYNC_STATE_LABEL[row.state]}
              </span>
            </Item>
          ))}
        </ul>
      )}
      <div className="flex gap-2">
        <Button disabled={busy} onClick={onApply}>
          {writing.length > 0 ? `Apply ${writing.length} changes` : "Apply"}
        </Button>
        <Button variant="outline" disabled={busy} onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </section>
  );
}

function NeedsYou({
  status,
  notes,
  held,
  busy,
  onThis,
  onOther,
}: {
  status: SyncStatus;
  notes: Record<string, SyncRow>;
  held: { id: string; label: string; note: string }[];
  busy: boolean;
  onThis: (id: string) => void;
  onOther: (id: string) => void;
}) {
  const total = status.pending.length + held.length;
  if (total === 0) return null;
  return (
    <section aria-label="Needs you">
      <SectionHeader count={total}>Needs you</SectionHeader>
      <ul className="toolport-list">
        {status.pending.map((id) => (
          <Item
            key={id}
            title={`${rowName(id)} changed on two machines`}
            detail={notes[id]?.note ?? "Your version is kept until you choose."}
          >
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              aria-label={`This machine for ${rowName(id)}`}
              onClick={() => onThis(id)}
            >
              This machine
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              aria-label={`Other machine for ${rowName(id)}`}
              onClick={() => onOther(id)}
            >
              Other machine
            </Button>
          </Item>
        ))}
        {held.map((row) => (
          <Item key={row.id} title={row.label} detail={row.note} />
        ))}
      </ul>
    </section>
  );
}

function WhatSyncs({
  status,
  counts,
  busy,
  onToggle,
}: {
  status: SyncStatus;
  counts: Record<string, number>;
  busy: boolean;
  onToggle: (id: string, on: boolean) => void;
}) {
  return (
    <section aria-label="What syncs">
      <SectionHeader count={status.parts.length + 1}>What syncs</SectionHeader>
      <ul className="toolport-list">
        {status.parts.map((part) => (
          <Item key={part.id} title={part.label} detail={DESCRIPTION[part.id]}>
            {part.handledBy ? (
              <>
                <Badge variant="partial">Handled by {part.handledBy} Sync</Badge>
                <span className="text-meta text-muted-foreground">not here</span>
              </>
            ) : (
              <>
                {counts[part.id] !== undefined && (
                  <span className="rounded-full bg-secondary px-2 py-px text-xs">
                    {counts[part.id]} {UNIT[part.id]}
                  </span>
                )}
                <Switch
                  size="sm"
                  aria-label={`Sync ${part.label}`}
                  checked={part.on}
                  disabled={busy}
                  onCheckedChange={(on) => onToggle(part.id, on)}
                />
              </>
            )}
          </Item>
        ))}
        <Item
          title="Skills library"
          detail="Not in this bundle. The library is its own Git repository with its own remote and sync (Library > Sync)."
        >
          <Badge variant="partial">Own Git sync</Badge>
          <span className="text-meta text-muted-foreground">not here</span>
        </Item>
      </ul>
    </section>
  );
}

function PushPreviewBlock({
  preview,
  busy,
  onPush,
}: {
  preview: PushPreview;
  busy: boolean;
  onPush: () => void;
}) {
  return (
    <div role="status" className="text-meta">
      <p className="font-medium">
        Would push {preview.parts.length} parts, about {preview.totalSize} bytes
      </p>
      <ul className="list-disc pl-5 text-muted-foreground">
        {preview.parts.map((part) => (
          <li key={part.id}>
            {part.label}: {part.count}
          </li>
        ))}
      </ul>
      <p className="mt-1 text-muted-foreground">Not included:</p>
      <ul className="list-disc pl-5 text-muted-foreground">
        {preview.notIncluded.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
      <Button className="mt-2" size="sm" disabled={busy} onClick={onPush}>
        Push now
      </Button>
    </div>
  );
}

function Remote({
  busy,
  preview,
  onChangePassphrase,
  onPreview,
  onPush,
}: {
  busy: boolean;
  preview: PushPreview | null;
  onChangePassphrase: () => void;
  onPreview: () => void;
  onPush: () => void;
}) {
  return (
    <section
      aria-label="What the remote can see"
      className="flex flex-col gap-2 rounded-xl border bg-card p-4"
    >
      <p className="flex items-center gap-2 text-sm font-semibold">
        <Eye className="size-4" /> What the remote can see
      </p>
      <p className="text-meta text-muted-foreground">
        Ciphertext of every part, the file names, and the salt. Part names, server names
        and paths are inside the encrypted manifest, so a repository host learns the size
        of the bundle and the times of the commits, nothing else.
      </p>
      <p className="text-meta text-muted-foreground">
        The passphrase is kept in this machine&apos;s keychain, not in a file next to the
        data. A second machine needs the passphrase once.
      </p>
      {preview && <PushPreviewBlock preview={preview} busy={busy} onPush={onPush} />}
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" size="sm" onClick={onChangePassphrase}>
          Change passphrase…
        </Button>
        <Button variant="ghost" size="sm" disabled={busy} onClick={onPreview}>
          Show what would be pushed
        </Button>
      </div>
    </section>
  );
}

/** Settings > Toolport+ > Setup & backup > Sync: the setup file sealed and kept in step on a
 * private git remote or a folder. Everything comes in as a plan first. */
export default function SyncView() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [counts, setCounts] = useState<Record<string, number>>({});
  const [notes, setNotes] = useState<Record<string, SyncRow>>({});
  const [plan, setPlan] = useState<{
    data: SyncPlan;
    taken: string[];
    pushAfter: boolean;
  } | null>(null);
  const [held, setHeld] = useState<{ id: string; label: string; note: string }[]>([]);
  const [preview, setPreview] = useState<PushPreview | null>(null);
  const [settings, setSettings] = useState(false);
  const [firstRun, setFirstRun] = useState(false);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const next = await ctlData<SyncStatus>(["sync", "status"]);
      setStatus(next);
      setLoadError(null);
      return next;
    } catch (error) {
      setLoadError(errorText(error));
      return null;
    }
  }, []);

  useEffect(() => {
    void ctlData<SyncStatus>(["sync", "status"]).then(setStatus, (error) =>
      setLoadError(errorText(error)),
    );
    void ctlData<PartsData>(["setup", "parts"]).then(
      (data) => setCounts(Object.fromEntries(data.parts.map((p) => [p.id, p.count]))),
      () => {},
    );
  }, []);

  const needsNotes =
    status !== null && status.pending.length > 0 && status.passphrase === "stored";
  useEffect(() => {
    if (!needsNotes) return;
    void ctlData<SyncPlan>(pullArgv({})).then(
      (data) =>
        setNotes(Object.fromEntries((data.rows ?? []).map((row) => [row.id, row]))),
      () => {},
    );
  }, [needsNotes]);

  async function guarded<T>(
    what: string,
    work: () => Promise<T>,
  ): Promise<T | undefined> {
    setBusy(true);
    try {
      return await work();
    } catch (error) {
      toastError(what, { description: errorText(error) });
      return undefined;
    } finally {
      setBusy(false);
    }
  }

  async function push() {
    const done = await guarded("Could not push", () =>
      ctlData<PushPreview>(["sync", "push"]),
    );
    if (done) {
      setPreview(null);
      toast.success("Pushed. The other machines pick it up on their next sync.");
    }
    await refresh();
  }

  async function syncNow() {
    const data = await guarded("Could not read the remote", () =>
      ctlData<SyncPlan>(pullArgv({})),
    );
    if (!data) return;
    if (data.skipped === true) {
      toast.info(data.reason ?? "Sync was skipped");
      return;
    }
    if (data.empty) {
      await showPush();
      toast.info("Nothing is on the remote yet. Review what goes out, then push.");
      return;
    }
    if (changes(data, []).length === 0) {
      await push();
      return;
    }
    setPlan({ data, taken: [], pushAfter: true });
  }

  async function chooseOther(id: string) {
    const data = await guarded("Could not plan the other machine's copy", () =>
      ctlData<SyncPlan>(pullArgv({ take: [id] })),
    );
    if (data) setPlan({ data, taken: [id], pushAfter: false });
  }

  async function chooseThis(id: string) {
    const done = await guarded("Could not keep this machine's copy", async () => {
      const data = await ctlData<SyncPlan>(pullArgv({}));
      return ctlData<SyncPlan>(pullArgv({ keep: [id], plan: data.plan }));
    });
    if (done)
      toast.success(
        `Kept this machine's ${rowName(id)}. It goes out with your next push.`,
      );
    await refresh();
  }

  async function apply() {
    if (!plan) return;
    const done = await guarded("Could not apply the plan", () =>
      ctlData<SyncPlan>(pullArgv({ take: plan.taken, plan: plan.data.plan })),
    );
    if (!done) return;
    setPlan(null);
    setHeld(done.needsYou ?? []);
    toast.success("Applied. Undo brings the previous state back.");
    const next = await refresh();
    if (plan.pushAfter && next && next.pending.length === 0) await push();
  }

  async function undo() {
    const done = await guarded("Could not undo", () => ctlData(["sync", "undo"]));
    if (done) toast.success("Sync undone");
    setHeld([]);
    await refresh();
  }

  async function toggle(id: string, on: boolean) {
    if (!status) return;
    const ids = status.parts.filter((p) => (p.id === id ? on : p.on)).map((p) => p.id);
    await guarded("Could not change what syncs", () =>
      ctlData<SyncStatus>(setupArgv({ parts: ids })),
    );
    await refresh();
  }

  async function showPush() {
    const data = await guarded("Could not preview the push", () =>
      ctlData<PushPreview>(["sync", "push", "--dry-run"]),
    );
    if (data) setPreview(data);
  }

  async function firstSyncPreview(next: SyncStatus) {
    setStatus(next);
    setFirstRun(false);
    await syncNow();
    await refresh();
  }

  if (loadError)
    return (
      <ErrorCallout
        title="Could not read the sync settings"
        cause={loadError}
        onRetry={() => void refresh()}
        onViewLog={() => {}}
      />
    );
  if (!status)
    return <p className="text-meta text-muted-foreground">Reading the sync settings…</p>;

  if (!status.configured || firstRun) {
    return (
      <div className="@container flex flex-col gap-4">
        <div>
          <p className="text-base font-semibold">Keep your machines in step</p>
          <p className="text-meta text-muted-foreground">
            Encrypted, and the same file as an export. First sync shows a dry run, like a
            restore.
          </p>
        </div>
        <SetupForm
          status={status}
          submitLabel="Preview the first sync"
          onSaved={firstSyncPreview}
          onCancel={() => setFirstRun(false)}
        />
        {plan && (
          <PlanCard
            plan={plan.data}
            taken={plan.taken}
            busy={busy}
            onApply={() => void apply()}
            onCancel={() => setPlan(null)}
          />
        )}
      </div>
    );
  }

  const where = status.transport ? placeOf(status.transport) : "";
  const lastText = status.lastSync ? whenText(status.lastSync.at) : "not yet";
  const remoteName = status.transport?.kind === "git" ? "Private Git remote" : "Folder";
  const upstreamOwns = status.parts.some((p) => p.handledBy);

  return (
    <div className="@container flex flex-col gap-4">
      <div>
        <p className="text-base font-semibold">Keep your machines in step</p>
        <p className="text-meta text-muted-foreground">
          Encrypted, and the same file as an export. A sync pull shows the plan before it
          changes anything.
        </p>
      </div>
      {status.paused ? (
        <StatusCard
          variant="warning"
          icon={<Lock />}
          title={
            <>
              Sync is paused: the keychain is locked{" "}
              <Badge variant="partial">Paused</Badge>
            </>
          }
          detail={`The 30-minute check skips while the keychain is locked and nothing is asked of you. Unlock the keychain, or press Sync now to be asked once. Nothing was pushed or applied while it was locked; last synced ${lastText}.`}
          actions={
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() => void syncNow()}
            >
              Sync now
            </Button>
          }
        />
      ) : (
        <StatusCard
          variant={status.enabled ? "success" : "info"}
          icon={status.enabled ? <CircleCheck /> : <Info />}
          title={
            <>
              Encrypted sync{" "}
              <Badge variant={status.enabled ? "success" : "off"}>
                {status.enabled ? "On" : "Off"}
              </Badge>{" "}
              <Badge variant="outline">{remoteName}</Badge>
            </>
          }
          detail={`${where}${status.transport?.kind === "git" ? ` · branch ${status.transport.branch}` : ""} · last synced ${lastText}`}
          actions={
            <>
              <Button disabled={busy || !status.enabled} onClick={() => void syncNow()}>
                <RefreshCw /> Sync now
              </Button>
              <Button variant="outline" onClick={() => setSettings(true)}>
                Settings…
              </Button>
              {status.undoable && (
                <Button variant="ghost" disabled={busy} onClick={() => void undo()}>
                  <Undo2 /> Undo the last sync
                </Button>
              )}
            </>
          }
        >
          <ul aria-label="Machines" className="flex flex-wrap gap-2">
            {status.machines.map((machine) => (
              <li
                key={machine.machine}
                className="rounded-full bg-secondary px-2.5 py-0.5 text-xs"
              >
                {machine.thisMachine
                  ? `This machine · ${machine.machine}`
                  : `${machine.machine} · ${whenText(machine.at)}`}
              </li>
            ))}
          </ul>
        </StatusCard>
      )}
      {status.paused && (
        <section className="flex flex-col gap-2 rounded-xl border bg-card p-4">
          <p className="text-sm font-semibold">{remoteName}</p>
          <p className="text-meta text-muted-foreground">
            {where}
            {status.transport?.kind === "git"
              ? ` · branch ${status.transport.branch}`
              : ""}{" "}
            · the passphrase is kept in this machine&apos;s keychain, so it cannot be read
            while the keychain is locked.
          </p>
          {preview && (
            <PushPreviewBlock preview={preview} busy={busy} onPush={() => void push()} />
          )}
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" size="sm" onClick={() => setSettings(true)}>
              Settings…
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={busy}
              onClick={() => void showPush()}
            >
              Show what would be pushed
            </Button>
          </div>
        </section>
      )}
      {upstreamOwns && (
        <StatusCard
          variant="warning"
          icon={<TriangleAlert />}
          title="Upstream Sync (Pro) is on and handles Servers."
          detail="Server names, commands and paths are sent to the hosted service as readable JSON. Toolport's encrypted sync keeps everything else and leaves the servers alone."
        />
      )}
      {plan && (
        <PlanCard
          plan={plan.data}
          taken={plan.taken}
          busy={busy}
          onApply={() => void apply()}
          onCancel={() => setPlan(null)}
        />
      )}
      <NeedsYou
        status={status}
        notes={notes}
        held={held}
        busy={busy}
        onThis={(id) => void chooseThis(id)}
        onOther={(id) => void chooseOther(id)}
      />
      <WhatSyncs
        status={status}
        counts={counts}
        busy={busy}
        onToggle={(id, on) => void toggle(id, on)}
      />
      <RepoHeads
        active={status.parts.some((part) => part.id === "serverRepos" && part.on)}
      />
      {!status.paused && (
        <Remote
          busy={busy}
          preview={preview}
          onChangePassphrase={() => setSettings(true)}
          onPreview={() => void showPush()}
          onPush={() => void push()}
        />
      )}
      <StatusCard
        variant="info"
        icon={<Info />}
        title={
          <>
            Upstream Sync (Toolport Pro){" "}
            <Badge variant="off">{status.upstream.active ? "On" : "Off"}</Badge>
          </>
        }
        detail="Hosted by a third party. Sends server definitions as readable JSON over TLS, not encrypted for you alone. Disabled in this build."
      />
      <Dialog open={settings} onOpenChange={setSettings}>
        <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
          <DialogHeader>
            <DialogTitle>Sync settings</DialogTitle>
            <DialogDescription>
              Where the sealed file is kept, the passphrase in this machine&apos;s
              keychain and the 30-minute check.
            </DialogDescription>
          </DialogHeader>
          <SetupForm
            status={status}
            submitLabel="Save"
            forgetOption
            onSaved={async () => {
              setSettings(false);
              await refresh();
            }}
            onCancel={() => setSettings(false)}
          />
          <DialogFooter>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() =>
                void guarded("Could not change sync", () =>
                  ctlData<SyncStatus>(setupArgv({ enabled: !status.enabled })),
                ).then(() => {
                  setSettings(false);
                  return refresh();
                })
              }
            >
              {status.enabled ? "Switch sync off" : "Switch sync on"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
