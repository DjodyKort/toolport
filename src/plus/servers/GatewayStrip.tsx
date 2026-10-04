import { cn } from "@/lib/utils";
import { Skeleton } from "@/components/ui/skeleton";
import { ErrorState } from "../ui";
import { stripFacts, type StripFact } from "./model";
import { useServers } from "./useServers";

const DOT: Record<StripFact["tone"], string> = {
  success: "bg-success",
  warning: "bg-warning",
  destructive: "bg-destructive",
  muted: "bg-muted-foreground/50",
};

/** The four facts above the tabs: the gateway, how many tools it serves, the logins that
 * need you and how many clients go through it. */
export function GatewayStrip() {
  const { status, statusDoc, clients } = useServers();
  if (!statusDoc) {
    if (status.status === "error") {
      return (
        <ErrorState
          error={status.error}
          title="Couldn't read the gateway state"
          context="status"
          onRetry={status.reload}
        />
      );
    }
    return (
      <div
        role="status"
        aria-busy="true"
        aria-label="Loading the gateway state"
        className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4"
      >
        {[0, 1, 2, 3].map((i) => (
          <Skeleton key={i} className="h-16 rounded-lg" />
        ))}
      </div>
    );
  }
  const facts = stripFacts(statusDoc, clients.data?.clients ?? null);
  return (
    <section aria-label="Gateway" className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
      {facts.map((fact) => (
        <div
          key={fact.label}
          className="flex min-w-0 flex-col gap-0.5 rounded-lg border bg-card px-3 py-2"
        >
          <span className="text-xs text-muted-foreground">{fact.label}</span>
          <b className="flex items-center gap-2 text-sm font-semibold">
            <span
              className={cn("size-2 shrink-0 rounded-full", DOT[fact.tone])}
              aria-hidden="true"
            />
            {fact.value}
          </b>
          <small className="truncate text-xs text-muted-foreground">{fact.detail}</small>
        </div>
      ))}
    </section>
  );
}
