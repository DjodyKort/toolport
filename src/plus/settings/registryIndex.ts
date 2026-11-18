import { ago } from "../tasks/model";

export type IndexState = "fresh" | "stale" | "syncing" | "error";

/** `data` of `toolportctl catalog status`; the Rust goldens under plus/fixtures/ctl-envelopes are
 * the source. */
export interface RegistryIndexData {
  count: number;
  lastSyncedAt: string | null;
  lastAttemptAt: string | null;
  state: IndexState;
  /** An earlier sync stopped part-way; the next one continues it. */
  resumable: boolean;
  error: { kind: string; message: string } | null;
  background: boolean;
}

/** `data` of `toolportctl catalog sync`. */
export interface RegistryIndexSyncData {
  report: {
    kind: string;
    pages: number;
    fetched: number;
    count: number;
    complete: boolean;
  };
  status: RegistryIndexData;
}

export const INDEX_BADGE: Record<
  IndexState,
  { variant: "success" | "warning" | "destructive" | "info"; text: string }
> = {
  fresh: { variant: "success", text: "Up to date" },
  stale: { variant: "warning", text: "Out of date" },
  syncing: { variant: "info", text: "Syncing" },
  error: { variant: "destructive", text: "Sync failed" },
};

export function indexSummary(data: RegistryIndexData, now: number): string {
  const servers = `${data.count.toLocaleString("en-US")} ${data.count === 1 ? "server" : "servers"}`;
  const at = data.lastSyncedAt === null ? Number.NaN : Date.parse(data.lastSyncedAt);
  if (Number.isNaN(at))
    return data.count === 0 ? "Not synced yet" : `${servers}, never fully synced`;
  return `${servers}, synced ${ago(at, now)}`;
}

/** The line under the summary: why a search may still wait on the live registry. */
export function indexNote(data: RegistryIndexData): string | null {
  if (data.state === "error" && data.error) return data.error.message;
  if (data.state === "stale")
    return "Catalog searches ask the live registry until the index is synced again.";
  if (!data.background) return "Background sync is off (TOOLPORT_CATALOG_INDEX_SYNC).";
  return null;
}
