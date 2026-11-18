import fs from "node:fs";
import process from "node:process";
import readline from "node:readline";
import { clearTimeout, setTimeout } from "node:timers";

const logPath = process.env.SLOWMCP_LOG;
const serial = process.env.SLOWMCP_MODE === "serial";
const ignoreCancel = process.env.SLOWMCP_CANCEL === "ignore";
const tasksOn = process.env.SLOWMCP_TASKS === "1";
const taskPageSize = Number(process.env.SLOWMCP_TASK_PAGE ?? 100);
const jobListingBudget = Number(process.env.SLOWMCP_JOB_BUDGET ?? 0);

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

tools.push(
  {
    name: "odoo_job_wait",
    description: "Waits for a job that ends after job_s seconds, up to timeout_s",
    inputSchema: {
      type: "object",
      properties: {
        timeout_s: { type: "number" },
        job_s: { type: "number" },
        progress_every_ms: { type: "number" },
      },
    },
    ...(jobListingBudget ? { _meta: { "odh.dev/budget": jobListingBudget } } : {}),
  },
  {
    name: "odoo_status",
    description: "Answers at once",
    inputSchema: { type: "object", properties: {} },
  },
);

if (tasksOn) {
  tools.push(
    {
      name: "reset_job",
      description: "A destructive job that can run as a task",
      inputSchema: { type: "object", properties: { seconds } },
      annotations: { destructiveHint: true },
      execution: { taskSupport: "optional" },
    },
    {
      name: "slow_job",
      description: "A job that can run as a task",
      inputSchema: {
        type: "object",
        properties: {
          seconds,
          fail: { type: "boolean" },
          progress_every_ms: { type: "number" },
        },
      },
      execution: { taskSupport: "optional" },
    },
    {
      name: "task_only",
      description: "Only runs as a task",
      inputSchema: { type: "object", properties: { seconds } },
      execution: { taskSupport: "required" },
    },
  );
}

const RELATED_TASK = "io.modelcontextprotocol/related-task";
const tasks = new Map();
let taskSeq = 0;
const rpcError = (id, code, message) =>
  send({ jsonrpc: "2.0", id, error: { code, message } });
const terminal = (task) => ["completed", "failed", "cancelled"].includes(task.status);
const taskView = (entry) => entry.task;

const settle = (entry, status, outcome, statusMessage) => {
  if (terminal(entry.task)) return;
  clearTimeout(entry.timer);
  entry.task = {
    ...entry.task,
    status,
    statusMessage,
    lastUpdatedAt: new Date().toISOString(),
  };
  entry.outcome = outcome;
  send({ jsonrpc: "2.0", method: "notifications/tasks/status", params: entry.task });
  for (const waiter of entry.waiters.splice(0)) answerResult(waiter, entry);
};

const answerResult = (waiterId, entry) => {
  inFlight.delete(String(waiterId));
  if (entry.outcome.error) {
    send({
      jsonrpc: "2.0",
      id: waiterId,
      error: {
        ...entry.outcome.error,
        data: { _meta: { [RELATED_TASK]: { taskId: entry.task.taskId } } },
      },
    });
    return;
  }
  reply(waiterId, {
    ...entry.outcome.result,
    _meta: { [RELATED_TASK]: { taskId: entry.task.taskId } },
  });
};

const createTask = (id, params) => {
  const args = params.arguments ?? {};
  taskSeq += 1;
  const now = new Date().toISOString();
  const entry = {
    task: {
      taskId: `task-${taskSeq}`,
      status: "working",
      statusMessage: "started",
      createdAt: now,
      lastUpdatedAt: now,
      ttl: params.task.ttl ?? 60000,
      pollInterval: 100,
    },
    waiters: [],
    outcome: null,
  };
  tasks.set(entry.task.taskId, entry);
  const total = Math.round((args.seconds ?? 0) * 1000);
  entry.timer = setTimeout(() => {
    if (args.fail)
      settle(entry, "failed", { error: { code: -32603, message: "job failed" } });
    else
      settle(entry, "completed", {
        result: text(`job ${entry.task.taskId} done after ${args.seconds ?? 0} s`),
      });
  }, total);
  const token = params._meta?.progressToken;
  if (token !== undefined && args.progress_every_ms) {
    const tick = () => {
      if (terminal(entry.task)) return;
      send({
        jsonrpc: "2.0",
        method: "notifications/progress",
        params: {
          progressToken: token,
          progress: Date.now(),
          _meta: { [RELATED_TASK]: { taskId: entry.task.taskId } },
        },
      });
      setTimeout(tick, args.progress_every_ms);
    };
    setTimeout(tick, args.progress_every_ms);
  }
  reply(id, { task: taskView(entry) });
};

