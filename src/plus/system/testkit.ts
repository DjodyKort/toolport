import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { CtlResult } from "../bridge/ctl";
import { CtlReplyFailure } from "../fixtures/ctlReply";
import { commandsData, systemCtlFixtures } from "./fixtures";
import { createSystemWorld, type SystemState } from "./world";

export type Reply = unknown | ((argv: string[]) => unknown | Promise<unknown>);

export const failure = (code: string, message: string, data?: unknown) =>
  new CtlReplyFailure(code, message, data);

interface Call {
  job: string;
  argv: string[];
  stdinSecret: string | null;
}

/** A fake `plus_ctl` bridge over the fixture world of the System screen. Every argv has a
 * reply and an argv without one fails the test, so a screen that runs a command it should not
 * shows up as a missing reply. A test overrides a reply with `set`, or makes it a function
 * that changes the world. `stdin` records what each run was given on its standard input.
 * With `world`, an argv without a row is answered by the stateful System world (an applied
 * write changes the next read), and a row still wins so a test can force a failure. */
export function createBridge(options: { world?: boolean | Partial<SystemState> } = {}) {
  const world = options.world
    ? createSystemWorld(options.world === true ? {} : options.world)
    : null;
  const replies = new Map<string, Reply>(
    world ? [["commands", commandsData]] : systemCtlFixtures,
  );
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
      calls
        .filter((call) => call.argv.join(" ") === argv)
        .map((call) => call.stdinSecret),
    async invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (command === "plus_ctl") {
        const call = {
          job: `job-${++count}`,
          argv: args.argv as string[],
          stdinSecret: (args.stdinSecret as string | null) ?? null,
        };
        calls.push(call);
        jobs.set(call.job, call);
        return call.job;
      }
      if (command === "plus_ctl_result") {
        const call = jobs.get(args.job as string);
        if (!call) throw new Error(`unknown job ${String(args.job)}`);
        const key = call.argv.join(" ");
        const own = replies.has(key);
        const wanted = replies.get(key);
        const value = own
          ? typeof wanted === "function"
            ? await wanted(call.argv)
            : wanted
          : world?.reply(call.argv, call.stdinSecret);
        if (!own && value === undefined) {
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

const dir = join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes");

/** `data` of a real golden envelope, e.g. `golden("sync-push.preview")`. */
export const golden = (stem: string) =>
  JSON.parse(readFileSync(join(dir, `${stem}.json`), "utf8")).envelope.data;

type Mocks = {
  invoke: ReturnType<typeof import("vitest").vi.fn>;
  listen: ReturnType<typeof import("vitest").vi.fn>;
};

export function wire(mocks: Mocks, bridge: Bridge) {
  mocks.listen.mockReset().mockResolvedValue(() => {});
  mocks.invoke.mockReset().mockImplementation(bridge.invoke);
}
