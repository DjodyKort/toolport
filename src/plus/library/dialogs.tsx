import { useId, useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { toastError } from "@/lib/toast";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import type { WritePlan } from "./data";

export function Mono({ children }: { children: ReactNode }) {
  return (
    <code className="font-mono text-xs break-all text-muted-foreground">{children}</code>
  );
}

export function PlanSteps({ plan }: { plan: WritePlan }) {
  return (
    <div className="flex flex-col gap-2 text-meta">
      <p className="text-sm font-medium">{plan.summary}</p>
      <ul className="flex flex-col gap-1.5" aria-label="Plan">
        {plan.steps.map((step, index) => (
          <li key={`${step.detail}-${index}`} className="flex flex-col gap-0.5">
            <span>{step.detail}</span>
            {step.path && <Mono>{step.path}</Mono>}
          </li>
        ))}
      </ul>
      {plan.warnings.map((warning) => (
        <p key={warning} className="text-destructive">
          {warning}
        </p>
      ))}
      {plan.undo && (
        <p className="text-muted-foreground">
          To undo later: <Mono>{plan.undo}</Mono>
        </p>
      )}
    </div>
  );
}

export function FileLines({ title, files }: { title?: string; files: string[] }) {
  if (files.length === 0) return null;
  return (
    <div className="flex flex-col gap-1 text-meta">
      {title && <p className="font-medium">{title}</p>}
      <ul className="flex max-h-48 flex-col gap-0.5 overflow-y-auto" aria-label={title}>
        {files.map((file) => (
          <li key={file}>
            <Mono>{file}</Mono>
          </li>
        ))}
      </ul>
    </div>
  );
}

export interface WriteDialogProps<T> {
  title: string;
  description?: string;
  confirmLabel: string;
  destructive?: boolean;
  /** Runs the same command with `--dry-run`: what the dialog shows before anything is written. */
  preview: () => Promise<T>;
  render: (data: T, reload: () => void) => ReactNode;
  /** The reason the write cannot run, shown instead of the button's effect. */
  blocked?: (data: T) => string | null;
  /** An explicit tick the write needs before it can run. */
  tick?: (data: T) => string | null;
  confirm: () => Promise<unknown>;
  onClose: () => void;
  onDone: () => void;
}

/** The preview first, then the write: Enter confirms the primary action, Esc cancels. */
export function WriteDialog<T>({
  title,
  description,
  confirmLabel,
  destructive,
  preview,
  render,
  blocked,
  tick,
  confirm,
  onClose,
  onDone,
}: WriteDialogProps<T>) {
  const data = useLoad(preview, title);
  const [busy, setBusy] = useState(false);
  const [ticked, setTicked] = useState(false);
  const tickId = useId();
  const reason = data.data && blocked ? blocked(data.data) : null;
  const tickText = data.data && tick ? tick(data.data) : null;

  async function go() {
    setBusy(true);
    try {
      await confirm();
      onDone();
      onClose();
    } catch (error) {
      toastError("Nothing was changed", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>
            {description ??
              "The preview comes first. Nothing is written until you confirm."}
          </DialogDescription>
        </DialogHeader>
        {data.error && !data.data && <p role="alert">{data.error}</p>}
        {!data.data && !data.error && <p className="text-meta">Preparing the preview…</p>}
        {data.data && render(data.data, data.reload)}
        {tickText && (
          <label htmlFor={tickId} className="flex items-start gap-2 text-meta">
            <input
              id={tickId}
              type="checkbox"
              className="mt-0.5"
              checked={ticked}
              onChange={(event) => setTicked(event.target.checked)}
            />
            <span>{tickText}</span>
          </label>
        )}
        {reason && (
          <p role="alert" className="text-meta text-destructive">
            {reason}
          </p>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant={destructive ? "destructive" : "default"}
            data-confirm
            autoFocus
            disabled={
              !data.data || busy || reason !== null || (tickText !== null && !ticked)
            }
            onClick={() => void go()}
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export interface AskDialogProps {
  title: string;
  description: string;
  label: string;
  placeholder?: string;
  initial?: string;
  continueLabel?: string;
  onClose: () => void;
  /** The dialog that follows once a value is typed. */
  next: (value: string, back: () => void) => ReactNode;
}

/** One typed value, then the preview of what it would do. */
export function AskDialog({
  title,
  description,
  label,
  placeholder,
  initial = "",
  continueLabel = "Preview",
  onClose,
  next,
}: AskDialogProps) {
  const [value, setValue] = useState(initial);
  const [asked, setAsked] = useState<string | null>(null);
  const id = useId();
  if (asked !== null) return <>{next(asked, () => setAsked(null))}</>;
  const text = value.trim();
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (text) setAsked(text);
          }}
        >
          <label htmlFor={id} className="text-meta font-medium">
            {label}
          </label>
          <Input
            id={id}
            value={value}
            placeholder={placeholder}
            autoFocus
            onChange={(event) => setValue(event.target.value)}
          />
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!text}>
              {continueLabel}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
