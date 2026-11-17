import readline from "node:readline";

const instructions = process.env.FAKE_INSTRUCTIONS;
let initializeParams = null;

const send = (message) => process.stdout.write(`${JSON.stringify(message)}\n`);
const reply = (id, result) => send({ jsonrpc: "2.0", id, result });
const text = (value) => ({ content: [{ type: "text", text: value }] });
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const tools = [
  { name: "slow", description: "Takes a while, reporting progress", inputSchema: { type: "object" } },
  { name: "link", description: "Returns a link to a resource it does not list", inputSchema: { type: "object" } },
  { name: "echo_initialize", description: "Returns what initialize carried", inputSchema: { type: "object" } },
];

async function call(id, params) {
  const args = params.arguments ?? {};
  const token = params._meta?.progressToken;
  if (params.name === "slow") {
    const started = Date.now();
    while (Date.now() - started < (args.totalMs ?? 0)) {
      await wait(args.tickMs ?? 100);
      if (token !== undefined && args.tickMs) {
        send({ jsonrpc: "2.0", method: "notifications/progress", params: { progressToken: token, progress: Date.now() - started, message: "working" } });
      }
    }
    return reply(id, text("done"));
  }
  if (params.name === "link") {
    return reply(id, {
      content: [
        { type: "text", text: "see the link" },
        { type: "resource_link", uri: args.uri ?? "fake://dyn/42", name: "dynamic" },
      ],
    });
  }
  if (params.name === "echo_initialize") {
    return reply(id, text(JSON.stringify(initializeParams)));
  }
  return send({ jsonrpc: "2.0", id, error: { code: -32602, message: "unknown tool" } });
}

readline.createInterface({ input: process.stdin }).on("line", (line) => {
  const message = JSON.parse(line);
  if (message.id === undefined) return;
  switch (message.method) {
    case "initialize":
      initializeParams = message.params;
      return reply(message.id, {
        protocolVersion: "2025-06-18",
        capabilities: { tools: {}, resources: {} },
        serverInfo: { name: "fake-mcp", version: "0" },
        ...(instructions === undefined ? {} : { instructions }),
      });
    case "tools/list":
      return reply(message.id, { tools });
    case "resources/list":
      return reply(message.id, { resources: [] });
    case "resources/read":
      return reply(message.id, { contents: [{ uri: message.params.uri, text: `body of ${message.params.uri}` }] });
    case "tools/call":
      return void call(message.id, message.params);
    default:
      return send({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "method not found" } });
  }
});
