import { useEffect, useMemo, type ReactNode } from "react";
import type {
  ClientLsData,
  CommandsData,
  ProfileLsData,
  ServerLsData,
} from "../bridge/data";
import type { ClientDirectLsData } from "../types/client";
import { useCtlQuery } from "../ui";
import { buildClientViews, buildServerViews, type StatusDoc } from "./model";
import { POLL_MS, ServersContext, type ServersCtx, type TabId } from "./useServers";

/** The reads the Servers screen is built from. The gateway state is read again every few
 * seconds while the page is visible; the lists are read again after every write. */
export function ServersProvider({
  children,
  go,
  pollMs = POLL_MS,
}: {
  children: ReactNode;
  go: (tab: TabId) => void;
  pollMs?: number;
}) {
  const status = useCtlQuery<StatusDoc>(["status"]);
  const servers = useCtlQuery<ServerLsData>(["server", "ls"]);
  const profiles = useCtlQuery<ProfileLsData>(["profile", "ls"]);
  const clients = useCtlQuery<ClientLsData>(["client", "ls"]);
  const direct = useCtlQuery<ClientDirectLsData>(["client", "direct", "ls"]);
  const registry = useCtlQuery<CommandsData>(["commands"]);

  const reloadStatus = status.reload;
  useEffect(() => {
    if (pollMs <= 0) return;
    const timer = window.setInterval(() => {
      if (!document.hidden) reloadStatus();
    }, pollMs);
    return () => window.clearInterval(timer);
  }, [pollMs, reloadStatus]);

  const reloads = [
    status.reload,
    servers.reload,
    profiles.reload,
    clients.reload,
    direct.reload,
  ];
  const reload = () => reloads.forEach((run) => run());

  const statusDoc = useMemo<StatusDoc | null>(() => {
    if (!status.data) return null;
    const entries = status.data.directEntries ?? direct.data?.entries.length;
    return entries === undefined
      ? status.data
      : { ...status.data, directEntries: entries };
  }, [status.data, direct.data]);

  const views = useMemo(
    () =>
      servers.data && profiles.data && statusDoc
        ? buildServerViews(servers.data, profiles.data, statusDoc)
        : null,
    [servers.data, profiles.data, statusDoc],
  );
  const clientViews = useMemo(
    () =>
      clients.data && profiles.data && views
        ? buildClientViews(clients.data.clients, profiles.data, views)
        : null,
    [clients.data, profiles.data, views],
  );

  const value: ServersCtx = {
    status,
    servers,
    profiles,
    clients,
    direct,
    registry,
    rows: registry.data?.commands ?? null,
    statusDoc,
    views,
    clientViews,
    reload,
    go,
  };
  return <ServersContext.Provider value={value}>{children}</ServersContext.Provider>;
}
