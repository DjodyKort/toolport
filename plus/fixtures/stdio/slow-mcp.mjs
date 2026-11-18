import fs from "node:fs";
import process from "node:process";
import readline from "node:readline";
import { clearTimeout, setTimeout } from "node:timers";

const logPath = process.env.SLOWMCP_LOG;
const serial = process.env.SLOWMCP_MODE === "serial";
const ignoreCancel = process.env.SLOWMCP_CANCEL === "ignore";

const record = (entry) => {
  if (logPath)
    fs.appendFileSync(logPath, `${JSON.stringify({ t: Date.now(), ...entry })}\n`);
};
const send = (message) => {
  record({ dir: "out", id: message.id, method: message.method, error: message.error });
  process.stdout.write(`${JSON.stringify(message)}\n`);
};
const reply = (id, result) => send({ jsonrpc: "2.0", id, result });
const text = (value) => ({ content: [{ type: "text", text: value }] });

const seconds = { type: "number", description: "How long the call takes" };
const tools = [
  {
    name: "sleep",
    description: "Sleeps, optionally reporting progress",
    inputSchema: {
      type: "object",
      properties: {
        seconds,
        progress_every_ms: { type: "number" },
        progress_for: { type: "string", enum: ["request", "unknown"] },
      },
    },
  },
  {
    name: "echo",
    description: "Answers at once",
    inputSchema: { type: "object", properties: { text: { type: "string" } } },
  },
  {
    name: "budgeted_sleep",
    description: "Sleeps; the listing carries an expected duration",
    inputSchema: {
      type: "object",
      properties: { seconds, progress_every_ms: { type: "number" } },
    },
    _meta: { "odh.dev/budget": 120 },
  },
];

tools.push({
  name: "emit_progress",
  description: "Reports progress under any token, then answers",
  inputSchema: {
    type: "object",
    properties: { token: {}, every_ms: { type: "number" }, count: { type: "number" } },
  },
});

const inFlight = new Map();
let queue = Promise.resolve();

const sleepCall = (id, params) =>
  new Promise((resolve) => {
    const args = params.arguments ?? {};
    const token =
      args.progress_for === "unknown" ? "never-issued" : params._meta?.progressToken;
    const started = Date.now();
    const total = Math.round((args.seconds ?? 0) * 1000);
    let ticker;
    let finish;
    const done = (result) => {
      clearTimeout(ticker);
      clearTimeout(finish);
      inFlight.delete(String(id));
      resolve();
      if (result) reply(id, result);
    };
    inFlight.set(String(id), { abort: () => done(null) });
    if (token !== undefined && args.progress_every_ms) {
      const tick = () => {
        send({
          jsonrpc: "2.0",
          method: "notifications/progress",
          params: {
            progressToken: token,
            progress: Date.now() - started,
            message: "working",
          },
        });
        ticker = setTimeout(tick, args.progress_every_ms);
      };
      ticker = setTimeout(tick, args.progress_every_ms);
    }
    finish = setTimeout(() => done(text(`slept ${args.seconds ?? 0} s`)), total);
  });

const emitCall = (id, params) =>
  new Promise((resolve) => {
    const args = params.arguments ?? {};
    let sent = 0;
    const tick = () => {
      if (sent >= (args.count ?? 1)) {
        reply(id, text(`emitted ${sent}`));
        resolve();
        return;
      }
      sent += 1;
      send({
        jsonrpc: "2.0",
        method: "notifications/progress",
        params: { progressToken: args.token, progress: sent, message: "working" },
      });
      setTimeout(tick, args.every_ms ?? 100);
    };
    tick();
  });

const call = (id, params) => {
  switch (params.name) {
    case "emit_progress":
      return emitCall(id, params);
    case "sleep":
    case "budgeted_sleep":
      return sleepCall(id, params);
    case "echo":
      reply(id, text(params.arguments?.text ?? "echo"));
      return Promise.resolve();
    default:
      send({ jsonrpc: "2.0", id, error: { code: -32602, message: "unknown tool" } });
      return Promise.resolve();
  }
};

const handle = (message) => {
  switch (message.method) {
    case "initialize":
      return reply(message.id, {
        protocolVersion: "2025-06-18",
        capabilities: { tools: {} },
        serverInfo: { name: "slow-mcp", version: "0" },
      });
    case "tools/list":
      return reply(message.id, { tools });
    case "tools/call": {
      const run = () => call(message.id, message.params);
      if (serial) {
        queue = queue.then(run);
        return queue;
      }
      return run();
    }
    default:
      return send({
        jsonrpc: "2.0",
        id: message.id,
        error: { code: -32601, message: "method not found" },
      });
  }
};

readline.createInterface({ input: process.stdin }).on("line", (line) => {
  const message = JSON.parse(line);
  record({
    dir: "in",
    id: message.id,
    method: message.method,
    tool: message.params?.name,
    token: message.params?._meta?.progressToken,
    requestId: message.params?.requestId,
    reason: message.params?.reason,
  });
  if (message.method === "notifications/cancelled") {
    if (!ignoreCancel) inFlight.get(String(message.params?.requestId))?.abort();
    return;
  }
  if (message.id === undefined) return;
  handle(message);
});
