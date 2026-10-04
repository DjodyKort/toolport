import { createContext, useContext } from "react";
import type {
  ClientLsData,
  CommandRow,
  CommandsData,
  ProfileLsData,
  ServerLsData,
} from "../bridge/data";
import type { ClientDirectLsData } from "../types/client";
import type { CtlQuery } from "../ui";
import type { ClientView, ServerView, StatusDoc } from "./model";

export const POLL_MS = 5000;

export type TabId =
  "servers" | "profiles" | "clients" | "logins" | "secrets" | "integrations" | "health";

export interface ServersCtx {
  status: CtlQuery<StatusDoc>;
  servers: CtlQuery<ServerLsData>;
  profiles: CtlQuery<ProfileLsData>;
  clients: CtlQuery<ClientLsData>;
  direct: CtlQuery<ClientDirectLsData>;
  registry: CtlQuery<CommandsData>;
  /** The registry rows the policy of each write is read from. */
  rows: CommandRow[] | null;
  statusDoc: StatusDoc | null;
  views: ServerView[] | null;
  clientViews: ClientView[] | null;
  /** Reads everything again, e.g. after a write. */
  reload: () => void;
  go: (tab: TabId) => void;
}

export const ServersContext = createContext<ServersCtx | null>(null);

export function useServers(): ServersCtx {
  const value = useContext(ServersContext);
  if (!value) throw new Error("useServers needs a ServersProvider");
  return value;
}
