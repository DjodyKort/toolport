import { useEffect, useState } from "react";
import { getRegistry } from "@/lib/api";
import {
  isEnabled,
  type ProbeResult,
  type Registry,
  type ServerEntry,
} from "@/lib/types";
import { SectionHeader } from "@/components/ui/section-header";
import { Badge } from "@/components/ui/badge";
import { AdvancedRow } from "./AdvancedRow";
import { listAccessSets, loadLogins, serverInfo } from "./commands";
import { FinishSetup } from "./FinishSetup";
import { useMembershipSync, type MembershipChange } from "./membership";
import { LoginRow } from "./LoginRow";
import RepoRow from "./RepoRow";
import { loginNeedsYou, loginRowOf } from "./loginModel";
import { leftOutOfDefault } from "./model";
import { ReachRow } from "./ReachRow";
import { RemoveRow } from "./RemoveRow";
import { useRestartState } from "./restartStore";
import { RuntimeRow } from "./RuntimeRow";
import { SecretsRow } from "./SecretsRow";
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
  const logins = useLoad(loadLogins, key);
  const [secretsVersion, setSecretsVersion] = useState(0);
  const restart = useRestartState(server.id);
  const syncRegistry = useMembershipSync(registry, onRegistryChange);

  useEffect(() => {
    if (registry) observeServers(registry.servers);
  }, [registry]);

  function refresh(change: MembershipChange) {
    syncRegistry(change);
    info.reload();
    accessSets.reload();
  }

  async function secretsChanged() {
    setSecretsVersion((value) => value + 1);
    logins.reload();
    info.reload();
    onRegistryChange(await getRegistry());
  }

  const login =
    server.transport === "stdio" ? loginRowOf(logins.data, server.id) : undefined;
  const needsYou =
    (restart.last?.status === "failed" ? 1 : 0) +
    (login && loginNeedsYou(login) ? 1 : 0) +
    (leftOutOfDefault(enabled, accessSets.data, info.data) ? 1 : 0);

  return (
    <div className="mt-6 flex flex-col gap-6">
      {watch.open.includes(server.id) && (
        <FinishSetup
          server={server}
          enabled={enabled}
          health={health}
          registry={registry}
          refreshKey={secretsVersion}
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
          {login && (
            <LoginRow
              key={server.id}
              serverId={server.id}
              serverName={server.name}
              row={login}
              onChanged={logins.reload}
            />
          )}
          {server.transport === "stdio" && (
            <RepoRow key={`repo-${server.id}`} serverId={server.id} />
          )}
          <ReachRow
            server={server}
            enabled={enabled}
            registry={registry}
            info={info}
            sets={accessSets}
            onChanged={refresh}
            onOpenAccessSets={host.openAccessSets}
          />
          <SecretsRow
            key={server.id}
            server={server}
            serverId={server.id}
            serverName={server.name}
            info={info}
            plain={server.env.filter((e) => !e.secret)}
            login={login}
            onSaved={onRegistryChange}
            onChanged={secretsChanged}
          />
          <AdvancedRow
            key={`advanced-${server.id}`}
            server={server}
            enabled={enabled}
            health={health}
            info={info}
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
