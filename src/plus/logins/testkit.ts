import type { CtlResult } from "../bridge/ctl";
import golden from "../../../src-tauri/tests/fixtures/ctl-envelopes/commands.json";
import { CtlReplyFailure, CtlReplyHeld } from "../fixtures/ctlReply";
import { loginsCtlFixtures } from "../fixtures/logins";

export { ctlReplyFailure } from "../fixtures/ctlReply";

/** `toolportctl` could not be run or did not print an envelope. */
export class BridgeDown {
  constructor(readonly message: string) {}
}

export const bridgeDown = (message: string) => new BridgeDown(message);

export type Reply = unknown | ((argv: string[]) => unknown | Promise<unknown>);

export interface Call {
  job: string;
  argv: string[];
  stdin: string | null;
}

export const commandsGolden = (golden as { envelope: { data: unknown } }).envelope.data;

/** A fake `plus_ctl` bridge over the fixture world of the Logins & secrets screens: every argv
 * has a reply and an argv without one fails the test. A test overrides a reply with `set`, or
 * makes it a function that changes the world (a write that edits what the next read returns). */
export function createBridge() {
  const replies = new Map<string, Reply>([
    ...loginsCtlFixtures,
    ["commands", commandsGolden],
  ]);
  const calls: Call[] = [];
  const missing: string[] = [];
  const cancelled: string[] = [];
  const jobs = new Map<string, Call>();
  const released = new Map<string, (data: unknown) => void>();
  const cancelHeld = new Map<string, () => void>();
  let onEvent: ((event: { payload: unknown }) => void) | null = null;
  let count = 0;

  const bridge = {
    calls,
    missing,
    cancelled,
    set(argv: string, reply: Reply) {
      replies.set(argv, reply);
    },
    get: (argv: string) => replies.get(argv),
    /** Once `argv` has run, the other commands answer as `effects` say. */
    after(argv: string, effects: Record<string, Reply>) {
      const original = replies.get(argv);
      replies.set(argv, async (given: string[]) => {
        for (const [key, reply] of Object.entries(effects)) replies.set(key, reply);
        return typeof original === "function" ? original(given) : original;
      });
    },
    ran: () => calls.map((call) => call.argv.join(" ")),
    count: (argv: string) => calls.filter((call) => call.argv.join(" ") === argv).length,
    /** Lets a held run (a sign-in waiting for the browser) finish with its data. */
    release(argv: string) {
      const call = [...calls].reverse().find((c) => c.argv.join(" ") === argv);
      if (call) released.get(call.job)?.(undefined);
    },
    onEvent(callback: (event: { payload: unknown }) => void) {
      onEvent = callback;
    },
    async invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (command === "plus_ctl") {
        const call: Call = {
          job: `job-${++count}`,
          argv: args.argv as string[],
          stdin: (args.stdinSecret as string | null) ?? null,
        };
        calls.push(call);
        jobs.set(call.job, call);
        const reply = replies.get(call.argv.join(" "));
        if (reply instanceof CtlReplyHeld) {
          reply.lines.forEach((line, index) =>
            onEvent?.({
              payload: { job: call.job, seq: index + 1, kind: "stderr", line },
            }),
          );
        }
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
        const base = {
          job: call.job,
          signal: null,
          parseError: null,
          stderr: [],
          truncated: false,
        };
        if (wanted instanceof CtlReplyHeld) {
          const ended = await new Promise<"released" | "cancelled">((resolve) => {
            released.set(call.job, () => resolve("released"));
            cancelHeld.set(call.job, () => resolve("cancelled"));
            if (cancelled.includes(call.job)) resolve("cancelled");
          });
          if (ended === "cancelled") {
            return {
              ...base,
              exitCode: null,
              cancelled: true,
              envelope: null,
            } satisfies CtlResult;
          }
          return {
            ...base,
            exitCode: 0,
            cancelled: false,
            envelope: {
              ok: true,
              command: call.argv[0],
              schemaVersion: 1,
              data: wanted.data,
            },
          } satisfies CtlResult;
        }
        const value = typeof wanted === "function" ? await wanted(call.argv) : wanted;
        if (value instanceof BridgeDown) {
          return {
            ...base,
            parseError: value.message,
            exitCode: null,
            cancelled: false,
            envelope: null,
          } satisfies CtlResult;
        }
        const failure = value instanceof CtlReplyFailure ? value : null;
        return {
          ...base,
          exitCode: failure ? 1 : 0,
          cancelled: false,
          envelope: failure
            ? {
                ok: false,
                command: call.argv[0],
                schemaVersion: 1,
                data: failure.data,
                error: { code: failure.code, message: failure.message },
              }
            : { ok: true, command: call.argv[0], schemaVersion: 1, data: value },
        } satisfies CtlResult;
      }
      if (command === "plus_ctl_cancel") {
        cancelled.push(args.job as string);
        cancelHeld.get(args.job as string)?.();
        return null;
      }
      throw new Error(`unexpected invoke ${command}`);
    },
  };
  return bridge;
}

export type Bridge = ReturnType<typeof createBridge>;

/** A reply the test settles later, to look at a screen while a run is in progress. */
export function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
