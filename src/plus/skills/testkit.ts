import { readFileSync } from "node:fs";
import { join } from "node:path";
import type { CtlResult } from "../bridge/ctl";
import {
  addData,
  cleanData,
  diffData,
  libraryRows,
  lintData,
  resolveData,
  skillsCtlFixtures,
  syncData,
  uninstallData,
} from "./fixtures";
import { tapsCtlFixtures } from "./fixturesTaps";
import { Failure, createSkillsWorld, type WorldOptions } from "./world";

const dir = join(__dirname, "../../../src-tauri/tests/fixtures/ctl-envelopes");

/** `data` of a real golden envelope, e.g. `goldenData("skills-sync.preview")`. */
export const goldenData = (stem: string) =>
  JSON.parse(readFileSync(join(dir, `${stem}.json`), "utf8")).envelope.data;

export type Reply = unknown | ((argv: string[]) => unknown | Promise<unknown>);

export { Failure };

export const failure = (code: string, message: string, data?: unknown) =>
  new Failure(code, message, data);

interface Call {
  job: string;
  argv: string[];
}

/** A fake `plus_ctl` bridge over the fixture world of the Skills tab. Every argv has a reply
 * and an argv without one fails the test: a screen that runs a command it should not shows up
 * as a missing reply. A test overrides a reply with `set`, or makes it a function that changes
 * the world (a write that edits what the next read returns). */
export function createBridge(options: { world?: boolean | WorldOptions } = {}) {
  const registry = JSON.parse(readFileSync(join(dir, "commands.json"), "utf8")).envelope
    .data;
  const stateful = options.world
    ? new Map<string, Reply>([
        ...createSkillsWorld(options.world === true ? {} : options.world),
        ["commands", registry],
      ])
    : null;
  const replies =
    stateful ??
    new Map<string, Reply>([
      ...skillsCtlFixtures,
      ...tapsCtlFixtures,
      ["commands", registry],
      ["skills clean --dry-run", cleanData(true)],
      ["skills clean", cleanData(false)],
      ["skills resolve --migrate --dry-run", resolveData(true, true)],
      ["skills resolve --migrate", resolveData(true, false)],
      ["skills sync --client claude-code --dry-run", syncData(["claude-code"], true)],
      ["skills sync --client claude-code", syncData(["claude-code"], false)],
      ["skills add reviewer --type skill --dry-run", addData("reviewer", "skill", true)],
      ["skills add reviewer --type skill", addData("reviewer", "skill", false)],
      ...["api-review", "deploy-helper"].flatMap((name): Array<[string, unknown]> => [
        [`skills uninstall ${name} --dry-run`, uninstallData(name, true)],
        [`skills uninstall ${name}`, uninstallData(name, false)],
      ]),
      ["skills diff", new Failure("unhealthy", "one or more checks failed", diffData)],
      ...libraryRows.map((row): [string, unknown] => [
        `skills lint --name ${row.name}`,
        {
          ...lintData,
          messages: lintData.messages.filter((m) => m.name === row.name),
        },
      ]),
    ]);
  const calls: Call[] = [];
  const missing: string[] = [];
  const jobs = new Map<string, Call>();
  let count = 0;

  return {
    calls,
    missing,
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
        const failed = value instanceof Failure ? value : null;
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
