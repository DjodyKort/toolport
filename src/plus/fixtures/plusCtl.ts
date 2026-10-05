import { emit } from "@tauri-apps/api/event";
import { PLUS_CTL_EVENT, type CtlEnvelope, type CtlResult } from "../bridge/ctl";
import { CtlReplyFailure, CtlReplyHeld } from "./ctlReply";
import { loginsCtlFixtures } from "./logins";
import {
  plusSourcesFixture,
  plusSourcesItemsFixture,
  plusSourcesRootFixture,
} from "./sources";
import { serversCtlFixtures } from "./servers";
import { serversToolsBrowserFixtures } from "../servers/browserFixtures";
import { skillsBrowserFixtures } from "../skills/browserFixtures";
import { compressionBrowserFixtures } from "../compression/browserFixtures";
import { usageCtlFixtures } from "../usage/browserFixtures";
import { createAgentsWorld } from "./agentsWorld";
import { contextBrowserFixtures } from "../context/browserFixtures";
import { systemBrowserFixtures } from "../system/browserFixtures";
import { tasksBrowserFixtures } from "../tasks/browserFixtures";
import { createMcpWorld } from "../agents/mcpWorld";
import { pluginsBrowserFixtures } from "../plugins/browserFixtures";
import { attentionBrowserFixtures } from "../attention/browserFixtures";

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
  ...serversCtlFixtures,
  ...serversToolsBrowserFixtures,
  // The two synthetic worlds share their servers; where both answer a command, Servers wins.
  ...loginsCtlFixtures.filter(([key]) => !serversCtlFixtures.has(key)),
  // The Usage tab reads and, for the OTel receiver, changes a small world of its own.
  ...usageCtlFixtures,
  // The Library screen drives a stateful skills world, so its commands win over the static Health fixtures.
  ...skillsBrowserFixtures,
  ...createAgentsWorld(),
  // Attention only previews the actions of its rows in the dev browser, so where a screen's own
  // world answers the same argv (task run, compression presets), that world wins.
  ...attentionBrowserFixtures,
  ...compressionBrowserFixtures,
  ...contextBrowserFixtures,
  ...systemBrowserFixtures,
  ...tasksBrowserFixtures,
  ...createMcpWorld(),
  // Plugins, hooks and the plugin card of System > Updates share one world; its cc rows win over the System ones.
  ...pluginsBrowserFixtures,
]);

const jobs = new Map<string, string>();
const stdins = new Map<string, string>();
const held = new Map<string, (result: CtlResult) => void>();
let counter = 0;

export function plusCtlStart(argv: string[], stdin?: string): string {
  const key = argv.join(" ");
  if (!plusCtlFixtures.has(key))
    throw new Error(`Unimplemented fixture command: plus_ctl ${key}`);
  const job = `fixture-job-${++counter}`;
  jobs.set(job, key);
  if (stdin !== undefined) stdins.set(job, stdin);
  const reply = plusCtlFixtures.get(key);
  if (reply instanceof CtlReplyHeld) {
    reply.lines.forEach((line, index) => {
      void emit(PLUS_CTL_EVENT, { job, seq: index + 1, kind: "stderr", line }).catch(
        () => {},
      );
    });
  }
  return job;
}

function resultFor(job: string, key: string, reply: unknown): CtlResult {
  const failure = reply instanceof CtlReplyFailure ? reply : null;
  const envelope: CtlEnvelope = failure
    ? {
        ok: false,
        command: key.split(" --")[0],
        schemaVersion: 1,
        data: failure.data,
        error: { code: failure.code, message: failure.message },
      }
    : {
        ok: true,
        command: key.split(" --")[0],
        schemaVersion: 1,
        data: reply instanceof CtlReplyHeld ? reply.data : reply,
      };
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

export function plusCtlResult(job: string): CtlResult {
  const key = jobs.get(job);
  if (key === undefined) throw new Error(`unknown job: ${job}`);
  jobs.delete(job);
  const stdin = stdins.get(job);
  stdins.delete(job);
  const reply = plusCtlFixtures.get(key);
  const value =
    typeof reply === "function"
      ? (reply as (argv: string[], stdin?: string) => unknown)(key.split(" "), stdin)
      : reply;
  return resultFor(job, key, value);
}

/** A run that waits (a sign-in waiting for the browser) answers only once it is cancelled. */
export function plusCtlHeld(job: string): Promise<CtlResult> | null {
  const key = jobs.get(job);
  if (key === undefined || !(plusCtlFixtures.get(key) instanceof CtlReplyHeld))
    return null;
  jobs.delete(job);
  return new Promise((resolve) => held.set(job, resolve));
}

export function plusCtlCancel(job: string): null {
  jobs.delete(job);
  held.get(job)?.({
    job,
    exitCode: null,
    signal: null,
    cancelled: true,
    envelope: null,
    parseError: null,
    stderr: [],
    truncated: false,
  });
  held.delete(job);
  return null;
}
