import { useCallback, useEffect, useState } from "react";
import { Database, RotateCw } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { toastError } from "@/lib/toast";
import { ctlData } from "../bridge/ctl";
import {
  INDEX_BADGE,
  indexNote,
  indexSummary,
  type RegistryIndexData,
  type RegistryIndexSyncData,
} from "./registryIndex";
import { errorText } from "./useCtl";

const POLL_MS = 4000;

/** The local index of the MCP Registry (D-118): how many servers it holds, when it was synced,
 * whether catalog search can answer from it, and a Refresh now button. */
export function RegistryIndexCard() {
  const [data, setData] = useState<RegistryIndexData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(timer);
  }, []);

  const load = useCallback(async () => {
    const result = await ctlData<RegistryIndexData>(["catalog", "status"]).then(
      (status) => ({ status, failure: null }),
      (failure: unknown) => ({ status: null, failure: errorText(failure) }),
    );
    setNow(Date.now());
    if (result.status) setData(result.status);
    setError(result.failure);
  }, []);

  useEffect(() => {
    let alive = true;
    void ctlData<RegistryIndexData>(["catalog", "status"]).then(
      (status) => alive && setData(status),
      (failure: unknown) => alive && setError(errorText(failure)),
    );
    return () => {
      alive = false;
    };
  }, []);

  const running = refreshing || data?.state === "syncing";
  useEffect(() => {
    if (!running) return;
    const timer = setInterval(() => void load(), POLL_MS);
    return () => clearInterval(timer);
  }, [running, load]);

  async function refresh() {
    setRefreshing(true);
    try {
      const done = await ctlData<RegistryIndexSyncData>(["catalog", "sync"]);
      setData(done.status);
      setNow(Date.now());
      toast.success(
        `Registry index synced: ${done.report.count.toLocaleString("en-US")} servers`,
      );
    } catch (failure) {
      toastError("Could not sync the registry index", {
        description: errorText(failure),
      });
      await load();
    } finally {
      setRefreshing(false);
    }
  }

  const shown = data && refreshing ? { ...data, state: "syncing" as const } : data;
  const badge = shown ? INDEX_BADGE[shown.state] : null;
  const note = shown ? indexNote(shown) : null;

  return (
    <div className="toolport-list">
      <div className="flex items-center gap-3 px-4 py-3">
        <div
          aria-hidden="true"
          className="grid size-8 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground [&_svg]:size-4"
        >
          <Database />
        </div>
        <div className="min-w-0 flex-1">
          <p className="flex items-center gap-2 text-sm font-semibold">
            Registry index
            {badge && <Badge variant={badge.variant}>{badge.text}</Badge>}
          </p>
          <p className="text-meta text-muted-foreground">
            {shown ? indexSummary(shown, now) : (error ?? "Reading the registry index…")}
          </p>
          {note && <p className="text-meta text-muted-foreground">{note}</p>}
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void refresh()}
          disabled={running}
        >
          <RotateCw className={running ? "animate-spin" : undefined} /> Refresh now
        </Button>
      </div>
    </div>
  );
}
