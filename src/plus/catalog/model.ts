import type { CatalogIndexStatus } from "../api";
import { ageText } from "../attention/model";

export function indexSummary(status: CatalogIndexStatus, now = Date.now()): string {
  const servers = `${status.count.toLocaleString()} server${status.count === 1 ? "" : "s"}`;
  if (!status.lastSyncedAt) {
    return status.count > 0
      ? `Index: ${servers}, not fully synced yet`
      : "Index: not built yet";
  }
  const age = ageText(status.lastSyncedAt, now);
  return `Index: ${servers}, synced ${age === "just now" ? age : `${age} ago`}`;
}
