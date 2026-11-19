import { lazy, Suspense, useEffect, useState } from "react";
import { refreshAttention } from "./attentionCtl";
import { REVIEW_EVENT, type Reviewed } from "./reviewTarget";

const RepoUpdateDialog = lazy(() => import("../servers/RepoUpdateDialog"));
const RepoSwitchDialog = lazy(() => import("../servers/RepoSwitchDialog"));
const PackageUpdateDialog = lazy(() => import("../servers/PackageUpdateDialog"));

export function ReviewDialog({
  reviewed: { server, verb, fetch },
  open,
  onOpenChange,
  onChanged = refreshAttention,
}: {
  reviewed: Reviewed;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChanged?: () => void;
}) {
  return (
    <Suspense fallback={null}>
      {verb === "switch" ? (
        <RepoSwitchDialog
          serverName={server}
          open={open}
          onOpenChange={onOpenChange}
          onChanged={onChanged}
        />
      ) : verb === "package" ? (
        <PackageUpdateDialog
          serverName={server}
          open={open}
          onOpenChange={onOpenChange}
          onChanged={onChanged}
        />
      ) : (
        <RepoUpdateDialog
          serverName={server}
          open={open}
          onOpenChange={onOpenChange}
          onChanged={onChanged}
          fetch={fetch}
        />
      )}
    </Suspense>
  );
}

/** Draws the plan dialog `requestReview` asks for. */
export function ReviewHost() {
  const [reviewed, setReviewed] = useState<Reviewed | null>(null);
  useEffect(() => {
    const listener = (event: Event) =>
      setReviewed((event as CustomEvent<Reviewed>).detail);
    window.addEventListener(REVIEW_EVENT, listener);
    return () => window.removeEventListener(REVIEW_EVENT, listener);
  }, []);
  if (!reviewed) return null;
  return (
    <ReviewDialog
      key={`${reviewed.verb}:${reviewed.server}`}
      reviewed={reviewed}
      open
      onOpenChange={(open) => !open && setReviewed(null)}
    />
  );
}
