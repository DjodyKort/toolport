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

/** `data` of the obs commands, checked against the golden envelopes by `data.test.ts`. */

export const obsOtelDisableData = obj({
  actions: arr(str),
  changed: bool,
  dryRun: bool,
  enabled: bool,
  kept: arr(any),
  port: num,
  settingsPath: str,
});
export type ObsOtelDisableData = Infer<typeof obsOtelDisableData>;

export const obsOtelEnableData = obj({
  actions: arr(str),
  changed: bool,
  conflicts: arr(any),
  dryRun: bool,
  enabled: bool,
  endpoint: str,
  port: num,
  settingsPath: str,
  warnings: arr(any),
});
export type ObsOtelEnableData = Infer<typeof obsOtelEnableData>;

export const obsOtelStatusData = obj({
  enabled: bool,
  endpoint: str,
  events: obj({
    count: num,
    latest: nullable(any),
  }),
  port: num,
  receiver: obj({
    listening: bool,
    state: str,
  }),
  settings: obj({
    exists: bool,
    keys: obj({
      CLAUDE_CODE_ENABLE_TELEMETRY: str,
      OTEL_EXPORTER_OTLP_ENDPOINT: str,
      OTEL_EXPORTER_OTLP_PROTOCOL: str,
      OTEL_LOGS_EXPORTER: str,
      OTEL_METRICS_EXPORTER: str,
    }),
    path: str,
    state: str,
    warnings: arr(any),
  }),
});
export type ObsOtelStatusData = Infer<typeof obsOtelStatusData>;

/** Golden file stem to the shape of its envelope `data`. */
export const obsShapes: Record<string, Shape<unknown>> = {
  "obs-otel-disable.apply": obsOtelDisableData,
  "obs-otel-disable.preview": obsOtelDisableData,
  "obs-otel-enable.apply": obsOtelEnableData,
  "obs-otel-enable.preview": obsOtelEnableData,
  "obs-otel-status": obsOtelStatusData,
};
