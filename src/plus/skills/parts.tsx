import { useId, type ReactNode } from "react";
import { RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { Origin } from "../bridge/data";
import { AsyncView, CopyButton, type CtlQuery } from "../ui";
import { kindTone, plural, type LintMessage } from "./model";

export function Section({
  title,
  count,
  children,
}: {
  title: string;
  count?: number;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="flex flex-col gap-3">
      <h3 id={id} className="flex items-center gap-2 text-sm font-semibold">
        {title}
        {count != null && (
          <span className="rounded-full bg-secondary px-2 py-px text-2xs font-semibold text-muted-foreground tabular-nums">
            {count}
          </span>
        )}
      </h3>
      {children}
    </section>
  );
}

export function SourceBadge({ origin }: { origin: Origin }) {
  return (
    <Badge variant={kindTone(origin.kind)} title={origin.name}>
      {origin.kind}
    </Badge>
  );
}

export function Stat({
  label,
  value,
  note,
  warn,
}: {
  label: string;
  value: ReactNode;
  note?: string;
  warn?: boolean;
}) {
  return (
    <div role="group" aria-label={label} className="rounded-lg border bg-card p-3">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className="text-lg font-semibold">
        {warn && (
          <span
            className="mr-1.5 inline-block size-2 rounded-full bg-warning"
            aria-hidden="true"
          />
        )}
        {value}
      </p>
      {note && <p className="text-xs text-muted-foreground">{note}</p>}
    </div>
  );
}

export function Card<T>({
  title,
  query,
  id,
  children,
}: {
  title: string;
  query: CtlQuery<T>;
  /** Makes the card something a link can bring focus to. */
  id?: string;
  children: (data: T) => ReactNode;
}) {
  return (
    <div
      id={id}
      tabIndex={id ? -1 : undefined}
      role="group"
      aria-label={title}
      className="rounded-lg border bg-card p-4 outline-none focus-visible:ring-1 focus-visible:ring-ring"
    >
      <div className="mb-2 flex items-center justify-between gap-2">
        <h4 className="text-sm font-medium">{title}</h4>
        <Button
          size="xs"
          variant="ghost"
          aria-label={`Check ${title.toLowerCase()} again`}
          onClick={query.reload}
        >
          <RefreshCw />
        </Button>
      </div>
      <AsyncView query={query} errorTitle={`Couldn't check ${title.toLowerCase()}`}>
        {children}
      </AsyncView>
    </div>
  );
}

export function Verdict({ ok, children }: { ok: boolean; children: ReactNode }) {
  return (
    <p className="flex items-center gap-2 text-sm">
      <Badge variant={ok ? "success" : "warning"}>{ok ? "OK" : "Look"}</Badge>
      <span>{children}</span>
    </p>
  );
}

export function PathLine({ path }: { path: string }) {
  return (
    <span className="flex flex-wrap items-center gap-2">
      <code className="font-mono text-xs break-all text-muted-foreground">{path}</code>
      <CopyButton text={path} label="Copy path" size="xs" />
    </span>
  );
}

const LEVEL: Record<string, "destructive" | "warning" | "info"> = {
  error: "destructive",
  warning: "warning",
  info: "info",
};

export function Messages({ messages }: { messages: LintMessage[] }) {
  return (
    <ul className="flex flex-col gap-1.5 text-sm">
      {messages.map((m, i) => (
        <li key={i} className="flex flex-wrap items-baseline gap-2">
          <Badge variant={LEVEL[m.level] ?? "secondary"}>{m.level}</Badge>
          <code className="font-mono text-xs">{m.name}</code>
          <span className="min-w-0 break-words">{m.message}</span>
        </li>
      ))}
    </ul>
  );
}

export function LintSummary({
  data,
  noun,
}: {
  data: { errors: number; warnings: number; messages: LintMessage[] };
  noun: string;
}) {
  return (
    <div className="flex flex-col gap-2">
      <Verdict ok={data.errors === 0 && data.warnings === 0}>
        {data.messages.length === 0
          ? `No problems in ${noun}`
          : `${plural(data.errors, "error")}, ${plural(data.warnings, "warning")}, ${plural(data.messages.length - data.errors - data.warnings, "note")}`}
      </Verdict>
      <Messages messages={data.messages} />
    </div>
  );
}

export function Notes({ items }: { items: string[] }) {
  if (items.length === 0) return null;
  return (
    <ul className="list-disc pl-4 text-xs text-warning" aria-label="Notes">
      {items.map((item, i) => (
        <li key={i}>{item}</li>
      ))}
    </ul>
  );
}
