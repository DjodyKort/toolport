import {
  bool,
  lit,
  nullable,
  num,
  obj,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** Checked against the golden envelopes by `data.test.ts` and `selfmcp.test.ts`. */
export const catalogIndexStatus = obj({
  count: num,
  lastSyncedAt: nullable(str),
  complete: bool,
  running: bool,
});
export type CatalogIndexStatus = Infer<typeof catalogIndexStatus>;

export const catalogSyncReport = obj({
  kind: lit("full", "resume", "incremental"),
  pages: num,
  fetched: num,
  count: num,
  complete: bool,
  lastSyncedAt: nullable(str),
});
export type CatalogSyncReport = Infer<typeof catalogSyncReport>;

export const catalogShapes: Record<string, Shape<unknown>> = {
  "catalog-status.default": catalogIndexStatus,
};

export const catalogToolShapes: Record<string, Shape<unknown>> = {
  catalog_status: catalogIndexStatus,
};