const taskMethod = (message) => {
  const { id, method, params } = message;
  if (method === "tasks/list") {
    const all = [...tasks.values()].map(taskView);
    const start = params?.cursor === undefined ? 0 : Number(params.cursor);
    const page = all.slice(start, start + taskPageSize);
    const next =
      start + taskPageSize < all.length ? String(start + taskPageSize) : undefined;
    return reply(id, { tasks: page, ...(next ? { nextCursor: next } : {}) });
  }
  const entry = tasks.get(params?.taskId);
  if (!entry) return rpcError(id, -32602, "Failed to retrieve task: not found");
  if (method === "tasks/get") return reply(id, taskView(entry));
  if (method === "tasks/cancel") {
    if (terminal(entry.task))
      return rpcError(
        id,
        -32602,
        `Cannot cancel task: already in terminal status '${entry.task.status}'`,
      );
    settle(entry, "cancelled", { error: { code: -32603, message: "task cancelled" } });
    return reply(id, taskView(entry));
  }
  if (terminal(entry.task)) return answerResult(id, entry);
  entry.waiters.push(id);
  inFlight.set(String(id), {
    abort: () => {
      entry.waiters = entry.waiters.filter((waiter) => waiter !== id);
      inFlight.delete(String(id));
    },
  });
};

const inFlight = new Map();
let queue = Promise.resolve();

const sleepCall = (id, params, outcome) =>
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
    finish = setTimeout(
      () => done(text(outcome ?? `slept ${args.seconds ?? 0} s`)),
      total,
    );
  });

const jobWaitCall = (id, params) => {
  const args = params.arguments ?? {};
  const job = args.job_s ?? 0;
  const limit = args.timeout_s ?? job;
  const finished = job <= limit;
  return sleepCall(
    id,
    {
      ...params,
      arguments: {
        seconds: Math.min(job, limit),
        progress_every_ms: args.progress_every_ms ?? 500,
      },
    },
    finished ? `job finished after ${job} s` : `job still running after ${limit} s`,
  );
};

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
    case "odoo_job_wait":
      return jobWaitCall(id, params);
    case "echo":
      reply(id, text(params.arguments?.text ?? "echo"));
      return Promise.resolve();
    case "odoo_status":
      reply(id, text("odoo is up"));
      return Promise.resolve();
    case "slow_job":
    case "task_only":
    case "reset_job":
      return sleepCall(id, params);
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
        capabilities: tasksOn
          ? {
              tools: {},
              tasks: { list: {}, cancel: {}, requests: { tools: { call: {} } } },
            }
          : { tools: {} },
        serverInfo: { name: "slow-mcp", version: "0" },
      });
    case "tools/list":
      return reply(message.id, { tools });
    case "tasks/get":
    case "tasks/result":
    case "tasks/cancel":
    case "tasks/list":
      if (tasksOn) return taskMethod(message);
      return rpcError(message.id, -32601, "method not found");
    case "tools/call": {
      const required = tasksOn && message.params.name === "task_only";
      if (required && !message.params.task)
        return rpcError(message.id, -32601, "Task augmentation required for tools/call");
      if (
        tasksOn &&
        message.params.task &&
        ["slow_job", "task_only", "reset_job"].includes(message.params.name)
      )
        return createTask(message.id, message.params);
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

record({ dir: "start", pid: process.pid });

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
    task: message.params?.task,
    taskId: message.params?.taskId,
    cursor: message.params?.cursor,
  });
  if (message.method === "notifications/cancelled") {
    if (!ignoreCancel) inFlight.get(String(message.params?.requestId))?.abort();
    return;
  }
  if (message.id === undefined) return;
  handle(message);
});
