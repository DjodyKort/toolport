import { useId, type ReactNode } from "react";
import { Terminal, WifiOff } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Input } from "@/components/ui/input";
import { CopyButton } from "../ui";
import type { Tone } from "./model";

export function Card({
  title,
  actions,
  children,
}: {
  title: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section
      role="group"
      aria-labelledby={id}
      className="flex min-w-0 flex-col gap-3 rounded-lg border bg-card p-4"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id={id} className="text-sm font-semibold">
          {title}
        </h3>
        {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
      </div>
      {children}
    </section>
  );
}

export function Kv({ rows }: { rows: Array<[string, ReactNode]> }) {
  return (
    <dl className="grid grid-cols-[minmax(7rem,max-content)_minmax(0,1fr)] gap-x-4 gap-y-1.5 text-sm">
      {rows.map(([key, value]) => (
        <div key={key} className="contents">
          <dt className="text-muted-foreground">{key}</dt>
          <dd className="min-w-0 break-words">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

export function Tag({ tone, children }: { tone: Tone; children: ReactNode }) {
  return <Badge variant={tone}>{children}</Badge>;
}

export function Mono({ children }: { children: ReactNode }) {
  return <code className="font-mono text-xs break-all">{children}</code>;
}

export function Field({
  label,
  value,
  onChange,
  placeholder,
  hint,
  error,
  disabled,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  hint?: string;
  error?: string | null;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <label htmlFor={id} className="text-sm font-medium">
        {label}
      </label>
      <Input
        id={id}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        autoComplete="off"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        disabled={disabled}
        aria-invalid={error ? true : undefined}
        aria-describedby={hint || error ? `${id}-hint` : undefined}
      />
      {(error || hint) && (
        <p
          id={`${id}-hint`}
          className={error ? "text-xs text-destructive" : "text-xs text-muted-foreground"}
        >
          {error ?? hint}
        </p>
      )}
    </div>
  );
}

export function Toggle({
  label,
  checked,
  onChange,
  hint,
  disabled,
}: {
  label: ReactNode;
  checked: boolean;
  onChange: (checked: boolean) => void;
  hint?: ReactNode;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="flex items-start gap-2 text-sm">
      <input
        id={id}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
        className="mt-1 size-4 accent-[var(--primary)]"
      />
      <label htmlFor={id} className="flex min-w-0 flex-col">
        <span>{label}</span>
        {hint && <span className="text-xs text-muted-foreground">{hint}</span>}
      </label>
    </div>
  );
}

/** A command that only a terminal can run, or a script outside the app: the exact line, a
 * copy button and the Open in Terminal button, which stays off until the app can start one. */
export function TerminalCommand({ line, note }: { line: string; note?: string }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2">
        <code
          aria-label="Command line"
          className="min-w-0 rounded bg-muted px-2 py-1 font-mono text-xs break-all"
        >
          {line}
        </code>
        <CopyButton text={line} label="Copy command" />
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled
          title="Opening a terminal from the app is not available yet. Copy the command and run it there."
        >
          <Terminal /> Open in Terminal
        </Button>
      </div>
      {note && <p className="text-xs text-muted-foreground">{note}</p>}
    </div>
  );
}

/** Shown while the machine reports no connection: checks that reach a remote will fail. */
export function OfflineNote() {
  if (typeof navigator === "undefined" || navigator.onLine) return null;
  return (
    <Callout variant="warning" role="status">
      <span className="flex items-center gap-2">
        <WifiOff className="size-4" aria-hidden="true" /> You are offline. Checks and
        updates that reach a remote fail until you are back online.
      </span>
    </Callout>
  );
}

export function Intro({ children }: { children: ReactNode }) {
  return <p className="text-sm text-muted-foreground">{children}</p>;
}
