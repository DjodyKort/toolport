import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { errorText } from "../settings/useCtl";
import { clearMemory, clearSessions } from "./commands";
import type { MemoryClearData, PlanData, SessionsClearData } from "./data";
import { sizeOf } from "./format";

function PlanText({ plan }: { plan: PlanData }) {
  return (
    <>
      <p className="text-sm font-medium">{plan.summary}</p>
      <ul className="list-disc pl-5 text-meta text-muted-foreground">
        {plan.steps.map((step) => (
          <li key={`${step.op}${step.path}`}>{step.detail}</li>
        ))}
        {plan.warnings.map((warning) => (
          <li key={warning}>{warning}</li>
        ))}
      </ul>
    </>
  );
}

function Check({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-sm">
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
      {label}
    </label>
  );
}

export interface ClearMemoryDialogProps {
  folder: string;
  name: string;
  directory: string;
  onClose: () => void;
  /** Called with the finished clear so the caller can offer Undo. */
  onDone: (data: MemoryClearData) => void;
}

/** Shows what a clear would do before it does it: Toolport's trash by default, for good only
 * after the repository name is typed. */
export function ClearMemoryDialog({
  folder,
  name,
  directory,
  onClose,
  onDone,
}: ClearMemoryDialogProps) {
  const [forever, setForever] = useState(false);
  const [alsoOff, setAlsoOff] = useState(false);
  const [typed, setTyped] = useState("");
  const [preview, setPreview] = useState<MemoryClearData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    clearMemory(folder, { forever, confirm: name, alsoOff, dryRun: true }).then(
      (data) => alive && (setPreview(data), setError(null)),
      (cause: unknown) => alive && setError(errorText(cause)),
    );
    return () => {
      alive = false;
    };
  }, [folder, forever, alsoOff, name]);

  async function apply() {
    setBusy(true);
    try {
      onDone(await clearMemory(folder, { forever, confirm: typed, alsoOff }));
      onClose();
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  }

  const ready = !forever || typed === name;
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Clear auto memory of {name}</DialogTitle>
          <DialogDescription>
            Worktrees of this repository share the memory. To keep a copy first, copy{" "}
            {directory}.
          </DialogDescription>
        </DialogHeader>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        {preview && (
          <>
            <PlanText plan={preview.plan} />
            <ul aria-label="Files to clear" className="text-meta">
              {preview.files.map((f) => (
                <li key={f.path}>
                  {f.name} · {sizeOf(f.bytes)}
                </li>
              ))}
            </ul>
          </>
        )}
        <Check
          label="Also turn auto memory off"
          checked={alsoOff}
          onChange={setAlsoOff}
        />
        <Check
          label="Delete for good instead of the trash"
          checked={forever}
          onChange={setForever}
        />
        {forever && (
          <div className="flex flex-col gap-1">
            <Label htmlFor="clear-confirm">Type {name} to confirm</Label>
            <Input
              id="clear-confirm"
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
            />
          </div>
        )}
        <Button
          variant={forever ? "destructive" : "default"}
          disabled={busy || !ready || !preview}
          onClick={() => void apply()}
        >
          {forever ? "Delete for good" : "Move to trash"}
        </Button>
      </DialogContent>
    </Dialog>
  );
}

export interface ClearSessionsDialogProps {
  folder: string;
  name: string;
  onClose: () => void;
  onDone: (data: SessionsClearData) => void;
}

export function ClearSessionsDialog({
  folder,
  name,
  onClose,
  onDone,
}: ClearSessionsDialogProps) {
  const [days, setDays] = useState(7);
  const [to, setTo] = useState<"trash" | "archive">("trash");
  const [preview, setPreview] = useState<SessionsClearData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    clearSessions(folder, { olderThan: days, to, dryRun: true }).then(
      (data) => alive && (setPreview(data), setError(null)),
      (cause: unknown) => alive && setError(errorText(cause)),
    );
    return () => {
      alive = false;
    };
  }, [folder, days, to]);

  async function apply() {
    setBusy(true);
    try {
      onDone(await clearSessions(folder, { olderThan: days, to }));
      onClose();
    } catch (cause) {
      setError(errorText(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Clear sessions of {name}</DialogTitle>
          <DialogDescription>
            Sessions are moved, never copied, so resuming one still finds it exactly once.
          </DialogDescription>
        </DialogHeader>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <div className="flex items-center gap-2">
          <Label htmlFor="older-than">Older than (days)</Label>
          <Input
            id="older-than"
            type="number"
            min={0}
            className="w-24"
            value={days}
            onChange={(event) => setDays(Math.max(0, Number(event.target.value) || 0))}
          />
        </div>
        <div role="group" aria-label="Where the sessions go" className="flex gap-2">
          {(["trash", "archive"] as const).map((place) => (
            <Button
              key={place}
              size="sm"
              variant="outline"
              aria-pressed={to === place}
              onClick={() => setTo(place)}
            >
              {place === "trash" ? "Trash (30 days)" : "Archive"}
            </Button>
          ))}
        </div>
        {preview && (
          <>
            <PlanText plan={preview.plan} />
            <p className="text-meta">
              {preview.sessions.length} sessions · {sizeOf(preview.totalBytes)}
            </p>
          </>
        )}
        <Button
          disabled={busy || !preview || preview.sessions.length === 0}
          onClick={() => void apply()}
        >
          {to === "trash" ? "Move to trash" : "Move to archive"}
        </Button>
      </DialogContent>
    </Dialog>
  );
}
