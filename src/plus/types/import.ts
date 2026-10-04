import {
  any,
  arr,
  bool,
  num,
  obj,
  rec,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** `data` of the import commands, checked against the golden envelopes by `data.test.ts`. */

export const importMcpmData = obj({
  clientDiscovery: arr(any),
  clientScopes: arr(any),
  clients: arr(any),
  counts: rec(num),
  dryRun: bool,
  profiles: arr(
    obj({
      action: str,
      id: str,
    }),
  ),
  rejects: arr(any),
  scripts: arr(any),
  secrets: arr(any),
  servers: arr(
    obj({
      action: str,
      id: str,
    }),
  ),
  skillsSync: arr(any),
  skippedClients: arr(any),
  warnings: arr(any),
});
export type ImportMcpmData = Infer<typeof importMcpmData>;

export const importMcpmNameMapData = obj({
  count: num,
  map: rec(str),
  servers: rec(str),
});
export type ImportMcpmNameMapData = Infer<typeof importMcpmNameMapData>;

export const importRenameRefsData = obj({
  dead: arr(any),
  dryRun: bool,
  files: arr(
    obj({
      path: str,
      replaced: num,
    }),
  ),
  orphans: arr(any),
  replaced: num,
  scanned: num,
});
export type ImportRenameRefsData = Infer<typeof importRenameRefsData>;

/** Golden file stem to the shape of its envelope `data`. */
export const importShapes: Record<string, Shape<unknown>> = {
  "import-mcpm.apply": importMcpmData,
  "import-mcpm.preview": importMcpmData,
  "import-mcpm.name-map": importMcpmNameMapData,
  "import-rename-refs.apply": importRenameRefsData,
  "import-rename-refs.preview": importRenameRefsData,
};
