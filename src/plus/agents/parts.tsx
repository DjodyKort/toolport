import { useId, type ReactNode } from "react";
import { RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { AsyncView, CopyButton, type CtlQuery } from "../ui";
import { clientName, plural, type LintMessage } from "./model";

export function Section({
  title,
  count,
  actions,
  children,
}: {
  title: string;
  count?: number;
  actions?: ReactNode;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section aria-labelledby={id} className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 id={id} className="flex items-center gap-2 text-sm font-semibold">
          {title}
          {count != null && (
            <span className="rounded-full bg-secondary px-2 py-px text-2xs font-semibold text-muted-foreground tabular-nums">
              {count}
            </span>
          )}
        </h3>
        {actions && <div className="flex flex-wrap gap-2">{actions}</div>}
      </div>
      {children}
    </section>
  );
}

export function Card<T>({
  title,
  query,
  children,
}: {
  title: string;
  query: CtlQuery<T>;
  children: (data: T) => ReactNode;
}) {
  return (
    <div role="group" aria-label={title} className="rounded-lg border bg-card p-4">
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

export function Chips({ items, empty }: { items: string[]; empty: string }) {
  if (items.length === 0) return <span className="text-muted-foreground">{empty}</span>;
  return (
    <span className="flex flex-wrap gap-1">
      {items.map((item) => (
        <Badge key={item} variant="secondary">
          {clientName(item)}
        </Badge>
      ))}
    </span>
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

export function Verdict({ ok, children }: { ok: boolean; children: ReactNode }) {
  return (
    <p className="flex items-center gap-2 text-sm">
      <Badge variant={ok ? "success" : "warning"}>{ok ? "OK" : "Look"}</Badge>
      <span>{children}</span>
    </p>
  );
}

interface LintData {
  messages: unknown[];
  errors: number;
  warnings: number;
  infos: number;
  discoveryWarnings: unknown[];
}

export function LintBody({ data, noun }: { data: LintData; noun: string }) {
  const found = data.errors + data.warnings + data.infos;
  return (
    <div className="flex flex-col gap-2">
      <Verdict ok={data.errors === 0 && data.warnings === 0}>
        {found === 0
          ? `No problems in ${noun}`
          : `${plural(data.errors, "error")}, ${plural(data.warnings, "warning")}, ${plural(data.infos, "note")}`}
      </Verdict>
      <Messages messages={data.messages as LintMessage[]} />
      <Discovery warnings={data.discoveryWarnings.map(String)} />
    </div>
  );
}

export function Discovery({ warnings }: { warnings: string[] }) {
  if (warnings.length === 0) return null;
  return (
    <ul
      className="list-disc pl-4 text-xs text-warning"
      aria-label="Files that did not load"
    >
      {warnings.map((w, i) => (
        <li key={i}>{w}</li>
      ))}
    </ul>
  );
}

interface DiffData {
  new: string[];
  modified: unknown[];
  removed: unknown[];
  unchanged: number;
  clean: boolean;
  noLockfile: boolean;
  discoveryWarnings: unknown[];
}

const MARKS = [
  ["new", "+", "text-success", "new"],
  ["modified", "~", "text-warning", "modified"],
  ["removed", "−", "text-destructive", "removed"],
] as const;

export function DiffBody({ data, noun }: { data: DiffData; noun: string }) {
  return (
    <div className="flex flex-col gap-2">
      <Verdict ok={data.clean && !data.noLockfile}>
        {data.noLockfile
          ? data.new.length === 0
            ? `Nothing synced yet, and no ${noun} to sync`
            : `Never synced: every ${noun} is new`
          : data.clean
            ? `No changes since the last sync`
            : `Changed since the last sync`}
      </Verdict>
      <ul className="flex flex-col gap-1 text-sm">
        {MARKS.flatMap(([key, glyph, tone, word]) =>
          data[key].map(String).map((name) => (
            <li key={`${key}-${name}`} className="flex gap-2">
              <span className={`w-4 text-center font-mono ${tone}`} aria-hidden="true">
                {glyph}
              </span>
              <code className="font-mono text-xs">{name}</code>
              <span className="text-muted-foreground">({word})</span>
            </li>
          )),
        )}
      </ul>
      {data.unchanged > 0 && (
        <p className="text-xs text-muted-foreground">{data.unchanged} unchanged</p>
      )}
      <Discovery warnings={data.discoveryWarnings.map(String)} />
    </div>
  );
}
