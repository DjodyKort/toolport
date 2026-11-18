import { Suspense, type ReactNode } from "react";
import { ErrorBoundary } from "@/components/ErrorBoundary";

/** A fork row never takes the upstream page down with it: a crash in it leaves a quiet line. */
export function Guard({ name, children }: { name: string; children: ReactNode }) {
  return (
    <ErrorBoundary
      fallback={(error) => (
        <p className="text-meta text-muted-foreground" role="alert">
          {name} could not be drawn: {error.message}
        </p>
      )}
    >
      <Suspense fallback={null}>{children}</Suspense>
    </ErrorBoundary>
  );
}
