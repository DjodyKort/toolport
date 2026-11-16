import { useEffect } from "react";
import { getRegistry } from "@/lib/api";
import {
  isEnabled,
  type ProbeResult,
  type Registry,
  type ServerEntry,
} from "@/lib/types";
import { SectionHeader } from "@/components/ui/section-header";
import { Badge } from "@/components/ui/badge";
import { listAccessSets, serverInfo } from "./commands";
import { FinishSetup } from "./FinishSetup";
import { leftOutOfDefault } from "./model";
import { ReachRow } from "./ReachRow";
import { RemoveRow } from "./RemoveRow";
import { useRestartState } from "./restartStore";
import { RuntimeRow } from "./RuntimeRow";
import { useLoad } from "./useLoad";
import { fingerprint, observeServers, useServerWatch } from "./watch";

export interface PlusHost {
  openServer: (id: string) => void;
  openAccessSets: () => void;
  closeServer: () => void;
}

export interface ServerOperationsProps {
  server: ServerEntry;
  registry: Registry | null;
  health?: ProbeResult;
  host: PlusHost;
  onRegistryChange: (registry: Registry) => void;
}

/** What the fork knows about one server, under upstream's Connection card: the few things a new
 * server still needs, then one list card of rows named after the question they answer. */
export default function ServerOperations({
  server,
  registry,
  health,
  host,
  onRegistryChange,
}: ServerOperationsProps) {
  const watch = useServerWatch();
  const enabled = registry ? isEnabled(registry, server.id) : server.enabled !== false;
  const sets = (registry?.profiles ?? [])
    .filter((p) => p.enabledServerIds.includes(server.id))
    .map((p) => p.id)
    .join();
  const key = `${fingerprint(server)}|${enabled}|${sets}|${registry?.defaultAccessContextId ?? ""}`;
  const info = useLoad(() => serverInfo(server.id), key);
  const accessSets = useLoad(listAccessSets, key);
  const restart = useRestartState(server.id);

  useEffect(() => {
    if (registry) observeServers(registry.servers);
  }, [registry]);

  async function refresh() {
    onRegistryChange(await getRegistry());
    info.reload();
    accessSets.reload();
  }

  const needsYou =
    (restart.last?.status === "failed" ? 1 : 0) +
    (leftOutOfDefault(enabled, accessSets.data, info.data) ? 1 : 0);

  return (
    <div className="mt-6 flex flex-col gap-6">
      {watch.open.includes(server.id) && (
        <FinishSetup
          server={server}
          enabled={enabled}
          health={health}
          registry={registry}
          sets={accessSets}
          onRegistryChange={onRegistryChange}
          onMembershipChanged={refresh}
        />
      )}
      <section aria-label="Operations">
        <SectionHeader>
          Operations{" "}
          {needsYou > 0 && <Badge variant="warning">{needsYou} need you</Badge>}
        </SectionHeader>
        <div className="toolport-list">
          <RuntimeRow
            server={server}
            enabled={enabled}
            health={health}
            info={info}
            changedAt={watch.changed[server.id]}
          />
          <ReachRow
            server={server}
            enabled={enabled}
            registry={registry}
            info={info}
            sets={accessSets}
            onChanged={refresh}
            onOpenAccessSets={host.openAccessSets}
          />
          <RemoveRow
            server={server}
            info={info.data}
            onRemoved={async () => {
              onRegistryChange(await getRegistry());
              host.closeServer();
            }}
          />
        </div>
      </section>
    </div>
  );
}
