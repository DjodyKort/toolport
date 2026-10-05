import type { HooksLsData } from "../types/plugins";
import { createBridge, goldenData, wire, type Bridge } from "../skills/testkit";

export { wire, type Bridge };

export const FOLDER = "/fixture/home/work/acme-erp";
export const QUIET = "/fixture/home/work/quiet";
export const SENTINEL = "SENTINEL-NOT-FOR-THE-DOM";

export const golden = (stem: string): HooksLsData =>
  JSON.parse(
    JSON.stringify(goldenData(stem)).split("<WORLD>").join("/fixture"),
  ) as HooksLsData;

export const lsArgv = (cwd = ""): string => `hooks ls${cwd ? ` --cwd ${cwd}` : ""}`;

/** The Skills bridge plus `hooks ls` for three folders, answered from the real goldens. */
export function createHooksBridge(): Bridge {
  const bridge = createBridge();
  bridge.set(lsArgv(), () => golden("hooks-ls.full"));
  bridge.set(lsArgv(FOLDER), () => golden("hooks-ls.full"));
  bridge.set(lsArgv(QUIET), () => golden("hooks-ls.disabled"));
  return bridge;
}
