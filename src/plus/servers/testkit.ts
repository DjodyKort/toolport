import type { CtlResult } from "../bridge/ctl";
import { CtlReplyFailure } from "../fixtures/ctlReply";
import { ctlFailure } from "../fixtures/servers";
import { plusCtlFixtures } from "../fixtures/plusCtl";

export { ctlFailure };

/** `toolportctl` could not be run or did not print an envelope. */
export class BridgeDown {
  constructor(readonly message: string) {}
}

export const bridgeDown = (message: string) => new BridgeDown(message);

export type Reply =
  unknown | ((argv: string[], call: Call) => unknown | Promise<unknown>);

export interface Call {
  job: string;
  argv: string[];
  stdin: string | null;
}

/** A fake `plus_ctl` bridge over the fixture world: every argv has a reply, an argv without
 * one is a failure of the test. Tests override a reply with `set`, or make it a function that
 * changes the world (a write that edits what the next read returns). */
export function createBridge() {
  const replies = new Map<string, Reply>(plusCtlFixtures);
  const calls: Call[] = [];
  const missing: string[] = [];
  const cancelled: string[] = [];
  const jobs = new Map<string, Call>();
  let count = 0;

  const bridge = {
    calls,
    missing,
    cancelled,
    set(argv: string, reply: Reply) {
      replies.set(argv, reply);
    },
    get: (argv: string) => replies.get(argv),
    /** Once `argv` has run, the other commands answer as `effects` say: a write that changes
     * what the next read returns. */
    after(argv: string, effects: Record<string, Reply>) {
      const original = replies.get(argv);
      replies.set(argv, async (given: string[]) => {
        for (const [key, reply] of Object.entries(effects)) replies.set(key, reply);
        return typeof original === "function" ? original(given) : original;
      });
    },
    /** The argv of every run, joined with spaces. */
    ran: () => calls.map((call) => call.argv.join(" ")),
    count: (argv: string) => calls.filter((call) => call.argv.join(" ") === argv).length,
    async invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (command === "plus_ctl") {
        const call: Call = {
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
        if (!replies.has(key)) {
          missing.push(key);
          throw new Error(`no fake reply for plus_ctl ${key}`);
        }
        const wanted = replies.get(key);
        const value =
          typeof wanted === "function" ? await wanted(call.argv, call) : wanted;
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
        const failure = value instanceof CtlReplyFailure ? value : null;
        const wasCancelled = cancelled.includes(call.job);
        return {
          job: call.job,
          exitCode: wasCancelled ? null : failure ? 1 : 0,
          signal: null,
          cancelled: wasCancelled,
          envelope: wasCancelled
            ? null
            : failure
              ? {
                  ok: false,
                  command: call.argv[0],
                  schemaVersion: 1,
                  data: failure.data,
                  error: { code: failure.code, message: failure.message },
                }
              : { ok: true, command: call.argv[0], schemaVersion: 1, data: value },
          parseError: null,
          stderr: [],
          truncated: false,
        } satisfies CtlResult;
      }
      if (command === "plus_ctl_cancel") {
        cancelled.push(args.job as string);
        return null;
      }
      throw new Error(`unexpected invoke ${command}`);
    },
  };
  return bridge;
}

export type Bridge = ReturnType<typeof createBridge>;

export const clone = <T>(value: T): T => structuredClone(value);

/** A reply the test settles later, to look at a screen while a run is in progress. */
export function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
