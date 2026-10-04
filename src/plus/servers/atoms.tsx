import { useId, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { ErrorState, ScreenSkeleton, errorText, type CtlQuery } from "../ui";
import { initials, stateLabel, stateTone, type ServerView, type Tone } from "./model";

const BADGE: Record<Tone, "success" | "warning" | "destructive" | "secondary" | "info"> =
  {
    success: "success",
    warning: "warning",
    destructive: "destructive",
    secondary: "secondary",
    info: "info",
  };

export function StateBadge({ view }: { view: ServerView }) {
  return <Badge variant={BADGE[stateTone(view)]}>{stateLabel(view)}</Badge>;
}

export function Avatar({ name }: { name: string }) {
  return (
    <span
      aria-hidden="true"
      className="grid size-8 shrink-0 place-items-center rounded-md bg-secondary text-xs font-semibold text-muted-foreground"
    >
      {initials(name)}
    </span>
  );
}

export function Pill({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-full bg-secondary px-2 py-px text-xs font-medium text-muted-foreground",
        className,
      )}
    >
      {children}
    </span>
  );
}

export function Chip({ children }: { children: ReactNode }) {
  return (
    <span className="inline-flex items-center rounded-md border bg-background px-2 py-0.5 text-xs">
      {children}
    </span>
  );
}

export function Kv({
  rows,
  className,
}: {
  rows: Array<[string, ReactNode]>;
  className?: string;
}) {
  return (
    <dl
      className={cn(
        "grid grid-cols-[minmax(6rem,max-content)_minmax(0,1fr)] gap-x-4 gap-y-2.5 text-sm",
        className,
      )}
    >
      {rows.map(([label, value]) => (
        <div key={label} className="contents">
          <dt className="text-muted-foreground">{label}</dt>
          <dd className="min-w-0 break-words">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

export function Code({ children }: { children: ReactNode }) {
  return (
    <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs break-all">
      {children}
    </code>
  );
}

/** Shows the loading skeleton until every read has an answer, the failure of the first one
 * that failed (with Retry) while one is missing, and a stale-data note when a refresh fails
 * but an older answer is still there. */
export function Gate({
  queries,
  title,
  context,
  children,
}: {
  queries: Array<CtlQuery<unknown>>;
  title: string;
  context: string;
  children: () => ReactNode;
}) {
  const failed = queries.find((query) => query.status === "error");
  const missing = queries.some((query) => query.data === null);
  const retry = () =>
    queries.forEach((query) => query.status === "error" && query.reload());
  if (missing) {
    if (!failed) return <ScreenSkeleton label="Loading" />;
    const unreachable = errorText(failed.error).code === "bridge";
    return (
      <ErrorState
        error={failed.error}
        title={unreachable ? "Toolport could not run toolportctl" : title}
        context={context}
        onRetry={retry}
      />
    );
  }
  return (
    <div className="flex flex-col gap-4">
      {failed && (
        <Callout
          variant="warning"
          role="status"
          className="flex flex-wrap items-center gap-2"
        >
          <span>
            Could not refresh, showing the last answer: {errorText(failed.error).message}
          </span>
          <Button size="sm" variant="outline" onClick={retry}>
            Retry
          </Button>
        </Callout>
      )}
      {children()}
    </div>
  );
}

/** A label tied to its control; `children` gets the id to put on the control. */
export function Field({
  label,
  hint,
  children,
  className,
}: {
  label: string;
  hint?: string;
  children: (id: string, describedBy: string | undefined) => ReactNode;
  className?: string;
}) {
  const id = useId();
  const hintId = hint ? `${id}-hint` : undefined;
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
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

export const SELECT_CLASS =
  "h-8 w-full rounded-lg border border-input bg-transparent px-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30";

export function Problems({ items }: { items: string[] }) {
  if (items.length === 0) return null;
  return (
    <ul role="alert" className="list-disc pl-5 text-xs text-destructive">
      {items.map((item) => (
        <li key={item}>{item}</li>
      ))}
    </ul>
  );
}
