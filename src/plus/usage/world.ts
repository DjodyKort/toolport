/** A synthetic transcript index for the Usage tab: made-up messages, aggregated the way
 * `toolportctl usage` aggregates them (once per message id, by day, model, session and MCP
 * server), so a test can compare the screen with numbers it worked out another way. The
 * shape is the one of the golden `usage.apply.json`. */

export interface Msg {
  id: string;
  day: string;
  ts: string;
  session: string;
  cwd: string;
  model: string;
  input: number;
  output: number;
  cacheCreation: number;
  cacheRead: number;
  tools: string[];
}

const SESSIONS = [
  {
    id: "s-erp-1",
    cwd: "/work/acme-erp",
    model: "claude-a",
    from: 18,
    to: 26,
    tool: "mcp__github__list",
  },
  {
    id: "s-erp-2",
    cwd: "/work/acme-erp",
    model: "claude-b",
    from: 27,
    to: 30,
    tool: "mcp__corp-tools__search",
  },
  {
    id: "s-repo-1",
    cwd: "/work/client-repo",
    model: "claude-a",
    from: 24,
    to: 33,
    tool: "mcp__github__get",
  },
  { id: "s-notes-1", cwd: "/work/notes", model: "claude-a", from: 29, to: 33, tool: "" },
] as const;

const dayOf = (offset: number) => {
  const date = new Date(Date.UTC(2026, 8, offset));
  return date.toISOString().slice(0, 10);
};

/** Messages from 2026-09-18 to 2026-10-03, two per session and day. */
export function worldMessages(): Msg[] {
  const out: Msg[] = [];
  SESSIONS.forEach((session, s) => {
    for (let offset = session.from; offset <= session.to; offset += 1) {
      for (let n = 0; n < 2; n += 1) {
        const k = offset * 7 + s * 13 + n * 5;
        const day = dayOf(offset);
        out.push({
          id: `${session.id}-${offset}-${n}`,
          day,
          ts: `${day}T${String(9 + n * 3).padStart(2, "0")}:${String(k % 60).padStart(2, "0")}:00Z`,
          session: session.id,
          cwd: session.cwd,
          model: session.model,
          input: 20 + (k % 40),
          output: 150 + (k % 300),
          cacheCreation: 900 + (k % 7) * 400,
          cacheRead: 12_000 + (k % 11) * 3_500,
          tools: n === 0 && session.tool ? [session.tool, session.tool] : [],
        });
      }
    }
  });
  return out;
}

interface Counts {
  messages: number;
  input: number;
  output: number;
  cacheCreation: number;
  cacheRead: number;
}

const zero = (): Counts => ({
  messages: 0,
  input: 0,
  output: 0,
  cacheCreation: 0,
  cacheRead: 0,
});

function add(into: Counts, msg: Msg) {
  into.messages += 1;
  into.input += msg.input;
  into.output += msg.output;
  into.cacheCreation += msg.cacheCreation;
  into.cacheRead += msg.cacheRead;
}

export const OTEL_ONLY_MESSAGE: Msg = {
  id: "otel-only-1",
  day: "2026-10-03",
  ts: "2026-10-03T16:30:00Z",
  session: "s-erp-2",
  cwd: "/work/acme-erp",
  model: "claude-b",
  input: 50,
  output: 400,
  cacheCreation: 0,
  cacheRead: 8_000,
  tools: [],
};

export interface Aggregated {
  totals: Counts;
  byDay: Record<string, Counts>;
  byModel: Record<string, Counts>;
  bySession: Record<string, Counts & { cwd: string; firstTs: string; lastTs: string }>;
  byMcpServer: Record<string, { calls: number; tools: Record<string, number> }>;
}

/** The CLI's own aggregation of a list of messages (each id counts once). */
export function aggregate(messages: Msg[]): Aggregated {
  const seen = new Set<string>();
  const out: Aggregated = {
    totals: zero(),
    byDay: {},
    byModel: {},
    bySession: {},
    byMcpServer: {},
  };
  for (const msg of messages) {
    if (seen.has(msg.id)) continue;
    seen.add(msg.id);
    add(out.totals, msg);
    add((out.byDay[msg.day] ??= zero()), msg);
    add((out.byModel[msg.model] ??= zero()), msg);
    const session = (out.bySession[msg.session] ??= {
      ...zero(),
      cwd: msg.cwd,
      firstTs: msg.ts,
      lastTs: msg.ts,
    });
    add(session, msg);
    if (msg.ts < session.firstTs) session.firstTs = msg.ts;
    if (msg.ts > session.lastTs) session.lastTs = msg.ts;
    for (const tool of msg.tools) {
      const server = tool.split("__")[1];
      const agg = (out.byMcpServer[server] ??= { calls: 0, tools: {} });
      agg.calls += 1;
      agg.tools[tool] = (agg.tools[tool] ?? 0) + 1;
    }
  }
  return out;
}

