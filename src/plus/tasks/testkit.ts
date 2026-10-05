import type { CtlResult } from "../bridge/ctl";
import { CtlReplyFailure } from "../fixtures/ctlReply";
import { createTasksWorld, type TasksState } from "./world";

export type Reply = unknown | ((argv: string[]) => unknown | Promise<unknown>);

interface Call {
  job: string;
  argv: string[];
  stdin: string | null;
}

/** A fake `plus_ctl` bridge over the stateful Tasks world. A test overrides one argv with
 * `set` (a value, a failure, or a function that changes the world); every other argv is
 * answered by the world, and an argv the world does not know fails the test. */
export function createBridge(seed: Partial<TasksState> = {}) {
  const world = createTasksWorld(seed);
  const replies = new Map<string, Reply>();
  const calls: Call[] = [];
  const missing: string[] = [];
  const jobs = new Map<string, Call>();
  let count = 0;

  return {
    calls,
    missing,
    world,
    set(argv: string, reply: Reply) {
      replies.set(argv, reply);
    },
    ran: () => calls.map((call) => call.argv.join(" ")),
    count: (argv: string) => calls.filter((call) => call.argv.join(" ") === argv).length,
    stdin: (argv: string) =>
      calls.filter((call) => call.argv.join(" ") === argv).map((call) => call.stdin),
    async invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (command === "plus_ctl") {
        const call = {
          job: `job-${++count}`,
          argv: args.argv as string[],
          stdin: (args.stdinSecret as string | null) ?? null,
        };
        calls.push(call);
        jobs.set(call.job, call);
        return call.job;
      }
      if (command === "plus_ctl_result") {
        const call = jobs.get(args.job as string);
        if (!call) throw new Error(`unknown job ${String(args.job)}`);
        const key = call.argv.join(" ");
        const wanted = replies.get(key);
        const value = replies.has(key)
          ? typeof wanted === "function"
            ? await wanted(call.argv)
            : wanted
          : world.reply(call.argv, call.stdin);
        if (value === undefined) {
          missing.push(key);
          throw new Error(`no fake reply for plus_ctl ${key}`);
        }
        const failed = value instanceof CtlReplyFailure ? value : null;
        return {
          job: call.job,
          exitCode: failed ? 1 : 0,
          signal: null,
          cancelled: false,
          parseError: null,
          stderr: [],
          truncated: false,
          envelope: failed
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
      if (command === "plus_ctl_cancel") return null;
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
