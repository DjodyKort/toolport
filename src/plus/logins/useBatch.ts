import { useCallback, useEffect, useRef, useState } from "react";
import type { Settled } from "./model";

export interface Batch<V> {
  results: Record<string, Settled<V>>;
  /** Keys that have no answer yet. */
  pending: number;
  /** Reads the given keys again (all of them without an argument) and keeps the rest. */
  refresh: (keys?: string[]) => Promise<void>;
}

async function settle<V>(load: () => Promise<V>): Promise<Settled<V>> {
  try {
    return { ok: true, value: await load() };
  } catch (error) {
    return { ok: false, error };
  }
}

/** Reads one answer per key, a few at a time, so a long list does not start a child process
 * for every row at once. A failure is kept as that key's answer. */
export function useBatch<V>(
  keys: readonly string[],
  load: (key: string) => Promise<V>,
  concurrency = 4,
): Batch<V> {
  const [results, setResults] = useState<Record<string, Settled<V>>>({});
  const loadRef = useRef(load);
  const alive = useRef(true);
  useEffect(() => {
    loadRef.current = load;
  });
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const run = useCallback(
    async (todo: string[]) => {
      const queue = [...todo];
      const worker = async () => {
        for (let key = queue.shift(); key !== undefined; key = queue.shift()) {
          const answer = await settle(() => loadRef.current(key));
          if (!alive.current) return;
          setResults((prev) => ({ ...prev, [key]: answer }));
        }
      };
      await Promise.all(
        Array.from({ length: Math.min(concurrency, queue.length) }, worker),
      );
    },
    [concurrency],
  );

  const signature = keys.join("\u0001");
  useEffect(() => {
    const wanted = signature === "" ? [] : signature.split("\u0001");
    setResults((prev) => {
      const next: Record<string, Settled<V>> = {};
      for (const key of wanted) if (key in prev) next[key] = prev[key];
      return Object.keys(next).length === Object.keys(prev).length ? prev : next;
    });
    void run(wanted);
  }, [signature, run]);

  const refresh = useCallback(
    (only?: string[]) => run(only ?? (signature === "" ? [] : signature.split("\u0001"))),
    [run, signature],
  );

  const pending = keys.filter((key) => !(key in results)).length;
  return { results, pending, refresh };
}
