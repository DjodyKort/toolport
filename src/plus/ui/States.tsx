import { type ReactNode } from "react";
import { Copy, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Skeleton } from "@/components/ui/skeleton";
import { diagnosticsText, errorText } from "./errorText";
import type { CtlQuery } from "./useCtlQuery";

export function ScreenSkeleton({
  rows = 4,
  label = "Loading",
  className,
}: {
  rows?: number;
  label?: string;
  className?: string;
}) {
  return (
    <div
      role="status"
      aria-busy="true"
      aria-label={label}
      className={cn("flex flex-col gap-2", className)}
    >
      {Array.from({ length: rows }).map((_, i) => (
        <Skeleton key={i} className="h-11 w-full rounded-lg" />
      ))}
    </div>
  );
}

/** A failure a screen can always show: what went wrong in the CLI's own words, Retry, and a
 * copy button whose text holds the title, code and message and nothing else. */
export function ErrorState({
  error,
  title = "Couldn't load this",
  onRetry,
  context,
  className,
}: {
  error: unknown;
  title?: string;
  onRetry?: () => void;
  /** One line added to the diagnostics, e.g. the command id. */
  context?: string;
  className?: string;
}) {
  const { code, message } = errorText(error);
  async function copy() {
    try {
      await navigator.clipboard.writeText(diagnosticsText(title, error, context));
      toast.success("Diagnostics copied, paste them into your bug report");
    } catch {
      toastError("Couldn't copy diagnostics");
    }
  }
  return (
    <Callout
      variant="danger"
      role="alert"
      className={cn("flex flex-col gap-2", className)}
    >
      <p className="font-medium">{title}</p>
      <p className="text-sm break-words">
        {code && <code className="mr-1.5 font-mono text-xs">{code}</code>}
        {message}
      </p>
      <div className="flex flex-wrap gap-2">
        {onRetry && (
          <Button size="sm" variant="outline" onClick={onRetry}>
            <RefreshCw /> Retry
          </Button>
        )}
        <Button size="sm" variant="outline" onClick={() => void copy()}>
          <Copy /> Copy diagnostics
        </Button>
      </div>
    </Callout>
  );
}

/** The four states of a screen that reads one command: loading skeleton, error with Retry,
 * empty (offering the action that fills it) and the content. Nothing is ever blank. */
export function AsyncView<T>({
  query,
  errorTitle,
  context,
  isEmpty,
  empty,
  skeleton,
  children,
}: {
  query: CtlQuery<T>;
  errorTitle?: string;
  context?: string;
  isEmpty?: (data: T) => boolean;
  empty?: ReactNode;
  skeleton?: ReactNode;
  children: (data: T) => ReactNode;
}) {
  const { status, data, error, reload } = query;
  const failure =
    status === "error" ? (
      <ErrorState error={error} title={errorTitle} context={context} onRetry={reload} />
    ) : null;
  if (data === null) {
    return failure ?? skeleton ?? <ScreenSkeleton />;
  }
  if (isEmpty?.(data) && !failure) return <>{empty}</>;
  return (
    <div className="flex flex-col gap-4">
      {failure}
      {children(data)}
    </div>
  );
}
