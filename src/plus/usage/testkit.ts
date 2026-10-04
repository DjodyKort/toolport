import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { CtlResult } from "../bridge/ctl";
import { statusOff, usageWorld } from "./world";

const dir = join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes");

export interface Golden {
  argv: string[];
  envelope: { command: string; data: unknown };
  exitCode: number;
}

export const golden = (stem: string): Golden =>
  JSON.parse(readFileSync(join(dir, `${stem}.json`), "utf8")) as Golden;

export const goldenData = (stem: string): Record<string, unknown> =>
  structuredClone(golden(stem).envelope.data) as Record<string, unknown>;

export type Reply = unknown | ((argv: string[]) => unknown | Promise<unknown>);

/** A reply that is a failed envelope; `data` is what a command that exits 1 still prints. */
export class Failure {
  constructor(
    readonly code: string,
    readonly message: string,
    readonly data?: unknown,
  ) {}
}

export const failure = (code: string, message: string, data?: unknown) =>
  new Failure(code, message, data);

/** `toolportctl` could not be run or printed no envelope. */
export class BridgeDown {
  constructor(readonly message: string) {}
}

export const bridgeDown = (message: string) => new BridgeDown(message);

export function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

interface Call {
  job: string;
  argv: string[];
}

export const registry = () => goldenData("commands");

/** A fake `plus_ctl` bridge over the Usage tab's world. An argv without a reply fails the
 * test, so a screen that runs a command it should not shows up as a missing reply. */
export function createBridge() {
  const replies = new Map<string, Reply>([
    ["commands", registry()],
    ["usage", usageWorld()],
    ["usage --no-refresh", usageWorld()],
    ["obs otel status", statusOff],
    ["obs otel enable --port 4999 --dry-run", goldenData("obs-otel-enable.preview")],
    ["obs otel enable --port 4999", goldenData("obs-otel-enable.apply")],
    ["obs otel disable --dry-run", goldenData("obs-otel-disable.preview")],
    ["obs otel disable", goldenData("obs-otel-disable.apply")],
  ]);
  const calls: Call[] = [];
  const missing: string[] = [];
  const cancelled: string[] = [];
  const jobs = new Map<string, Call>();
  let count = 0;

  return {
    calls,
    missing,
    cancelled,
    set(argv: string, reply: Reply) {
      replies.set(argv, reply);
    },
    ran: () => calls.map((call) => call.argv.join(" ")),
    count: (argv: string) => calls.filter((call) => call.argv.join(" ") === argv).length,
    async invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (command === "plus_ctl") {
        const call = { job: `job-${++count}`, argv: args.argv as string[] };
        calls.push(call);
        jobs.set(call.job, call);
        return call.job;
      }
      if (command === "plus_ctl_result") {
        const call = jobs.get(args.job as string);
        if (!call) throw new Error(`unknown job ${String(args.job)}`);
        const key = call.argv.join(" ");
        if (!replies.has(key)) {
          missing.push(key);
          throw new Error(`no fake reply for plus_ctl ${key}`);
        }
        const wanted = replies.get(key);
        const value = typeof wanted === "function" ? await wanted(call.argv) : wanted;
        if (value instanceof BridgeDown) {
          return {
            job: call.job,
            exitCode: null,
            signal: null,
            cancelled: false,
            envelope: null,
            parseError: value.message,
            stderr: [],
            truncated: false,
          } satisfies CtlResult;
        }
        const failed = value instanceof Failure ? value : null;
        const wasCancelled = cancelled.includes(call.job);
        return {
          job: call.job,
          exitCode: wasCancelled ? null : failed ? 1 : 0,
          signal: null,
          cancelled: wasCancelled,
          parseError: null,
          stderr: [],
          truncated: false,
          envelope: wasCancelled
            ? null
            : failed
              ? {
                  ok: false,
                  command: call.argv[0],
                  schemaVersion: 1,
                  data: failed.data,
                  error: { code: failed.code, message: failed.message },
                }
              : { ok: true, command: call.argv[0], schemaVersion: 1, data: value },
        } satisfies CtlResult;
      }
      if (command === "plus_ctl_cancel") {
        cancelled.push(args.job as string);
        return null;
      }
      throw new Error(`unexpected invoke ${command}`);
    },
  };
}

export type Bridge = ReturnType<typeof createBridge>;

type Mocks = {
  invoke: ReturnType<typeof import("vitest").vi.fn>;
  listen: ReturnType<typeof import("vitest").vi.fn>;
};

export function wire(mocks: Mocks, bridge: Bridge) {
  mocks.listen.mockReset().mockResolvedValue(() => {});
  mocks.invoke.mockReset().mockImplementation(bridge.invoke);
}

/** The registry with one row's tier changed, e.g. to see a destructive tier asked for. */
export function registryWithTier(id: string, tier: "read" | "write" | "destructive") {
  const data = registry() as { commands: Array<{ id: string; tier: string | null }> };
  for (const row of data.commands) if (row.id === id) row.tier = tier;
  return data;
}
