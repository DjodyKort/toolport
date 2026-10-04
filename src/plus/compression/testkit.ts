import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { CtlResult } from "../bridge/ctl";
import { CtlReplyFailure } from "../fixtures/ctlReply";
import { goldenData, presetsData, statusData } from "./fixtures";
import { createCompressionWorld, type WorldState } from "./world";

const dir = join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes");

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

interface Call {
  job: string;
  argv: string[];
}

/** A fake `plus_ctl` bridge over the fixture world of the Compression tab. An argv without
 * a reply fails the test: a screen that runs a command it should not shows up as a missing
 * reply. A test overrides a reply with `set`, or makes it a function that changes the world.
 * With `world`, an argv without a row is answered by the stateful compression world (an applied
 * write changes the next read), and a row still wins so a test can force a failure. */
export function createBridge(options: { world?: boolean | Partial<WorldState> } = {}) {
  const registry = JSON.parse(readFileSync(join(dir, "commands.json"), "utf8")).envelope
    .data;
  const world = options.world
    ? createCompressionWorld(options.world === true ? {} : options.world)
    : null;
  const replies = new Map<string, Reply>([
    ["commands", registry],
    ...(world
      ? []
      : ([
          ["compression status", statusData()],
          ["compression presets", presetsData()],
          ["compression pin", goldenData("compression-pin")],
          ["compression doctor", goldenData("compression-doctor")],
          ["compression ledger summary", goldenData("compression-ledger-summary")],
          [
            "compression set-provider headroom --dry-run",
            goldenData("compression-set-provider.preview"),
          ],
          [
            "compression set-provider headroom",
            goldenData("compression-set-provider.apply"),
          ],
          ["compression use agent --dry-run", goldenData("compression-use.preview")],
          ["compression use agent", goldenData("compression-use.apply")],
        ] as Array<[string, Reply]>)),
  ]);
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
        const own = replies.has(key);
        const wanted = replies.get(key);
        const value = own
          ? typeof wanted === "function"
            ? await wanted(call.argv)
            : wanted
          : world?.reply(call.argv);
        if (!own && value === undefined) {
          missing.push(key);
          throw new Error(`no fake reply for plus_ctl ${key}`);
        }
        const failed =
          value instanceof Failure || value instanceof CtlReplyFailure ? value : null;
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
