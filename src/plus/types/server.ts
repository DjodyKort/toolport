import {
  any,
  arr,
  nullable,
  num,
  obj,
  opt,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";
import { serverInfoData, serverLsData } from "../bridge/data";

/** `data` of the server commands, checked against the golden envelopes by `data.test.ts`. */

export const serverEditData = obj({
  changed: arr(str),
  id: str,
});
export type ServerEditData = Infer<typeof serverEditData>;

export const serverInstallData = obj({
  envKeys: arr(any),
  id: str,
  name: str,
  source: str,
});
export type ServerInstallData = Infer<typeof serverInstallData>;

export const serverNewData = obj({
  id: str,
  name: str,
});
export type ServerNewData = Infer<typeof serverNewData>;

export const serverSearchData = obj({
  query: str,
  results: arr(
    obj({
      args: arr(str),
      category: str,
      command: nullable(str),
      description: str,
      envKeys: arr(str),
      name: str,
      source: str,
      transport: str,
      url: nullable(str),
    }),
  ),
  total: num,
  // Present only when the live MCP Registry call failed; curated/cached
  // `results` are reported regardless (D-101).
  registryError: opt(obj({ kind: str, message: str })),
});
export type ServerSearchData = Infer<typeof serverSearchData>;

/** Golden file stem to the shape of its envelope `data`. */
export const serverShapes: Record<string, Shape<unknown>> = {
  "server-edit.after": serverInfoData,
  "server-edit.apply": serverEditData,
  "server-install.apply": serverInstallData,
  "server-new.after": serverLsData,
  "server-new.apply": serverNewData,
  "server-new.remote": serverNewData,
  "server-search": serverSearchData,
};
