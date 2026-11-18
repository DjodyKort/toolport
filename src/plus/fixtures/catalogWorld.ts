import type { CtlReply } from "./ctlWorld";
import type { RegistryIndexData } from "../settings/registryIndex";

export interface CatalogWorldOptions {
  /** How the index starts: synced a few hours ago, a day or more ago, never, or after a failed sync. */
  index?: "fresh" | "stale" | "never" | "error";
  /** `catalog sync` answers with an error. */
  syncFails?: boolean;
}

export const INDEXED_SERVERS = 13712;
const HOUR_MS = 3_600_000;
const FAILURE = "the MCP Registry did not answer: connection failed: down";

function isoHoursAgo(hours: number): string {
  return new Date(Date.now() - hours * HOUR_MS).toISOString().replace(/\.\d+Z$/, "Z");
}

function seed(kind: NonNullable<CatalogWorldOptions["index"]>): RegistryIndexData {
  const base: RegistryIndexData = {
    count: INDEXED_SERVERS,
    lastSyncedAt: isoHoursAgo(3),
    lastAttemptAt: isoHoursAgo(3),
    state: "fresh",
    resumable: false,
    error: null,
    background: true,
  };
  if (kind === "stale") return { ...base, lastSyncedAt: isoHoursAgo(30), state: "stale" };
  if (kind === "never")
    return { ...base, count: 0, lastSyncedAt: null, lastAttemptAt: null, state: "stale" };
  if (kind === "error")
    return {
      ...base,
      lastSyncedAt: isoHoursAgo(30),
      state: "error",
      error: { kind: "registry", message: FAILURE },
    };
  return base;
}

/** The `catalog` commands of an in-memory toolportctl: a sync leaves a fresh index. */
export function createCatalogWorld(options: CatalogWorldOptions = {}) {
  let status = seed(options.index ?? "fresh");
  return {
    get status() {
      return status;
    },
    run(key: string[]): CtlReply {
      if (key[1] === "status") return { ok: true, data: status };
      if (key[1] === "sync") {
        if (options.syncFails) {
          status = {
            ...status,
            state: "error",
            error: { kind: "registry", message: FAILURE },
          };
          return { ok: false, error: { code: "registry", message: FAILURE } };
        }
        status = seed("fresh");
        return {
          ok: true,
          data: {
            report: {
              kind: key.includes("--full") ? "full" : "incremental",
              pages: 138,
              fetched: INDEXED_SERVERS,
              count: INDEXED_SERVERS,
              complete: true,
            },
            status,
          },
        };
      }
      throw new Error(`Unimplemented fixture command: plus_ctl ${key.join(" ")}`);
    },
  };
}
