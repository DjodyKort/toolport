import { useEffect, useMemo, type ReactNode } from "react";
import type {
  ClientLsData,
  CommandsData,
  ProfileLsData,
  ServerLsData,
} from "../bridge/data";
import type { ClientDirectLsData, ClientReachData } from "../types/client";
import { useCtlQuery, type CtlQuery } from "../ui";
import { buildClientViews, buildServerViews, type StatusDoc } from "./model";
import { POLL_MS, ServersContext, type ServersCtx, type TabId } from "./useServers";

const routeKey = (server: string, client: string, entry: string | null) =>
  `${server}\u0000${client}\u0000${entry ?? ""}`;

/** What the slow check of Claude Code found, laid over the quick reading of the configs. The
 * quick reading is the one a write reloads, so the routes stay current while the check lags. */
function withLive(
  config: ClientReachData | null,
  live: ClientReachData | null,
): ClientReachData | null {
  if (!config) return null;
  if (!live) return config;
  const states = new Map<string, string | null>();
  for (const server of live.servers)
    for (const route of server.routes)
      if (route.held)
        states.set(routeKey(server.id, route.client, route.entry), route.held.live);
  return {
    ...config,
    findings: [
      ...config.findings,
      ...live.findings.filter((finding) => finding.kind === "live-unavailable"),
    ],
    servers: config.servers.map((server) => ({
      ...server,
      routes: server.routes.map((route) =>
        route.held
          ? {
              ...route,
              held: {
                ...route.held,
                live: states.get(routeKey(server.id, route.client, route.entry)) ?? null,
              },
            }
          : route,
      ),
    })),
  };
}

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
  const reachConfig = useCtlQuery<ClientReachData>(["client", "reach"]);
  const reachLive = useCtlQuery<ClientReachData>(["client", "reach", "--live"]);
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
    reachConfig.reload,
  ];
  const reload = () => reloads.forEach((run) => run());

  const reach = useMemo<CtlQuery<ClientReachData>>(
    () => ({ ...reachConfig, data: withLive(reachConfig.data, reachLive.data) }),
    [reachConfig, reachLive.data],
  );
  const liveCheck = useMemo(
    () => ({ running: reachLive.status === "loading", run: reachLive.reload }),
    [reachLive.status, reachLive.reload],
  );

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
        ? buildServerViews(servers.data, profiles.data, statusDoc, reach.data ?? null)
        : null,
    [servers.data, profiles.data, statusDoc, reach.data],
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
    reach,
    liveCheck,
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
