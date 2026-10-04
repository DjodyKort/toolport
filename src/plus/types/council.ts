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

/** `data` of the council commands, checked against the golden envelopes by `data.test.ts`. */

export const councilDoctorData = obj({
  checks: arr(
    obj({
      detail: str,
      name: str,
      ok: bool,
    }),
  ),
});
export type CouncilDoctorData = Infer<typeof councilDoctorData>;

export const councilInstallData = obj({
  created: bool,
  id: str,
  keyStored: bool,
});
export type CouncilInstallData = Infer<typeof councilInstallData>;

export const councilToolsData = obj({
  resources: arr(
    obj({
      summary: str,
      uri: str,
    }),
  ),
  tools: arr(
    obj({
      name: str,
      summary: str,
      tier: num,
    }),
  ),
});
export type CouncilToolsData = Infer<typeof councilToolsData>;

export const councilUninstallData = obj({
  id: nullable(str),
  keyPurged: bool,
  removed: bool,
});
export type CouncilUninstallData = Infer<typeof councilUninstallData>;

/** Golden file stem to the shape of its envelope `data`. */
export const councilShapes: Record<string, Shape<unknown>> = {
  "council-doctor": councilDoctorData,
  "council-install.apply": councilInstallData,
  "council-install.key": councilInstallData,
  "council-tools": councilToolsData,
  "council-uninstall.again": councilUninstallData,
  "council-uninstall.apply": councilUninstallData,
};
