import { useCallback, useEffect, useState } from "react";
import { CtlError, runCtl, type CtlResult } from "../bridge/ctl";
import type { CtlQuery } from "../ui";

type Settled<T> = { key: string; tick: number; data: T | null; error: unknown };

function failure(result: CtlResult): CtlError {
  const error = result.envelope?.error;
  return new CtlError(
    error?.message ?? result.parseError ?? `toolportctl exited with ${result.exitCode}`,
    error?.code ?? "bridge",
    result,
  );
}

/** One read command. `doctor` exits 1 with the code `unhealthy` when a check fails but still
 * prints its checks, so that data is the answer, not an error. */
export function useRead<T>(argv: readonly string[]): CtlQuery<T> {
  const key = JSON.stringify(argv);
  const [tick, setTick] = useState(0);
  const [settled, setSettled] = useState<Settled<T> | null>(null);

  useEffect(() => {
    let alive = true;
    runCtl<T>(JSON.parse(key) as string[]).result.then(
      (result) => {
        if (!alive) return;
        const data = result.envelope?.data ?? null;
        const failed =
          data === null ||
          (!!result.envelope?.error && result.envelope.error.code !== "unhealthy");
        setSettled({
          key,
          tick,
          data: data as T | null,
          error: failed ? failure(result) : null,
        });
      },
      (error) => alive && setSettled({ key, tick, data: null, error }),
    );
    return () => {
      alive = false;
    };
  }, [key, tick]);

  const reload = useCallback(() => setTick((n) => n + 1), []);
  const same = settled?.key === key;
  const fresh = same && settled.tick === tick;
  const status = !fresh ? "loading" : settled.error ? "error" : "ready";
  return {
    status,
    data: same ? settled.data : null,
    error: fresh ? settled.error : null,
    reload,
  };
}
