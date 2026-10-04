import { readFileSync } from "node:fs";
import { join } from "node:path";

const dir = join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes");

/** `data` of a real golden envelope, e.g. `goldenData("compression-status")`. */
export const goldenData = (stem: string) =>
  JSON.parse(readFileSync(join(dir, `${stem}.json`), "utf8")).envelope.data;

/** Today's state of the Mac: the golden status with the provider and runtime of rtk-only. */
export const statusData = () => {
  const golden = goldenData("compression-status");
  return {
    ...golden,
    configExists: true,
    provider: "rtk-only",
    runtime: "hook",
    shims: { ...golden.shims, exists: false },
  };
};

export const presetsData = () => goldenData("compression-presets");
