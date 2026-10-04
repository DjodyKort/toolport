import { useCallback, useState } from "react";
import { useCtlJob, type CtlJobControl } from "../ui";

export interface ProbeControl {
  job: CtlJobControl;
  /** Which run is going: `"all"` or a server id. */
  target: string | null;
  run: (options: { server?: string; force: boolean }) => Promise<void>;
  dismiss: () => void;
}

/** `auth probe`: every server or one, with `--force` when the cache should be ignored. A probe
 * only reads (tier read), so it runs at once; the lists are read again when it is done. */
export function useProbe(onSettled: () => void): ProbeControl {
  const job = useCtlJob();
  const [target, setTarget] = useState<string | null>(null);
  const { start, reset } = job;
  const run = useCallback(
    async ({ server, force }: { server?: string; force: boolean }) => {
      setTarget(server ?? "all");
      await start([
        "auth",
        "probe",
        ...(server ? ["--server", server] : []),
        ...(force ? ["--force"] : []),
      ]);
      onSettled();
    },
    [start, onSettled],
  );
  const dismiss = useCallback(() => {
    reset();
    setTarget(null);
  }, [reset]);
  return { job, target, run, dismiss };
}
