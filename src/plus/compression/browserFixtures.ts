import { ctlFailure } from "../fixtures/servers";
import disableApply from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-disable.apply.json";
import disablePreview from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-disable.preview.json";
import doctor from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-doctor.json";
import enableApply from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-enable.apply.json";
import enablePreview from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-enable.preview.json";
import env from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-env.json";
import ledgerRecord from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-ledger-record.apply.json";
import pin from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-pin.json";
import presets from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-presets.json";
import runPlan from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-run.plan.json";
import sealApply from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-seal.apply.json";
import sealPreview from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-seal.preview.json";
import setProviderApply from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-set-provider.apply.json";
import setProviderPreview from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-set-provider.preview.json";
import status from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-status.json";
import syncApply from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-sync.apply.json";
import syncPreview from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-sync.preview.json";
import updatePreview from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-update.preview.json";
import usePreview from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-use.preview.json";
import useApply from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-use.apply.json";
import verify from "../../../src-tauri/tests/fixtures/ctl-envelopes/compression-verify.measured.json";

type Golden = { envelope: { data: unknown } };
const data = (golden: Golden) => golden.envelope.data;

const FOLDER = "/fixture/home/project";

const today = {
  ...(data(status) as object),
  configExists: true,
  provider: "rtk-only",
  runtime: "hook",
};

const ledger = {
  launchesPath: "/fixture/data/compression-launches.jsonl",
  savingsPath: "/fixture/data/compression-savings.jsonl",
  tokensSaved: 11600,
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
};

const installDry = {
  ...(data(pin) as { requirement: string }),
  dryRun: true,
  install: {
    requirement: (data(pin) as { requirement: string }).requirement,
    dryRun: true,
  },
  restartProxies: false,
};

/** What the dev browser fixture (`plusCtl.ts`) answers for the Compression tab: the golden
 * envelopes of the real commands, with today's state (rtk-only, hook runtime) as the status. */
export const compressionBrowserFixtures: Array<[string, unknown]> = [
  ["compression status", today],
  ["compression presets", data(presets)],
  ["compression pin", data(pin)],
  ["compression doctor", data(doctor)],
  ["compression ledger summary", ledger],
  ["compression set-provider headroom --dry-run", data(setProviderPreview)],
  ["compression set-provider headroom", data(setProviderApply)],
  ["compression use agent --dry-run", data(usePreview)],
  ["compression use agent", data(useApply)],
  ["compression enable --provider rtk-only --dry-run", data(enablePreview)],
  ["compression enable --provider rtk-only", data(enableApply)],
  [
    "compression disable --dry-run",
    { ...(data(disablePreview) as object), teardown: false },
  ],
  ["compression disable --teardown --dry-run", data(disablePreview)],
  ["compression disable", { ...(data(disableApply) as object), teardown: false }],
  ["compression disable --teardown", data(disableApply)],
  ["compression sync --dry-run", data(syncPreview)],
  ["compression sync", data(syncApply)],
  ["compression pin --install --dry-run", installDry],
  ["compression pin --refresh --dry-run", { ...installDry, install: null }],
  [
    "compression presets --refresh --dry-run",
    { ...(data(presets) as object), refresh: { version: "0.29.0", presets: [] } },
  ],
  ["compression update --latest", data(updatePreview)],
  [
    "compression update --latest --accept",
    { ...(data(updatePreview) as object), accepted: true },
  ],
  ["compression seal --dry-run", data(sealPreview)],
  ["compression seal --apply", data(sealApply)],
  ["compression proxy up", ctlFailure("proxy_up", "headroom not on PATH")],
  ["compression proxy down", ctlFailure("proxy_down", "no proxy listening on :29213")],
  ["compression proxy restart", ctlFailure("proxy_up", "headroom not on PATH")],
  ["compression verify", data(verify)],
  [
    "compression ledger record --provider rtk-only --before 1000 --after 400",
    data(ledgerRecord),
  ],
  [`compression run --plan --cwd ${FOLDER} claude`, data(runPlan)],
  [`compression env --cwd ${FOLDER}`, data(env)],
];
