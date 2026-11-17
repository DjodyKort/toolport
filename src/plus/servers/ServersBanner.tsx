import { Fragment, useEffect, useState } from "react";
import { ChevronRight, PackagePlus, RotateCw, ShieldAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import type { Registry, ServerEntry } from "@/lib/types";
import { checkLogin, listServers, loadLogins } from "./commands";
import { clock } from "./format";
import { loginFailed } from "./loginModel";
import { restart, useFailedRestarts } from "./restartStore";
import type { PlusHost } from "./ServerOperations";
import { plural } from "../settings/model";
import { useLoad } from "./useLoad";
import { observeServers, useServerWatch } from "./watch";

function NameLinks({
  ids,
  servers,
  host,
}: {
  ids: string[];
  servers: ServerEntry[];
  host: PlusHost;
}) {
  return (
    <>
      {ids.map((id, i) => (
        <Fragment key={id}>
          {i > 0 && ", "}
          <button
            type="button"
            className="font-medium text-primary underline-offset-2 hover:underline focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none"
            onClick={() => host.openServer(id)}
          >
            <span className="sr-only">Open </span>
            {servers.find((s) => s.id === id)?.name ?? id}
          </button>
        </Fragment>
      ))}
    </>
  );
}

export interface ServersBannerProps {
  registry: Registry | null;
  host: PlusHost;
}

/** Fork rows under upstream's sign-in or status card: a new server that is not set up yet, a
 * restart that failed in a gateway and a login check that came back refused. Each row names its
 * servers, and a name opens that server. */
export default function ServersBanner({ registry, host }: ServersBannerProps) {
  const watch = useServerWatch();
  const failures = useFailedRestarts();
  const servers = registry?.servers ?? [];
  const logins = useLoad(loadLogins, registry);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    if (registry) observeServers(registry.servers);
  }, [registry]);

  const fresh = watch.open.filter((id) => servers.some((s) => s.id === id));
  const failed = failures.filter((f) => servers.some((s) => s.id === f.id));
  const refused = (logins.data?.servers ?? []).filter(
    (row) =>
      loginFailed(row) &&
      servers.some((s) => s.id === row.server && s.transport === "stdio"),
  );
  if (fresh.length === 0 && failed.length === 0 && refused.length === 0) return null;

  async function checkAgain() {
    setChecking(true);
    await Promise.allSettled(refused.map((row) => checkLogin(row.server)));
    logins.reload();
    setChecking(false);
  }

  return (
    <div className="-mt-3 flex flex-col gap-2" data-plus="servers-banner">
      {fresh.length > 0 && (
        <StatusCard
          icon={<PackagePlus />}
          title={
            fresh.length === 1
              ? "1 new server has setup left"
              : `${fresh.length} new servers have setup left`
          }
          detail={
            <>
              <NameLinks ids={fresh} servers={servers} host={host} /> was added since you
              last looked. Open it to see what is missing.
            </>
          }
        />
      )}
      {failed.length > 0 && (
        <StatusCard
          variant="warning"
          icon={<RotateCw />}
          title={
            failed.length === 1
              ? "A restart failed in some gateways"
              : `${failed.length} restarts failed in some gateways`
          }
          detail={
            <>
              <NameLinks ids={failed.map((f) => f.id)} servers={servers} host={host} />
              {failed.length === 1 && ` · ${clock(failed[0].result.at)}`}. The gateways
              that did restart keep their new process.
            </>
          }
          actions={
            <Button
              variant="outline"
              size="sm"
              onClick={() =>
                failed.forEach(
                  ({ id }) =>
                    void restart(id, servers.find((s) => s.id === id)?.name ?? id),
                )
              }
            >
              <RotateCw /> Retry
            </Button>
          }
        />
      )}
      {refused.length > 0 && (
        <StatusCard
          variant="warning"
          icon={<ShieldAlert />}
          title={
            refused.length === 1
              ? "1 login check failed"
              : `${refused.length} login checks failed`
          }
          detail={
            <>
              <NameLinks
                ids={refused.map((r) => r.server)}
                servers={servers}
                host={host}
              />
              {refused.length === 1 && refused[0].lastProbe !== null
                ? ` · ${clock(refused[0].lastProbe * 1000)}`
                : ""}
              . The server still starts, but calls that need the login fail. Open it to
              sign in again.
            </>
          }
          actions={
            <Button
              variant="outline"
              size="sm"
              disabled={checking}
              onClick={() => void checkAgain()}
            >
              <RotateCw /> Check again
            </Button>
          }
        />
      )}
    </div>
  );
}

/** The quiet line under the list: how many servers are on and who sees them by default. */
export function ServersListEnd({ registry, host }: ServersBannerProps) {
  const list = useLoad(listServers, registry);
  if (!list.data) return null;
  const on = list.data.servers.filter((s) => s.enabled).length;
  const off = list.data.servers.length - on;
  const defaultSet = list.data.defaultAccess
    ? (registry?.profiles.find((p) => p.id === list.data!.defaultAccess)?.name ??
      list.data.defaultAccess)
    : null;
  return (
    <p
      data-plus="servers-list-end"
      className="flex flex-wrap items-center gap-x-2 text-meta text-muted-foreground"
    >
      <span>
        {plural(on, "server")} on, {off} off
      </span>
      <span aria-hidden="true">·</span>
      <span>
        Clients without their own access set see{" "}
        {defaultSet ? `the set ${defaultSet}` : "every enabled server"}
      </span>
      <Button variant="ghost" size="xs" onClick={host.openAccessSets}>
        Compare access sets <ChevronRight />
      </Button>
    </p>
  );
}
