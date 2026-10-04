import {
  any,
  arr,
  bool,
  nullable,
  num,
  obj,
  opt,
  rec,
  str,
  type Infer,
  type Shape,
} from "../bridge/shape";

/** `data` of the compression commands, checked against the golden envelopes by `data.test.ts`. */

export const compressionDisableData = obj({
  actions: arr(str),
  adopted: nullable(any),
  dryRun: bool,
  preset: obj({
    mode: str,
    name: str,
    port: num,
    savingsProfile: nullable(str),
  }),
  provider: str,
  removed: arr(any),
  runtime: str,
  teardown: bool,
  warnings: arr(str),
  written: arr(any),
});
export type CompressionDisableData = Infer<typeof compressionDisableData>;

export const compressionDoctorData = obj({
  checks: arr(
    obj({
      detail: str,
      name: str,
      ok: bool,
    }),
  ),
  healthy: bool,
  migrated: arr(any),
  provider: str,
});
export type CompressionDoctorData = Infer<typeof compressionDoctorData>;

export const compressionEnableData = obj({
  actions: arr(str),
  adopted: nullable(any),
  dryRun: bool,
  nextSteps: arr(str),
  preset: obj({
    mode: str,
    name: str,
    port: num,
    savingsProfile: nullable(str),
  }),
  provider: str,
  removed: arr(any),
  runtime: str,
  warnings: arr(any),
  written: arr(any),
});
export type CompressionEnableData = Infer<typeof compressionEnableData>;

export const compressionEnvData = obj({
  cwd: str,
  env: rec(str),
  launch: str,
  lines: arr(str),
  port: nullable(num),
  preset: str,
  provider: str,
});
export type CompressionEnvData = Infer<typeof compressionEnvData>;

export const compressionLedgerRecordData = obj({
  path: str,
  recorded: obj({
    provider: str,
    session: str,
    source: str,
    tokens_after: num,
    tokens_before: num,
    ts: str,
  }),
  tokensSaved: num,
});
export type CompressionLedgerRecordData = Infer<typeof compressionLedgerRecordData>;

export const compressionLedgerProvider = obj({
  launches: num,
  plain: num,
  provider: str,
  routed: num,
  savedPercent: nullable(num),
  savingsEntries: num,
  tokensAfter: num,
  tokensBefore: num,
  tokensSaved: num,
});
export type CompressionLedgerProvider = Infer<typeof compressionLedgerProvider>;

export const compressionLedgerSummaryData = obj({
  launchesPath: str,
  providers: arr(compressionLedgerProvider),
  savingsPath: str,
  tokensSaved: num,
});
export type CompressionLedgerSummaryData = Infer<typeof compressionLedgerSummaryData>;

export const compressionPinData = obj({
  adopted: nullable(any),
  drift: bool,
  dryRun: bool,
  install: nullable(any),
  installed: nullable(any),
  pin: str,
  refresh: nullable(any),
  requirement: str,
  set: bool,
});
export type CompressionPinData = Infer<typeof compressionPinData>;

export const compressionPresetsData = obj({
  active: str,
  presets: arr(
    obj({
      active: bool,
      knobCount: num,
      mode: str,
      name: str,
      port: num,
      savingsProfile: nullable(str),
      snapshotVersion: nullable(str),
    }),
  ),
});
export type CompressionPresetsData = Infer<typeof compressionPresetsData>;

export const compressionRunData = obj({
  argv: arr(str),
  cwd: str,
  env: obj({
    set: rec(str),
    unset: arr(str),
  }),
  installed: nullable(any),
  ledger: obj({
    cwd: str,
    pin: str,
    port: nullable(num),
    preset: str,
    provider: str,
    routed: bool,
  }),
  pin: str,
  preset: str,
  program: str,
  provider: str,
  proxy: nullable(any),
  routed: bool,
  warnings: arr(any),
});
export type CompressionRunData = Infer<typeof compressionRunData>;

