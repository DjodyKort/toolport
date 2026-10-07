import { useCallback, useEffect, useRef, useState } from "react";
import { Loader2, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { plusCatalogIndexStatus, plusCatalogIndexSync } from "../api";
import type { CatalogIndexStatus } from "../api";
import { indexSummary } from "./model";

const BUSY_POLL_MS = 1500;
const IDLE_POLL_MS = 10_000;

export function CatalogIndexLine({ onSynced }: { onSynced?: () => void }) {
  const [status, setStatus] = useState<CatalogIndexStatus | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  const wasRunning = useRef(false);
  const synced = useRef(onSynced);
  useEffect(() => {
    synced.current = onSynced;
  });

  const load = useCallback(
    () =>
      plusCatalogIndexStatus()
        .then((next) => {
          if (alive.current) setStatus(next);
        })
        .catch(() => {}),
    [],
  );

  useEffect(() => {
    alive.current = true;
    void load();
    return () => {
      alive.current = false;
    };
  }, [load]);

  const running = status?.running === true;
  const busy = refreshing || running;

  const readable = status !== null;
  useEffect(() => {
    if (!readable) return;
    const timer = setInterval(() => void load(), busy ? BUSY_POLL_MS : IDLE_POLL_MS);
    return () => clearInterval(timer);
  }, [readable, busy, load]);

  useEffect(() => {
    if (wasRunning.current && !running && !refreshing) synced.current?.();
    wasRunning.current = running;
  }, [running, refreshing]);

  async function refresh() {
    setRefreshing(true);
    setError(null);
    try {
      await plusCatalogIndexSync(false);
      synced.current?.();
    } catch (e) {
      if (alive.current) setError(e instanceof Error ? e.message : String(e));
    } finally {
      await load();
      if (alive.current) setRefreshing(false);
    }
  }

  // The live search still works without the line, so an unreadable index just hides it.
  if (!status) return null;
  return (
    <div
      aria-label="Registry index"
      className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground"
    >
      <span>{indexSummary(status)}</span>
      <Button
        variant="ghost"
        size="sm"
        className="h-6 gap-1 px-2 text-xs"
        disabled={busy}
        onClick={refresh}
      >
        <RefreshCw className="size-3" aria-hidden="true" />
        Refresh
      </Button>
      {busy && (
        <span role="status" aria-live="polite" className="flex items-center gap-1.5">
          <Loader2 className="size-3 shrink-0 animate-spin" aria-hidden="true" />
          Syncing the MCP Registry index: {status.count.toLocaleString()} servers so far
        </span>
      )}
      {error && (
        <span role="alert" className="text-warning">
          {error}
        </span>
      )}
    </div>
  );
}
