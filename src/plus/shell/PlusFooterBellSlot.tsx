import { lazy, Suspense, useEffect } from "react";

const Bell = lazy(() =>
  import("./PlusFooterBell").then((m) => ({ default: m.PlusFooterBell })),
);

/** Loaded after the first paint, so the bell's feed and popover stay out of the startup bundle. The
 * slot also starts the production feed, which keeps the bell out until `attention ls` answers. */
export function PlusFooterBell() {
  useEffect(() => {
    let stop: (() => void) | undefined;
    let unmounted = false;
    void import("./ctlAttentionFeed").then((m) => {
      if (!unmounted) stop = m.startAttentionFeed();
    });
    return () => {
      unmounted = true;
      stop?.();
    };
  }, []);
  return (
    <Suspense fallback={null}>
      <Bell />
    </Suspense>
  );
}
