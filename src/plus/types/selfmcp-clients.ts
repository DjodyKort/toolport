import {
  any,
  arr,
  bool,
  nullable,
  obj,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** Results of the clients self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

export const clientDirectAddResult = obj({
  action: str,
  backup: nullable(str),
  changed: bool,
  client: str,
  clientName: str,
  dryRun: bool,
  entry: str,
  launcher: obj({
    args: arr(str),
    command: str,
    env: obj({
      TOOLPORT_DATA_DIR: str,
    }),
  }),
  notes: arr(str),
  path: str,
  server: str,
  serverName: str,
  tradeoff: str,
});
export type ClientDirectAddResult = Infer<typeof clientDirectAddResult>;

export const clientDirectLsResult = obj({
  entries: arr(
    obj({
      client: str,
      clientName: str,
      entry: str,
      path: str,
      server: str,
      serverName: str,
      state: str,
    }),
  ),
});
export type ClientDirectLsResult = Infer<typeof clientDirectLsResult>;

export const clientDirectRmResult = obj({
  action: str,
  backup: nullable(str),
  changed: bool,
  client: str,
  clientName: str,
  dryRun: bool,
  entry: str,
  notes: arr(any),
  path: str,
  server: str,
  serverName: str,
});
export type ClientDirectRmResult = Infer<typeof clientDirectRmResult>;

export const clientsListResult = obj({
  clients: arr(
    obj({
      appPresent: bool,
      gatewayInstalled: bool,
      id: str,
      name: str,
    }),
  ),
});
export type ClientsListResult = Infer<typeof clientsListResult>;

export const clientsSyncResult = obj({
  clients: arr(any),
  dryRun: bool,
});
export type ClientsSyncResult = Infer<typeof clientsSyncResult>;

/** Tool name to the shape of its `structuredContent`. */
export const clientsToolShapes: Record<string, Shape<unknown>> = {
  client_direct_add: clientDirectAddResult,
  client_direct_ls: clientDirectLsResult,
  client_direct_rm: clientDirectRmResult,
  clients_list: clientsListResult,
  clients_sync: clientsSyncResult,
};
