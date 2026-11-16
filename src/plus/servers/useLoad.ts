import { useCallback, useEffect, useEffectEvent, useState } from "react";
import { errorText } from "../settings/useCtl";

export interface Loaded<T> {
  data: T | null;
  error: string | null;
  reload: () => void;
}

/** Loads once and again when `key` changes or `reload` is called. A slower, older answer never
 * replaces a newer one, and the last data stays on screen while the next load runs. */
export function useLoad<T>(load: () => Promise<T>, key: unknown): Loaded<T> {
  const [state, setState] = useState<{ data: T | null; error: string | null }>({
    data: null,
    error: null,
  });
  const [tick, setTick] = useState(0);
  const run = useEffectEvent(load);

  useEffect(() => {
    let alive = true;
    run().then(
      (data) => alive && setState({ data, error: null }),
      (error: unknown) =>
        alive && setState((current) => ({ ...current, error: errorText(error) })),
    );
    return () => {
      alive = false;
    };
  }, [key, tick]);

  const reload = useCallback(() => setTick((value) => value + 1), []);
  return { ...state, reload };
}
