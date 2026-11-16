import { useSyncExternalStore } from "react";
import type { ServerEntry } from "@/lib/types";

const STORAGE_KEY = "plus.servers.setup.v1";

interface Persisted {
  /** Every server this window has seen, so a server that appears later is new. */
  known: string[];
  /** New servers whose setup has not been finished or dismissed. */
  open: string[];
}

interface Snapshot {
  /** Server id to when its connection settings last changed in this window. */
  changed: Record<string, number>;
  open: string[];
}

let persisted: Persisted | null = null;
let snapshot: Snapshot = { changed: {}, open: [] };
const fingerprints = new Map<string, string>();
const listeners = new Set<() => void>();

function readStore(): Persisted | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as Persisted) : null;
  } catch {
    return null;
  }
}

function writeStore(next: Persisted) {
  persisted = next;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // The setup list is a convenience; without storage it starts empty again next time.
  }
}

function publish(next: Partial<Snapshot>) {
  snapshot = { ...snapshot, ...next };
  for (const listener of listeners) listener();
}

export function fingerprint(server: ServerEntry): string {
  return JSON.stringify([
    server.transport,
    server.command,
    server.args,
    server.url,
    server.cwd ?? null,
    server.inheritEnv ?? false,
    server.env.map((e) => [e.key, e.secret, e.secret ? "" : e.value]),
  ]);
}

/** Reads the registry the app holds: notes which servers changed their connection settings, and
 * which servers are new since the last visit. A new server is only known from its absence in the
 * list this window saw before, so the first sight sets the baseline and opens nothing. */
export function observeServers(servers: ServerEntry[]) {
  if (servers.length === 0) return;
  const changed = { ...snapshot.changed };
  let touched = false;
  for (const server of servers) {
    const next = fingerprint(server);
    const before = fingerprints.get(server.id);
    if (before !== undefined && before !== next) {
      changed[server.id] = Date.now();
      touched = true;
    }
    fingerprints.set(server.id, next);
  }
  const ids = servers.map((s) => s.id);
  const before = persisted ?? readStore();
  let current: Persisted;
  if (before === null) {
    current = { known: ids, open: [] };
    writeStore(current);
  } else {
    const fresh = ids.filter((id) => !before.known.includes(id));
    current = before;
    if (fresh.length > 0) {
      current = { known: [...before.known, ...fresh], open: [...before.open, ...fresh] };
      writeStore(current);
    }
    persisted = current;
  }
  const open = current.open.filter((id) => ids.includes(id));
  if (touched || open.join() !== snapshot.open.join()) publish({ changed, open });
}

/** Closes a server's Finish setup: its facts are all true, or the person said not now. */
export function closeSetup(id: string) {
  persisted ??= readStore() ?? { known: [], open: [] };
  writeStore({ ...persisted, open: persisted.open.filter((open) => open !== id) });
  publish({ open: snapshot.open.filter((open) => open !== id) });
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useServerWatch(): Snapshot {
  return useSyncExternalStore(subscribe, () => snapshot);
}

export function resetServerWatch() {
  persisted = null;
  snapshot = { changed: {}, open: [] };
  fingerprints.clear();
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Nothing stored, nothing to clear.
  }
  for (const listener of listeners) listener();
}
