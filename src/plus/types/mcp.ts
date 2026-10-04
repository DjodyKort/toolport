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

/** `data` of the mcp commands, checked against the golden envelopes by `data.test.ts`. */

export const mcpDoctorData = obj({
  activeProfile: nullable(str),
  checks: arr(
    obj({
      detail: str,
      name: str,
      ok: bool,
    }),
  ),
  clientProfiles: arr(any),
  state: str,
});
export type McpDoctorData = Infer<typeof mcpDoctorData>;

export const mcpInstallData = obj({
  action: str,
  command: str,
  enabled: arr(str),
  id: str,
  profile: nullable(str),
});
export type McpInstallData = Infer<typeof mcpInstallData>;

export const mcpToolsData = obj({
  resources: arr(
    obj({
      description: str,
      mimeType: str,
      name: str,
      uri: str,
    }),
  ),
  server: str,
  tools: arr(
    obj({
      description: str,
      dryRunDefault: bool,
      gate: str,
      name: str,
      params: arr(
        obj({
          description: str,
          name: str,
          required: bool,
          type: str,
        }),
      ),
      tier: num,
    }),
  ),
});
export type McpToolsData = Infer<typeof mcpToolsData>;

export const mcpUninstallData = obj({
  id: str,
  removed: bool,
});
export type McpUninstallData = Infer<typeof mcpUninstallData>;

/** Golden file stem to the shape of its envelope `data`. */
export const mcpShapes: Record<string, Shape<unknown>> = {
  "mcp-doctor": mcpDoctorData,
  "mcp-install.apply": mcpInstallData,
  "mcp-install.profile": mcpInstallData,
  "mcp-tools": mcpToolsData,
  "mcp-uninstall.apply": mcpUninstallData,
};