export function usageWorld(): Record<string, unknown> {
  const transcript = worldMessages();
  const all = [...transcript, OTEL_ONLY_MESSAGE];
  return {
    ...aggregate(all),
    index: { files: 4, messages: transcript.length },
    mcpFailures: [
      { session: "s-erp-2", server: "docs-server", ts: "2026-10-02T11:00:01Z" },
    ],
    otel: {
      apiRequests: { costUsd: 0.25, count: 5, durationMs: 9100 },
      costByModel: { "claude-a": 0.75, "claude-b": 0.5 },
      costUsd: 1.25,
      events: 17,
      mcpConnections: {
        failures: [{ count: 1, lastErrorCode: "ECONNREFUSED", server: "docs-server" }],
        total: 3,
      },
      tokens: { cacheCreation: 300, cacheRead: 9000, input: 100, output: 40 },
      toolDecisions: { accept: 5, reject: 1 },
    },
    sources: {
      otelOnly: 1,
      otelRequests: 5,
      transcriptMessages: transcript.length,
    },
  };
}

/** An index that has never run. */
export function emptyUsage(): Record<string, unknown> {
  return {
    ...aggregate([]),
    index: { files: 0, messages: 0 },
    mcpFailures: [],
    otel: {
      apiRequests: { costUsd: 0, count: 0, durationMs: 0 },
      costByModel: {},
      costUsd: 0,
      events: 0,
      mcpConnections: { failures: [], total: 0 },
      tokens: {},
      toolDecisions: {},
    },
    sources: { otelOnly: 0, otelRequests: 0, transcriptMessages: 0 },
  };
}

export const statusOn = {
  enabled: true,
  endpoint: "http://127.0.0.1:4318",
  events: { count: 17, latest: "2026-10-03T16:30:00Z" },
  port: 4318,
  receiver: { listening: true, state: "listening" },
  settings: {
    exists: true,
    keys: {
      CLAUDE_CODE_ENABLE_TELEMETRY: "ok",
      OTEL_EXPORTER_OTLP_ENDPOINT: "ok",
      OTEL_EXPORTER_OTLP_PROTOCOL: "ok",
      OTEL_LOGS_EXPORTER: "ok",
      OTEL_METRICS_EXPORTER: "ok",
    },
    path: "/fixture/home/.claude/settings.json",
    state: "configured",
    warnings: [],
  },
};

export const statusOff = {
  enabled: false,
  endpoint: "http://127.0.0.1:4318",
  events: { count: 0, latest: null },
  port: 4318,
  receiver: { listening: false, state: "disabled" },
  settings: {
    exists: true,
    keys: {
      CLAUDE_CODE_ENABLE_TELEMETRY: "missing",
      OTEL_EXPORTER_OTLP_ENDPOINT: "missing",
      OTEL_EXPORTER_OTLP_PROTOCOL: "missing",
      OTEL_LOGS_EXPORTER: "missing",
      OTEL_METRICS_EXPORTER: "missing",
    },
    path: "/fixture/home/.claude/settings.json",
    state: "missing",
    warnings: [],
  },
};

const enableActions = [
  "env.CLAUDE_CODE_ENABLE_TELEMETRY: added",
  "env.OTEL_METRICS_EXPORTER: added",
  "env.OTEL_LOGS_EXPORTER: added",
  "env.OTEL_EXPORTER_OTLP_PROTOCOL: added",
  "env.OTEL_EXPORTER_OTLP_ENDPOINT: added",
];

const disableActions = [
  "env.CLAUDE_CODE_ENABLE_TELEMETRY: removed",
  "env.OTEL_EXPORTER_OTLP_ENDPOINT: removed",
  "env.OTEL_EXPORTER_OTLP_PROTOCOL: removed",
  "env.OTEL_LOGS_EXPORTER: removed",
  "env.OTEL_METRICS_EXPORTER: removed",
];

const enableData = (dryRun: boolean, port: number) => ({
  actions: enableActions,
  changed: true,
  conflicts: [],
  dryRun,
  enabled: true,
  endpoint: `http://127.0.0.1:${port}`,
  port,
  settingsPath: "/fixture/home/.claude/settings.json",
  warnings: [],
});

const disableData = (dryRun: boolean, port: number) => ({
  actions: disableActions,
  changed: true,
  dryRun,
  enabled: false,
  kept: [],
  port,
  settingsPath: "/fixture/home/.claude/settings.json",
});

/** The dev browser starts with the receiver off, so Enable has something to preview. */
export const otelBrowserFixtures: Array<[string, unknown]> = [
  ["obs otel status", statusOff],
  ["obs otel enable --port 4318 --dry-run", enableData(true, 4318)],
  ["obs otel enable --port 4318", enableData(false, 4318)],
  ["obs otel disable --dry-run", disableData(true, 4318)],
  ["obs otel disable", disableData(false, 4318)],
];