export const compressionSealData = obj({
  apply: bool,
  complete: bool,
  declarable: arr(
    obj({
      knob: str,
      value: str,
    }),
  ),
  dryRun: bool,
  port: num,
  preset: str,
  sealed: num,
  unset: arr(str),
  version: nullable(str),
});
export type CompressionSealData = Infer<typeof compressionSealData>;

export const compressionSetProviderData = obj({
  actions: arr(str),
  adopted: nullable(any),
  dryRun: bool,
  preset: obj({
    mode: str,
    name: str,
    port: num,
    savingsProfile: nullable(str),
  }),
  provider: str,
  removed: arr(any),
  runtime: str,
  warnings: arr(str),
  written: arr(str),
});
export type CompressionSetProviderData = Infer<typeof compressionSetProviderData>;

export const compressionSyncData = obj({
  actions: arr(str),
  adopted: nullable(any),
  dryRun: bool,
  preset: obj({
    mode: str,
    name: str,
    port: num,
    savingsProfile: nullable(str),
  }),
  provider: str,
  removed: arr(any),
  runtime: str,
  warnings: arr(any),
  written: arr(any),
});
export type CompressionSyncData = Infer<typeof compressionSyncData>;

export const compressionUpdateData = obj({
  accepted: bool,
  current: str,
  same: bool,
  target: str,
});
export type CompressionUpdateData = Infer<typeof compressionUpdateData>;

export const compressionUseData = obj({
  actions: arr(str),
  adopted: nullable(any),
  dryRun: bool,
  preset: obj({
    mode: str,
    name: str,
    port: num,
    savingsProfile: str,
  }),
  provider: str,
  removed: arr(any),
  runtime: str,
  warnings: arr(any),
  written: arr(any),
});
export type CompressionUseData = Infer<typeof compressionUseData>;

const transcriptBucket = obj({
  cacheCreate: num,
  cacheRead: num,
  inputTokens: num,
  outputTokens: num,
  readRatio: nullable(num),
  readWrite: nullable(num),
  sessions: num,
  turns: num,
});

export const compressionVerifyData = obj({
  buckets: nullable(
    obj({
      plain: transcriptBucket,
      proxied: transcriptBucket,
      unattributed: transcriptBucket,
    }),
  ),
  checks: arr(
    obj({
      detail: str,
      name: str,
      ok: bool,
    }),
  ),
  provider: str,
  transcripts: obj({
    count: num,
    root: str,
  }),
  verdict: opt(nullable(any)),
});
export type CompressionVerifyData = Infer<typeof compressionVerifyData>;

/** Golden file stem to the shape of its envelope `data`. */
export const compressionShapes: Record<string, Shape<unknown>> = {
  "compression-disable.apply": compressionDisableData,
  "compression-disable.preview": compressionDisableData,
  "compression-doctor": compressionDoctorData,
  "compression-enable.apply": compressionEnableData,
  "compression-enable.preview": compressionEnableData,
  "compression-env": compressionEnvData,
  "compression-ledger-record.apply": compressionLedgerRecordData,
  "compression-ledger-summary": compressionLedgerSummaryData,
  "compression-pin": compressionPinData,
  "compression-presets": compressionPresetsData,
  "compression-run.plan": compressionRunData,
  "compression-seal.again": compressionSealData,
  "compression-seal.apply": compressionSealData,
  "compression-seal.preview": compressionSealData,
  "compression-set-provider.apply": compressionSetProviderData,
  "compression-set-provider.preview": compressionSetProviderData,
  "compression-sync.apply": compressionSyncData,
  "compression-sync.preview": compressionSyncData,
  "compression-update.preview": compressionUpdateData,
  "compression-use.apply": compressionUseData,
  "compression-use.preview": compressionUseData,
  "compression-verify.measured": compressionVerifyData,
  "compression-verify.no-transcripts": compressionVerifyData,
};
