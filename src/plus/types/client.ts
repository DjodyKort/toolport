import {
  any,
  arr,
  bool,
  nullable,
  obj,
  opt,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** `data` of the client commands, checked against the golden envelopes by `data.test.ts`. */

export const clientDirectLsData = obj({
  entries: arr(
    obj({
      client: str,
      clientName: str,
      entry: str,
      path: str,
      server: str,
      serverName: str,
      state: str,
      /** The address of a remote entry; a launcher entry has none. */
      url: opt(str),
    }),
  ),
});
export type ClientDirectLsData = Infer<typeof clientDirectLsData>;

export const clientDirectAddData = obj({
  action: str,
  backup: nullable(str),
  changed: bool,
  client: str,
  clientName: str,
  dryRun: bool,
  entry: str,
  /** A stdio server: the command the client starts. */
  launcher: opt(
    obj({
      args: arr(str),
      command: str,
      env: obj({
        TOOLPORT_DATA_DIR: str,
      }),
    }),
  ),
  /** A remote server: the address the client connects to and signs in to by itself. */
  remote: opt(obj({ url: str })),
  notes: arr(str),
  path: str,
  server: str,
  serverName: str,
  tradeoff: str,
});
export type ClientDirectAddData = Infer<typeof clientDirectAddData>;

export const clientDirectRmData = obj({
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
export type ClientDirectRmData = Infer<typeof clientDirectRmData>;

export const clientEditData = obj({
  backup: nullable(str),
  changed: bool,
  client: str,
  dryRun: bool,
  followsActive: nullable(any),
  name: str,
  notInClient: arr(any),
  path: str,
  profiles: obj({
    added: arr(str),
    after: arr(str),
    before: arr(any),
    removed: arr(any),
  }),
  scope: obj({
    after: str,
    before: nullable(any),
  }),
});
export type ClientEditData = Infer<typeof clientEditData>;

export const clientImportData = obj({
  client: str,
  direct: arr(
    obj({
      name: str,
      status: str,
      target: str,
      transport: str,
    }),
  ),
  dryRun: bool,
  gateway: arr(any),
  imported: arr(
    obj({
      id: str,
      name: str,
    }),
  ),
  name: str,
  path: str,
  profile: nullable(
    obj({
      created: bool,
      id: str,
      name: str,
      servers: arr(str),
    }),
  ),
  secrets: arr(any),
  selected: bool,
  skipped: arr(any),
});
export type ClientImportData = Infer<typeof clientImportData>;

export const clientReachData = obj({
  clients: arr(obj({ gateway: str, id: str, name: str, profile: nullable(str) })),
  findings: arr(
    obj({
      client: nullable(str),
      kind: str,
      level: str,
      server: nullable(str),
      text: str,
    }),
  ),
  servers: arr(
    obj({
      id: str,
      name: str,
      profiles: arr(str),
      routes: arr(
        obj({
          basis: nullable(str),
          client: str,
          clientName: str,
          entry: nullable(str),
          held: nullable(
            obj({ disabledIn: arr(str), enabled: bool, live: nullable(str) }),
          ),
          how: str,
          note: nullable(str),
          recorded: bool,
          state: nullable(str),
          target: nullable(str),
          transport: nullable(str),
        }),
      ),
      transport: str,
    }),
  ),
  unregistered: arr(
    obj({
      client: str,
      clientName: str,
      entry: str,
      target: nullable(str),
      transport: str,
    }),
  ),
});
export type ClientReachData = Infer<typeof clientReachData>;

/** Golden file stem to the shape of its envelope `data`. */
export const clientShapes: Record<string, Shape<unknown>> = {
  "client-direct-add.after": clientDirectLsData,
  "client-direct-ls": clientDirectLsData,
  "client-reach": clientReachData,
  "client-direct-add.apply": clientDirectAddData,
  "client-direct-add.preview": clientDirectAddData,
  "client-direct-rm.apply": clientDirectRmData,
  "client-direct-rm.preview": clientDirectRmData,
  "client-edit.apply": clientEditData,
  "client-edit.preview": clientEditData,
  "client-import.apply": clientImportData,
  "client-import.preview": clientImportData,
};
