import type { CtlEnvelope, CtlResult } from "../bridge/ctl";
import {
  plusSourcesFixture,
  plusSourcesItemsFixture,
  plusSourcesRootFixture,
} from "./sources";
import { commandsFixture } from "./commandsRegistry";

/** Envelope `data` the dev browser fixture returns per `toolportctl` argv (joined with spaces).
 * A command a screen runs needs a row here or the fixture rejects it as unimplemented. */
export const plusCtlFixtures = new Map<string, unknown>([
  [
    "status",
    {
      version: "0.0.0-fixture",
      dataDir: "/fixture/data",
      serverCount: 3,
      profileCount: 1,
      activeProfile: "local",
      secretsBackend: "encrypted-file",
    },
  ],
  ["sources ls", plusSourcesFixture],
  ["sources ls --items", plusSourcesItemsFixture],
  ["sources root ls", plusSourcesRootFixture],
  ["commands", commandsFixture],
  ["attention ls", { counts: { needsYou: 3, worthALook: 2 }, items: [] }],
]);

const jobs = new Map<string, string>();
let counter = 0;

export function plusCtlStart(argv: string[]): string {
  const key = argv.join(" ");
  if (!plusCtlFixtures.has(key))
    throw new Error(`Unimplemented fixture command: plus_ctl ${key}`);
  const job = `fixture-job-${++counter}`;
  jobs.set(job, key);
  return job;
}

export function plusCtlResult(job: string): CtlResult {
  const key = jobs.get(job);
  if (key === undefined) throw new Error(`unknown job: ${job}`);
  jobs.delete(job);
  const envelope: CtlEnvelope = {
    ok: true,
    command: key.split(" --")[0],
    schemaVersion: 1,
    data: plusCtlFixtures.get(key),
  };
  return {
    job,
    exitCode: 0,
    signal: null,
    cancelled: false,
    envelope,
    parseError: null,
    stderr: [],
    truncated: false,
  };
}

export function plusCtlCancel(job: string): null {
  jobs.delete(job);
  return null;
}
