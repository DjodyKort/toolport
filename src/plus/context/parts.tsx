import { createContext, useContext, useId, useState, type ReactNode } from "react";
import { RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { CommandRow } from "../bridge/data";
import { AsyncView, type CtlQuery } from "../ui";
import type { CheckLevel, Check } from "./model";

export const RowsContext = createContext<CommandRow[] | null>(null);
export const useRows = () => useContext(RowsContext);
/** Reads the command list again; a section's Retry calls it while the list is missing, so a
 * screen that opened while toolportctl was down can write once it is back. */
export const RowsReloadContext = createContext<() => void>(() => {});

export function Section({
  title,
  hint,
  actions,
  children,
}: {
  title: string;
  hint?: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section
      aria-labelledby={id}
      className="flex flex-col gap-3 rounded-lg border bg-card p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <h3 id={id} className="text-sm font-semibold">
            {title}
          </h3>
          {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
        </div>
        {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
      </div>
      {children}
    </section>
  );
}

/** A section that reads one command: skeleton, error with Retry, empty and the content. */
export function QuerySection<T>({
  title,
  hint,
  actions,
  query,
  isEmpty,
  empty,
  children,
}: {
  title: string;
  hint?: string;
  actions?: ReactNode;
  query: CtlQuery<T>;
  isEmpty?: (data: T) => boolean;
  empty?: ReactNode;
  children: (data: T) => ReactNode;
}) {
  const rows = useRows();
  const reloadRows = useContext(RowsReloadContext);
  const shown: CtlQuery<T> = rows
    ? query
    : {
        ...query,
        reload: () => {
          query.reload();
          reloadRows();
        },
      };
  return (
    <Section
      title={title}
      hint={hint}
      actions={
        <>
          {actions}
          <Button
            size="xs"
            variant="ghost"
            aria-label={`Read ${title.toLowerCase()} again`}
            onClick={shown.reload}
          >
            <RefreshCw />
          </Button>
        </>
      }
    >
      <AsyncView
        query={shown}
        errorTitle={`Couldn't read ${title.toLowerCase()}`}
        isEmpty={isEmpty}
        empty={empty}
      >
        {children}
      </AsyncView>
    </Section>
  );
}

export function Code({ children }: { children: ReactNode }) {
  return (
    <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs break-all">
      {children}
    </code>
  );
}

export function Kv({ rows }: { rows: Array<[string, ReactNode]> }) {
  return (
    <dl className="grid grid-cols-[minmax(6rem,max-content)_minmax(0,1fr)] gap-x-4 gap-y-2 text-sm">
      {rows.map(([label, value]) => (
        <div key={label} className="contents">
          <dt className="text-muted-foreground">{label}</dt>
          <dd className="min-w-0 break-words">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

const LEVEL: Record<
  CheckLevel,
  { variant: "success" | "warning" | "destructive"; word: string }
> = {
  ok: { variant: "success", word: "OK" },
  warn: { variant: "warning", word: "Look" },
  fail: { variant: "destructive", word: "Fail" },
};

export function Checks({ checks, label }: { checks: Check[]; label: string }) {
  if (checks.length === 0) return null;
  return (
    <ul aria-label={label} className="flex flex-col gap-1.5 text-sm">
      {checks.map(([level, text], i) => (
        <li key={i} className="flex items-baseline gap-2">
          <Badge variant={LEVEL[level]?.variant ?? "secondary"}>
            {LEVEL[level]?.word ?? level}
          </Badge>
          <span className="min-w-0 break-words">{text}</span>
        </li>
      ))}
    </ul>
  );
}

export const SELECT_CLASS =
  "h-8 w-full rounded-lg border border-input bg-transparent px-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30";

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: (id: string, describedBy: string | undefined) => ReactNode;
}) {
  const id = useId();
  const hintId = hint ? `${id}-hint` : undefined;
  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={id} className="text-sm font-medium">
        {label}
      </label>
      {children(id, hintId)}
      {hint && (
        <p id={hintId} className="text-xs text-muted-foreground">
          {hint}
        </p>
      )}
    </div>
  );
}

export function CheckField({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="flex items-start gap-2 text-sm">
      <input
        type="checkbox"
        className="mt-0.5"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span>
        {label}
        {hint && <span className="block text-xs text-muted-foreground">{hint}</span>}
      </span>
    </label>
  );
}

/** A small form in a dialog. `onSubmit` closes it by the caller, usually by starting a write. */
export function FormDialog({
  title,
  intro,
  submitLabel,
  valid = true,
  onSubmit,
  onClose,
  children,
}: {
  title: string;
  intro?: string;
  submitLabel: string;
  valid?: boolean;
  onSubmit: () => void;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent aria-describedby={undefined} className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (valid) onSubmit();
          }}
        >
          {intro && <p className="text-sm text-muted-foreground">{intro}</p>}
          {children}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!valid}>
              {submitLabel}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Which dialog of a screen is open. */
export function useDialog<T extends string>() {
  const [open, setOpen] = useState<T | null>(null);
  return { open, show: setOpen, close: () => setOpen(null) };
}
