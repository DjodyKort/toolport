import {
  arr,
  bool,
  nullable,
  num,
  obj,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";
import { origin, planV1, resultV1 } from "../bridge/data";

/** `data` of `context client add|edit|rm|list` and `context compose` (MIG-CTX-11), checked against
 * the golden envelopes by `data.test.ts`. The `context_compose` tool answers with the same object. */

const layerIssue = obj({ key: str, level: str, message: str });

/** What a layer says about where and how it is delivered. */
const delivery = {
  delivery: str,
  folders: arr(str),
  imports: arr(str),
  scope: str,
};

export const contextClientAddData = obj({
  ...delivery,
  created: bool,
  dryRun: bool,
  glob: str,
  issues: arr(layerIssue),
  name: str,
  path: str,
  plan: planV1,
  result: nullable(resultV1),
  rule: str,
});
export type ContextClientAddData = Infer<typeof contextClientAddData>;

export const contextClientEditData = obj({
  ...delivery,
  changed: bool,
  dryRun: bool,
  issues: arr(layerIssue),
  name: str,
  path: str,
  plan: planV1,
  result: nullable(resultV1),
});
export type ContextClientEditData = Infer<typeof contextClientEditData>;

export const contextClientRmData = obj({
  dryRun: bool,
  name: str,
  path: str,
  plan: planV1,
  result: nullable(resultV1),
});
export type ContextClientRmData = Infer<typeof contextClientRmData>;

export const contextClientListData = obj({
  layers: arr(
    obj({
      ...delivery,
      deployedTo: arr(str),
      description: str,
      globs: arr(str),
      issues: arr(layerIssue),
      name: str,
      path: str,
    }),
  ),
});
export type ContextClientListData = Infer<typeof contextClientListData>;

const composePart = obj({
  kind: str,
  layers: arr(str),
  lazy: bool,
  name: str,
  origin,
  path: str,
  source: str,
  text: str,
  tokens: obj({ basis: str, value: num }),
  via: arr(str),
  writable: bool,
});

export const contextComposeData = obj({
  cwd: str,
  levels: arr(obj({ dir: str, files: arr(str) })),
  notes: arr(str),
  parts: arr(composePart),
  skipped: arr(obj({ path: nullable(str), reason: str })),
  total: obj({ basis: str, value: num }),
});
export type ContextComposeData = Infer<typeof contextComposeData>;

export const contextLayerShapes: Record<string, Shape<unknown>> = {
  "context-client-add.apply": contextClientAddData,
  "context-client-add.exists": contextClientAddData,
  "context-client-add.folder": contextClientAddData,
  "context-client-add.folder-preview": contextClientAddData,
  "context-client-add.list": contextClientListData,
  "context-client-add.preview": contextClientAddData,
  "context-client-add.scaffold": contextClientAddData,
  "context-client-edit.apply": contextClientEditData,
  "context-client-edit.delivery": contextClientEditData,
  "context-client-edit.list": contextClientListData,
  "context-client-edit.preview": contextClientEditData,
  "context-client-edit.unchanged": contextClientEditData,
  "context-client-list": contextClientListData,
  "context-client-rm.apply": contextClientRmData,
  "context-client-rm.list": contextClientListData,
  "context-client-rm.preview": contextClientRmData,
  "context-client-rm.shared": contextClientRmData,
  "context-compose.before": contextComposeData,
  "context-compose.client": contextComposeData,
  "context-compose.layers": contextComposeData,
  "context-compose.list": contextClientListData,
  "context-compose.outside": contextComposeData,
};
