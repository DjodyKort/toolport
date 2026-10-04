import { useEffect, useRef } from "react";
import { toast } from "sonner";
import { plusAuthNotifications } from "./api";

const POLL_MS = 60_000;
const TOAST_MS = 60_000;

/** Headless: tells the user once when a login starts needing attention. A hidden window
 * is skipped, so an edge is not spent on a toast nobody can see. */
export function AuthNotifier({ onReview }: { onReview?: () => void }) {
  const review = useRef(onReview);
  useEffect(() => {
    review.current = onReview;
  });

  useEffect(() => {
    let alive = true;
    const poll = () => {
      if (document.visibilityState === "hidden") return;
      plusAuthNotifications()
        .then((list) => {
          if (!alive) return;
          for (const n of list) {
            toast.warning(n.title, {
              id: n.dedupeKey,
              description: n.body,
              duration: TOAST_MS,
              action: review.current
                ? { label: "Review", onClick: () => review.current?.() }
                : undefined,
            });
          }
        })
        .catch(() => {});
    };
    poll();
    const timer = setInterval(poll, POLL_MS);
    document.addEventListener("visibilitychange", poll);
    return () => {
      alive = false;
      clearInterval(timer);
      document.removeEventListener("visibilitychange", poll);
    };
  }, []);

  return null;
}
