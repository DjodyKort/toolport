import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { ArrowLeft, CircleCheck, Info, TriangleAlert } from "lucide-react";
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
  restoreArgv,
  STATE_LABEL,
  type CheckData,
  type ExportData,
  type PartsData,
  type RestoreData,
} from "./model";

const SyncView = lazy(() => import("./SyncView"));

type View = "export" | "restore" | "reset" | "sync";

const VIEWS: { id: View; label: string }[] = [
  { id: "export", label: "Export" },
  { id: "restore", label: "Restore" },
  { id: "reset", label: "Before a reset" },
  { id: "sync", label: "Sync" },
];

function ExportView() {
  const [parts, setParts] = useState<PartsData | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);
  const [out, setOut] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [preview, setPreview] = useState<ExportData | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void ctlData<PartsData>(["setup", "parts"]).then(
      (data) => {
        setParts(data);
        setChosen(data.parts.filter((p) => p.default).map((p) => p.id));
      },
      (error) =>
        toastError("Could not read the setup parts", { description: errorText(error) }),
    );
  }, []);

  async function run(dryRun: boolean) {
    setBusy(true);
    try {
      const secrets = passphrase !== "";
      const data = await ctlData<ExportData>(
        exportArgv(chosen, out, { secrets, dryRun }),
        {
          stdinSecret: secrets ? passphrase : undefined,
        },
      );
      if (dryRun) setPreview(data);
      else toast.success(`Wrote ${data.file}`);
    } catch (error) {
      toastError("Could not export the setup", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  if (!parts)
    return <p className="text-meta text-muted-foreground">Loading the parts…</p>;
  const short = passphrase !== "" && passphrase.length < parts.minPassphrase;
  return (
    <div className="flex flex-col gap-4">
      <section aria-label="Parts">
        <SectionHeader>Parts</SectionHeader>
        <ul className="toolport-list">
          {parts.parts.map((part) => (
            <li key={part.id} className="flex items-center gap-3 px-4 py-2">
              <input
                type="checkbox"
                id={`part-${part.id}`}
                checked={chosen.includes(part.id)}
                onChange={(e) =>
                  setChosen(
                    e.target.checked
                      ? [...chosen, part.id]
                      : chosen.filter((id) => id !== part.id),
                  )
                }
              />
              <label htmlFor={`part-${part.id}`} className="flex-1 text-sm">
                {part.label}
                <span className="text-meta text-muted-foreground"> · {part.count}</span>
                {part.notes.map((note) => (
                  <span key={note} className="block text-meta text-muted-foreground">
                    {note}
                  </span>
                ))}
              </label>
            </li>
          ))}
        </ul>
      </section>
      <section aria-label="Secrets">
        <SectionHeader>Secrets</SectionHeader>
        <p className="text-meta text-muted-foreground">
          Secrets only leave sealed with a passphrase of at least {parts.minPassphrase}{" "}
          characters. Leave it empty to export without them.
        </p>
        <Input
          type="password"
          aria-label="Passphrase"
          value={passphrase}
          onChange={(e) => setPassphrase(e.target.value)}
        />
        {short && (
          <p role="alert" className="text-meta text-destructive">
            The passphrase is shorter than {parts.minPassphrase} characters.
          </p>
        )}
      </section>
      <section aria-label="File">
        <SectionHeader>File</SectionHeader>
        <Input
          aria-label="Write to"
          placeholder="/path/to/folder or file.zip"
          value={out}
          onChange={(e) => setOut(e.target.value)}
        />
      </section>
      {preview && (
        <StatusCard
          variant="info"
          icon={<Info />}
          title={`Would write ${preview.name}`}
          detail={`${preview.parts.length} parts, ${preview.fileSize} bytes`}
        />
      )}
      <div className="flex gap-2">
        <Button
          variant="outline"
          disabled={busy || short || !out || chosen.length === 0}
          onClick={() => void run(true)}
        >
          Preview
        </Button>
        <Button
          disabled={busy || short || !out || chosen.length === 0}
          onClick={() => void run(false)}
        >
          Write the file
        </Button>
      </div>
    </div>
  );
}

function RestoreView() {
  const [file, setFile] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [plan, setPlan] = useState<RestoreData | null>(null);
  const [busy, setBusy] = useState(false);
  const sealed = passphrase !== "";
  const options = { stdinSecret: sealed ? passphrase : undefined };

  async function check() {
    setBusy(true);
    try {
      setPlan(await ctlData<RestoreData>(restoreArgv(file, { sealed }), options));
    } catch (error) {
      setPlan(null);
      toastError("Could not read the file", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  async function apply() {
    if (!plan) return;
    setBusy(true);
    try {
      const done = await ctlData<RestoreData>(
        restoreArgv(file, { sealed, plan: plan.plan }),
        options,
      );
      setPlan(done);
      toast.success("Setup restored. Undo brings the files and settings back.");
    } catch (error) {
      toastError("Could not restore the setup", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  async function undo() {
    try {
      await ctlData(["setup", "undo"]);
      setPlan(null);
      toast.success("Restore undone");
    } catch (error) {
      toastError("Could not undo", { description: errorText(error) });
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <Input
        aria-label="Setup file"
        placeholder="/path/to/toolport-setup.zip"
        value={file}
        onChange={(e) => setFile(e.target.value)}
      />
      <Input
        type="password"
        aria-label="Passphrase"
        placeholder="Passphrase, only for a sealed file"
        value={passphrase}
        onChange={(e) => setPassphrase(e.target.value)}
      />
      <div className="flex gap-2">
        <Button variant="outline" disabled={busy || !file} onClick={() => void check()}>
          Check the file
        </Button>
        <Button variant="outline" onClick={() => void undo()}>
          Undo the last restore
        </Button>
      </div>
      {plan && (
        <section aria-label="Plan">
          <SectionHeader>{plan.applied ? "Restored" : "What would change"}</SectionHeader>
          <ul className="toolport-list">
            {plan.rows.map((row) => (
              <li key={row.id} className="flex items-center gap-3 px-4 py-2 text-sm">
                <span className="flex-1">
                  {row.label}
                  <span className="block text-meta text-muted-foreground">
                    {row.note}
                  </span>
                </span>
                <span className="text-meta">{STATE_LABEL[row.state]}</span>
              </li>
            ))}
          </ul>
          {!plan.applied && (
            <Button className="mt-3" disabled={busy} onClick={() => void apply()}>
              Restore
            </Button>
          )}
        </section>
      )}
    </div>
  );
}

function ResetView() {
  const [check, setCheck] = useState<CheckData | null>(null);

  const load = useCallback(async (extra: string[] = []) => {
    try {
      setCheck(await ctlData<CheckData>(["setup", "check", ...extra]));
    } catch (error) {
      toastError("Could not check the repositories", { description: errorText(error) });
    }
  }, []);

  useEffect(() => {
    void ctlData<CheckData>(["setup", "check"]).then(setCheck, (error) =>
      toastError("Could not check the repositories", { description: errorText(error) }),
    );
  }, []);

  if (!check) return <p className="text-meta text-muted-foreground">Checking…</p>;
  return (
    <div className="flex flex-col gap-4">
      <StatusCard
        variant={check.ready ? "success" : "warning"}
        icon={check.ready ? <CircleCheck /> : <TriangleAlert />}
        title={check.ready ? "Ready for a reset" : "Not ready for a reset"}
        detail={`${check.open.blockers} blockers and ${check.open.carry} things to carry over are open`}
      />
      <section aria-label="Blockers">
        <SectionHeader>Blockers</SectionHeader>
        <ul className="toolport-list">
          {[...check.blockers, ...check.amber].map((finding) => (
            <li key={finding.id} className="flex items-center gap-3 px-4 py-2 text-sm">
              <span className="flex-1">
                {finding.repo}
                <span className="block text-meta text-muted-foreground">
                  {finding.note}
                </span>
              </span>
              {!finding.acked && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => void load(["--ack", finding.id])}
                >
                  Acknowledge
                </Button>
              )}
            </li>
          ))}
        </ul>
      </section>
      <section aria-label="Carry over">
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

/** Settings > Toolport+ > Setup & backup: export a setup file, restore one from a plan, and the
 * checklist before a reset. */
export function SetupPage({ onBack }: { onBack: () => void }) {
  const [view, setView] = useState<View>("export");
  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-2">
        <Button variant="ghost" size="sm" onClick={onBack}>
          <ArrowLeft /> Back
        </Button>
        <h2 className="text-base font-semibold">Setup &amp; backup</h2>
      </div>
      <div role="tablist" className="flex gap-2">
        {VIEWS.map((entry) => (
          <Button
            key={entry.id}
            role="tab"
            aria-selected={view === entry.id}
            variant={view === entry.id ? "default" : "outline"}
            size="sm"
            onClick={() => setView(entry.id)}
          >
            {entry.label}
          </Button>
        ))}
      </div>
      {view === "export" && <ExportView />}
      {view === "restore" && <RestoreView />}
      {view === "reset" && <ResetView />}
      {view === "sync" && (
        <Suspense
          fallback={<p className="text-meta text-muted-foreground">Loading sync…</p>}
        >
          <SyncView />
        </Suspense>
      )}
    </div>
  );
}
