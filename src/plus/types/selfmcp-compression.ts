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

/** Results of the compression self-MCP tools, checked against the golden results by `selfmcp.test.ts`. */

export const compressionDisableResult = obj({
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
  removed: arr(str),
  runtime: str,
  teardown: bool,
  warnings: arr(any),
  written: arr(any),
});
export type CompressionDisableResult = Infer<typeof compressionDisableResult>;

export const compressionEnableResult = obj({
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
export type CompressionEnableResult = Infer<typeof compressionEnableResult>;

export const compressionSealResult = obj({
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
export type CompressionSealResult = Infer<typeof compressionSealResult>;

export const compressionSetProviderResult = obj({
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
export type CompressionSetProviderResult = Infer<typeof compressionSetProviderResult>;

export const compressionStatusResult = obj({
  configExists: bool,
  configPath: str,
  contexts: num,
  migrationNotes: arr(any),
  pin: obj({
    drift: nullable(bool),
    installed: nullable(any),
    package: str,
    pin: str,
    requirement: str,
  }),
  preset: obj({
    knobCount: num,
    mode: str,
    name: str,
    port: num,
    savingsProfile: nullable(str),
    snapshotVersion: nullable(str),
  }),
  provider: str,
  runtime: str,
  scope: arr(str),
  shims: obj({
    exists: bool,
    path: str,
  }),
});
export type CompressionStatusResult = Infer<typeof compressionStatusResult>;

export const compressionSyncResult = obj({
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
export type CompressionSyncResult = Infer<typeof compressionSyncResult>;

export const compressionUseResult = obj({
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
export type CompressionUseResult = Infer<typeof compressionUseResult>;

/** Tool name to the shape of its `structuredContent`. */
export const compressionToolShapes: Record<string, Shape<unknown>> = {
  compression_disable: compressionDisableResult,
  compression_enable: compressionEnableResult,
  compression_seal: compressionSealResult,
  compression_set_provider: compressionSetProviderResult,
  compression_status: compressionStatusResult,
  compression_sync: compressionSyncResult,
  compression_use: compressionUseResult,
};
