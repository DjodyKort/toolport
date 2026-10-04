import {
  any,
  arr,
  bool,
  nullable,
  num,
  obj,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** Results of the core self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

export const doctorResult = obj({
  checks: arr(
    obj({
      detail: str,
      name: str,
      status: str,
    }),
  ),
  healthy: bool,
});
export type DoctorResult = Infer<typeof doctorResult>;

export const flowDiagramResult = obj({
  markdown: str,
});
export type FlowDiagramResult = Infer<typeof flowDiagramResult>;

export const whereAmIResult = obj({
  activeProfile: str,
  auth: obj({
    counts: obj({
      expiring: num,
      misconfigured: num,
      needs_reauth: num,
      ok: num,
      revoked: num,
      unknown: num,
      unreachable: num,
    }),
    servers: arr(any),
  }),
  dataDir: str,
  gateway: obj({
    build: nullable(any),
    builds: arr(any),
    path: str,
    present: bool,
  }),
  profileCount: num,
  registry: obj({
    error: nullable(str),
    exists: bool,
    path: str,
    readable: bool,
  }),
  secretsBackend: str,
  serverCount: num,
  version: str,
});
export type WhereAmIResult = Infer<typeof whereAmIResult>;

/** Tool name to the shape of its `structuredContent`. */
export const coreToolShapes: Record<string, Shape<unknown>> = {
  doctor: doctorResult,
  flow_diagram: flowDiagramResult,
  where_am_i: whereAmIResult,
};
