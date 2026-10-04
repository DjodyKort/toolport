import { any, arr, num, obj, rec, str, type Infer, type Shape } from "../bridge/shape";

/** `data` of the usage commands, checked against the golden envelopes by `data.test.ts`. */

export const usageData = obj({
  byDay: rec(
    obj({
      cacheCreation: num,
      cacheRead: num,
      input: num,
      messages: num,
      output: num,
    }),
  ),
  byMcpServer: obj({
    github: obj({
      calls: num,
      tools: obj({
        mcp__github__list: num,
      }),
    }),
  }),
  byModel: rec(
    obj({
      cacheCreation: num,
      cacheRead: num,
      input: num,
      messages: num,
      output: num,
    }),
  ),
  bySession: obj({
    s1: obj({
      cacheCreation: num,
      cacheRead: num,
      cwd: str,
      firstTs: str,
      input: num,
      lastTs: str,
      messages: num,
      output: num,
    }),
  }),
  index: obj({
    files: num,
    messages: num,
  }),
  mcpFailures: arr(any),
  otel: obj({
    apiRequests: obj({
      costUsd: num,
      count: num,
      durationMs: num,
    }),
    costByModel: rec(any),
    costUsd: num,
    events: num,
    mcpConnections: obj({
      failures: arr(any),
      total: num,
    }),
    tokens: rec(any),
    toolDecisions: rec(any),
  }),
  sources: obj({
    otelOnly: num,
    otelRequests: num,
    transcriptMessages: num,
  }),
  totals: obj({
    cacheCreation: num,
    cacheRead: num,
    input: num,
    messages: num,
    output: num,
  }),
});
export type UsageData = Infer<typeof usageData>;

/** Golden file stem to the shape of its envelope `data`. */
export const usageShapes: Record<string, Shape<unknown>> = {
  "usage.apply": usageData,
  "usage.cached": usageData,
};
