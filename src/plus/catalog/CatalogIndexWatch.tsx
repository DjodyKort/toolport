import { useEffect } from "react";
import { plusCatalogIndexTick } from "../api";

const POLL_MS = 30 * 60_000;

/** Unlike the update watch it also runs while the window is hidden: it raises nothing, and a
 * tray-only app should still keep the index fresh. */
export function CatalogIndexWatcher() {
  useEffect(() => {
    const poll = () => {
      plusCatalogIndexTick().catch(() => {});
    };
    poll();
    const timer = setInterval(poll, POLL_MS);
    const onVisible = () => {
      if (document.visibilityState === "visible") poll();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, []);

  return null;
}
