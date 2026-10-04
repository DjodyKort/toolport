import { arr, bool, obj, str, type Infer, type Shape } from "../bridge/shape";
import { whereAmIResult } from "./selfmcp-core";

/** The body of each self-MCP resource: parsed JSON, or the text itself. */

export const resourceClientsData = obj({
  clients: arr(
    obj({
      appPresent: bool,
      gatewayInstalled: bool,
      id: str,
      name: str,
    }),
  ),
});
export type ResourceClientsData = Infer<typeof resourceClientsData>;

export const resourceRouterStatusData = obj({
  note: str,
  router: obj({
    decision: str,
    present: bool,
    replacedBy: str,
    status: str,
  }),
});
export type ResourceRouterStatusData = Infer<typeof resourceRouterStatusData>;

/** Golden file stem to the shape of the resource body. */
export const resourceShapes: Record<string, Shape<unknown>> = {
  "resource-architecture": str,
  "resource-clients": resourceClientsData,
  "resource-flow": str,
  "resource-inventory-agents": str,
  "resource-inventory-servers": str,
  "resource-inventory-skills": str,
  "resource-inventory-styles": str,
  "resource-paths": whereAmIResult,
  "resource-router-status": resourceRouterStatusData,
  "resource-status": whereAmIResult,
  "resource-workflows": str,
};
