import { lazy, Suspense } from "react";

const Bell = lazy(() =>
  import("./PlusFooterBell").then((m) => ({ default: m.PlusFooterBell })),
);

/** Loaded after the first paint, so the bell's feed and popover stay out of the startup bundle. */
export function PlusFooterBell() {
  return (
    <Suspense fallback={null}>
      <Bell />
    </Suspense>
  );
}
