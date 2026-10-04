import { useEffect, useMemo } from "react";
import { ctlData } from "../bridge/ctl";
import type {
  CommandsData,
  ServerInfoData,
  ServerLsData,
  StatusData,
} from "../bridge/data";
import { useCtlQuery, type CtlQuery } from "../ui";
import { authRowsOf, buildRoster, type Roster } from "./model";
import { useBatch, type Batch } from "./useBatch";

export const POLL_MS = 15000;

export type Tier = "read" | "write" | "destructive";

export interface RosterData {
  status: CtlQuery<StatusData>;
  servers: CtlQuery<ServerLsData>;
  infos: Batch<ServerInfoData>;
  /** `server info` calls that failed; those servers are listed without their secrets. */
  failedInfos: number;
  /** Null until the status, the server list and every `server info` have answered. */
  roster: Roster | null;
  reload: () => void;
}

/** The reads the Logins tab is built from: the login state of every server (`status`), the
 * servers (`server ls`) and the secret keys each one declares (`server info`). The state is read
 * again every few seconds while the window is visible; `pollMs` of 0 turns that off. */
export function useRoster(pollMs: number): RosterData {
  const status = useCtlQuery<StatusData>(["status"]);
  const servers = useCtlQuery<ServerLsData>(["server", "ls"]);
  const ids = useMemo(() => servers.data?.servers.map((s) => s.id) ?? [], [servers.data]);
  const infos = useBatch<ServerInfoData>(ids, (id) =>
    ctlData<ServerInfoData>(["server", "info", id]),
  );

  const reloadStatus = status.reload;
  useEffect(() => {
    if (pollMs <= 0) return;
    const timer = window.setInterval(() => {
      if (!document.hidden) reloadStatus();
    }, pollMs);
    return () => window.clearInterval(timer);
  }, [pollMs, reloadStatus]);

  const roster = useMemo(() => {
    if (!status.data || !servers.data || infos.pending > 0) return null;
    return buildRoster(
      servers.data.servers,
      infos.results,
      authRowsOf(status.data.auth.servers),
    );
  }, [status.data, servers.data, infos.pending, infos.results]);

  const failedInfos = Object.values(infos.results).filter((answer) => !answer.ok).length;
  const reloadServers = servers.reload;
  return {
    status,
    servers,
    infos,
    failedInfos,
    roster,
    reload: () => {
      reloadStatus();
      reloadServers();
      void infos.refresh();
    },
  };
}

/** The tier of a command from `toolportctl commands`, the policy table (D-081). Until the
 * registry has answered, or when it has no such row, the safe tier is assumed. */
export function usePolicy(): (id: string, fallback: Tier) => Tier {
  const registry = useCtlQuery<CommandsData>(["commands"]);
  return (id, fallback) => {
    const row = registry.data?.commands.find((r) => r.kind === "command" && r.id === id);
    return row?.tier ?? fallback;
  };
}
