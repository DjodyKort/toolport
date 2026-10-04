import { useCallback, useEffect, useState } from "react";
import { ctlData } from "../bridge/ctl";

export type QueryStatus = "loading" | "ready" | "error";

export interface CtlQuery<T> {
  status: QueryStatus;
  /** The last answer for this argv; kept while a reload is running. */
  data: T | null;
  error: unknown;
  reload: () => void;
}

type Settled<T> = {
  key: string;
  tick: number;
  ok: boolean;
  data: T | null;
  error: unknown;
};

/** Runs one read command and keeps its answer. A failure is a state of its own, so a
 * screen built on it has something to show for every outcome (see `AsyncView`). */
export function useCtlQuery<T>(argv: readonly string[]): CtlQuery<T> {
  const key = JSON.stringify(argv);
  const [tick, setTick] = useState(0);
  const [settled, setSettled] = useState<Settled<T> | null>(null);

  useEffect(() => {
    let alive = true;
    ctlData<T>(JSON.parse(key) as string[]).then(
      (data) => alive && setSettled({ key, tick, ok: true, data, error: null }),
      (error) =>
        alive &&
        setSettled((prev) => ({
          key,
          tick,
          ok: false,
          data: prev?.key === key ? prev.data : null,
          error,
        })),
    );
    return () => {
      alive = false;
    };
  }, [key, tick]);

  const reload = useCallback(() => setTick((n) => n + 1), []);
  const sameArgv = settled?.key === key;
  const fresh = sameArgv && settled.tick === tick;
  const status: QueryStatus = !fresh ? "loading" : settled.ok ? "ready" : "error";
  return {
    status,
    data: sameArgv ? settled.data : null,
    error: fresh ? settled.error : null,
    reload,
  };
}
