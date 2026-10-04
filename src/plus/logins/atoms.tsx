import { type ReactNode } from "react";
import { WifiOff } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { ErrorState, ScreenSkeleton, errorText, type CtlQuery } from "../ui";
import { formatWhen, isoWhen, stateLabel, stateTone, type RowState } from "./model";

export function StateBadge({ state }: { state: RowState }) {
  return (
    <Badge variant={stateTone(state)} data-state={state}>
      {stateLabel(state)}
    </Badge>
  );
}

export function WhenText({ seconds }: { seconds: number | null | undefined }) {
  if (!seconds) return <span className="text-muted-foreground">never</span>;
  return (
    <time dateTime={isoWhen(seconds)} title={isoWhen(seconds)}>
      {formatWhen(seconds)}
    </time>
  );
}

/** The toolportctl child process itself could not run: nothing on these tabs can be read or
 * changed until it does, which is why it is told apart from a command that failed. */
export function Offline({
  error,
  onRetry,
  onOpenCommands,
}: {
  error: unknown;
  onRetry: () => void;
  onOpenCommands?: (group?: string) => void;
}) {
  return (
    <Callout variant="warning" role="alert" className="flex flex-col gap-2">
      <p className="flex items-center gap-2 font-medium">
        <WifiOff className="size-4" aria-hidden="true" /> Toolport can't run toolportctl
      </p>
      <p className="text-sm break-words">{errorText(error).message}</p>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" variant="outline" onClick={onRetry}>
          Retry
        </Button>
        {onOpenCommands && (
          <Button size="sm" variant="outline" onClick={() => onOpenCommands("doctor")}>
            Open doctor
          </Button>
        )}
      </div>
    </Callout>
  );
}

/** Loading skeleton until every read has an answer; the failure of the first read that
 * failed (offline when the child process could not run); a stale-data note when a refresh
 * fails but an older answer is on screen. */
export function Gate({
  queries,
  pending = false,
  title,
  context,
  onOpenCommands,
  children,
}: {
  queries: Array<CtlQuery<unknown>>;
  /** Further reads that are still running. */
  pending?: boolean;
  title: string;
  context: string;
  onOpenCommands?: (group?: string) => void;
  children: () => ReactNode;
}) {
  const failed = queries.find((query) => query.status === "error");
  const missing = queries.some((query) => query.data === null);
  const retry = () =>
    queries.forEach((query) => query.status === "error" && query.reload());
  if (missing) {
    if (!failed) return <ScreenSkeleton label="Loading logins" />;
    return errorText(failed.error).code === "bridge" ? (
      <Offline error={failed.error} onRetry={retry} onOpenCommands={onOpenCommands} />
    ) : (
      <ErrorState error={failed.error} title={title} context={context} onRetry={retry} />
    );
  }
  if (pending) return <ScreenSkeleton label="Loading logins" />;
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

export function Stat({
  label,
  tone,
  value,
  note,
}: {
  label: string;
  tone?: "ok" | "warn";
  value: ReactNode;
  note?: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1 rounded-xl border bg-card px-3.5 py-3">
      <span className="text-xs text-muted-foreground">{label}</span>
      <b className="flex items-center gap-2 text-[0.95rem] font-semibold tabular-nums">
        {tone && (
          <span
            aria-hidden="true"
            className={`size-2 rounded-full ${tone === "ok" ? "bg-success" : "bg-warning"}`}
          />
        )}
        {value}
      </b>
      {note && <small className="text-xs text-muted-foreground">{note}</small>}
    </div>
  );
}
