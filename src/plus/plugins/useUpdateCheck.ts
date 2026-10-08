import { useCallback, useEffect, useState } from "react";
import { ctlData } from "../bridge/ctl";

const FRESH_MS = 30 * 60_000;
let checkedAt = 0;

/** Asks Claude Code's marketplaces what is new (`cc list --refresh`), then lets the screen read
 * its lists again. The plugin lists answer from the marketplaces' last refresh, so without this
 * a plugin looks up to date until someone runs an update. It runs when a screen opens, at most
 * every half hour, and again on request. */
export function usePluginUpdateCheck(onChecked: () => void) {
  const [checking, setChecking] = useState(false);
  const run = useCallback(() => {
    setChecking(true);
    ctlData(["cc", "list", "--refresh"])
      .catch(() => undefined)
      .finally(() => {
        checkedAt = Date.now();
        setChecking(false);
        onChecked();
      });
  }, [onChecked]);
  useEffect(() => {
    if (Date.now() - checkedAt > FRESH_MS) run();
    // Once per mount: `run` changes with the reload callback of the screen.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return { checking, run };
}

export function forgetPluginUpdateCheck() {
  checkedAt = 0;
}
