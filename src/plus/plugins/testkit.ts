import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { CcUpdateData } from "../types/cc";
import type { PluginsLsData, PluginsShowData } from "../types/plugins";
import { createBridge, goldenData, failure, wire, type Bridge } from "../skills/testkit";

export { failure, goldenData, wire, type Bridge };

export const FOLDER = "/fixture/home/work/acme-erp";
export const OTHER = "/fixture/home/work/side-project";
export const SENTINEL = "SENTINEL-NOT-FOR-THE-DOM";

/** The goldens scrub the fixture home to `<WORLD>`; a test reads it as `/fixture`. */
export const unscrub = <T>(data: T): T =>
  JSON.parse(JSON.stringify(data).split("<WORLD>").join("/fixture")) as T;

export const golden = <T>(stem: string): T => unscrub(goldenData(stem)) as T;

export function goldenFailure(stem: string) {
  const dir = join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes");
  const { error, data } = JSON.parse(
    readFileSync(join(dir, `${stem}.json`), "utf8"),
  ).envelope;
  return failure(error.code, unscrub(error.message as string), data);
}

export const lsData = (): PluginsLsData => golden("plugins-ls.measured");
export const showData = (): PluginsShowData => {
  const data = golden<PluginsShowData>("plugins-show.cli");
  return { ...data, version: "2.2.0" };
};

/** The data of a plugin that has no adapter. */
export const plainShow = (): PluginsShowData => ({
  ...showData(),
  id: "demo-plugin@fake-market",
  name: "demo-plugin",
  adapter: null,
  knobs: [],
  mcpServers: [],
  hooks: [],
  options: [],
});

export const ccPreview = (): CcUpdateData => golden("cc-update.preview");
export const ccApply = (): CcUpdateData => golden("cc-update.apply");

export const lsArgv = (cwd = "") => `plugins ls${cwd ? ` --cwd ${cwd}` : ""}`;
export const showArgv = (id: string, cwd = "") =>
  `plugins show ${id}${cwd ? ` --cwd ${cwd}` : ""}`;

/** The Skills bridge plus the plugin commands of the goldens, for the folder `FOLDER`. A write
 * changes what the next read answers, like the real command. */
export function createPluginsBridge(): Bridge & { state: { show: PluginsShowData } } {
  const bridge = createBridge();
  const state = { show: showData() };
  for (const cwd of ["", FOLDER]) {
    bridge.set(lsArgv(cwd), () => lsData());
    bridge.set(showArgv("ecc@ecc", cwd), () => state.show);
  }
  bridge.set("plugins config", () => null);
  return Object.assign(bridge, { state });
}
