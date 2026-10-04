import type { CtlEnvelope, CtlResult } from "../bridge/ctl";
import {
  plusSourcesFixture,
  plusSourcesItemsFixture,
  plusSourcesRootFixture,
} from "./sources";
import { FixtureFailure, serversCtlFixtures } from "./servers";

/** Envelope `data` the dev browser fixture returns per `toolportctl` argv (joined with spaces).
 * A command a screen runs needs a row here or the fixture rejects it as unimplemented. */
const uninstallPlan = {
  dryRun: true,
  plan: {
    summary: "Remove acme-erp and the entries that point at it",
    steps: [
      {
        op: "delete",
        path: "registry.json",
        detail: "Remove the server acme-erp from the registry",
        keys: ["servers.acme-erp"],
        diff: { before: '"acme-erp": { "command": "acme-erp-mcp" }', after: "" },
      },
      {
        op: "update",
        path: "~/.config/client-a/mcp.json",
        detail: "Remove the acme-erp entry from client-a",
        keys: ["mcpServers.acme-erp"],
      },
      { op: "note", detail: "Its secrets stay in the vault (--keep-secrets)" },
    ],
    effects: {
      tokens: { before: 5200, after: 3900, basis: "estimated from tool definitions" },
    },
    warnings: ["client-a is running and reads its config only at start"],
    undo: "toolportctl server new acme-erp --command acme-erp-mcp",
  },
};

export const plusCtlFixtures = new Map<string, unknown>([
  ["sources ls", plusSourcesFixture],
  ["sources ls --items", plusSourcesItemsFixture],
  ["sources root ls", plusSourcesRootFixture],
  ["server uninstall acme-erp --dry-run", uninstallPlan],
  ["attention ls", { counts: { needsYou: 3, look: 2, fyi: 0 }, items: [] }],
  ...serversCtlFixtures,
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
  const reply = plusCtlFixtures.get(key);
  const failure = reply instanceof FixtureFailure ? reply : null;
  const envelope: CtlEnvelope = failure
    ? {
        ok: false,
        command: key.split(" --")[0],
        schemaVersion: 1,
        data: failure.data,
        error: { code: failure.code, message: failure.message },
      }
    : { ok: true, command: key.split(" --")[0], schemaVersion: 1, data: reply };
  return {
    job,
    exitCode: failure ? 1 : 0,
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
