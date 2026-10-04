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

/** The error of a failed golden, as a bridge reply (`data` too, when the command prints it). */
export const goldenFailureOf = (stem: string) => {
  const { envelope } = JSON.parse(readFileSync(join(dir, `${stem}.json`), "utf8"));
  return {
    code: envelope.error.code,
    message: envelope.error.message,
    data: envelope.data,
  };
};

/** A ledger with two providers; the golden world has none. Field names are the serde names of
 * `ProviderSummary` plus the two derived fields `summary_json` adds. */
export const ledgerData = () => ({
  ...goldenData("compression-ledger-summary"),
  tokensSaved: 6600,
  providers: [
    {
      provider: "headroom",
      launches: 7,
      routed: 6,
      plain: 1,
      savingsEntries: 3,
      tokensBefore: 20000,
      tokensAfter: 8000,
      tokensSaved: 12000,
      savedPercent: 60,
    },
    {
      provider: "rtk-only",
      launches: 4,
      routed: 0,
      plain: 4,
      savingsEntries: 2,
      tokensBefore: 1000,
      tokensAfter: 1400,
      tokensSaved: -400,
      savedPercent: -40,
    },
  ],
});
