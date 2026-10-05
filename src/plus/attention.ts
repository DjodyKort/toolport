import { useEffect, useState } from "react";
import { ctlData } from "./bridge/ctl";
import type { CommandsData } from "./bridge/data";

const POLL_MS = 60_000;

const listeners = new Set<() => void>();

/** Reads the number again now; the Attention screen calls it after a row is hidden or an
 * action ran, so the sidebar does not wait for the next minute. */
export function refreshAttentionCount() {
  listeners.forEach((listener) => listener());
}

let probe: Promise<boolean> | null = null;

/** Whether this build of the CLI has an `attention ls` row. Asked once: the registry is large
 * and only changes with the binary. */
function hasAttentionRow(): Promise<boolean> {
  probe ??= ctlData<CommandsData>(["commands"])
    .then((registry) => registry.commands.some((row) => row.id === "attention ls"))
    .catch((error) => {
      probe = null;
      throw error;
    });
  return probe;
}

export function forgetAttentionProbe() {
  probe = null;
}

/** The number on the sidebar's Attention item: what needs you (`counts.needsYou`, contract
 * section 9). `null` while the CLI has no `attention ls` row; MIG-GUI-11 adds it. This is the
 * one function that decides where the number comes from. */
export async function readAttentionCount(): Promise<number | null> {
  if (!(await hasAttentionRow())) return null;
  const data = await ctlData<{ counts: { needsYou: number } }>([
    "attention",
    "ls",
    "--level",
    "needs-you",
  ]);
  return data.counts.needsYou;
}

/** The count, refreshed every minute. A failed read keeps the last number: the badge never
 * turns into a claim that everything is fine. */
export function useAttentionCount(
  read: () => Promise<number | null> = readAttentionCount,
) {
  const [count, setCount] = useState<number | null>(null);
  useEffect(() => {
    let alive = true;
    const load = () =>
      read().then(
        (next) => alive && setCount(next),
        () => {},
      );
    void load();
    const id = setInterval(() => void load(), POLL_MS);
    listeners.add(load);
    return () => {
      alive = false;
      listeners.delete(load);
      clearInterval(id);
    };
  }, [read]);
  return count;
}
