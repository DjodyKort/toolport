import { CtlReplyFailure } from "../fixtures/ctlReply";
import commands from "../../../src-tauri/tests/fixtures/ctl-envelopes/commands.json";
import type { TaskDefinition, TaskLsData, TaskRun } from "../types/tasks";

/** Envelope `data` of the `task` commands: the real golden envelopes of
 * `src-tauri/tests/fixtures/ctl-envelopes` with their placeholders filled in. The names are
 * made up: a task that waits for a sign-in, a scheduled report and a draft. */

interface Golden {
  argv: string[];
  envelope: {
    ok: boolean;
    data?: unknown;
    error?: { code: string; message: string };
  };
}

export const STAMP = "2026-10-04T08:00:00Z";

const files = import.meta.glob<Golden>(
  "../../../src-tauri/tests/fixtures/ctl-envelopes/task-*.json",
  { eager: true, import: "default" },
);

const byStem = new Map(
  Object.entries(files).map(([path, file]) => [
    path.replace(/^.*\/(.*)\.json$/, "$1"),
    file,
  ]),
);

function fill<T>(value: T): T {
  return JSON.parse(
    JSON.stringify(value)
      .replace(/<TIME>/g, STAMP)
      .replace(/<WORLD>/g, "/data")
      .replace(/<RUN>/g, "000")
      .replace(/\{home\}/g, "/home/demo"),
  ) as T;
}

export function goldenFile(stem: string): Golden {
  const file = byStem.get(stem);
  if (!file) throw new Error(`no golden envelope ${stem}`);
  return file;
}

export const goldenData = <T = unknown>(stem: string): T =>
  fill(goldenFile(stem).envelope.data) as T;

/** What the fake bridge answers for a golden: its data, or the failure it recorded. */
export function goldenReply(stem: string): unknown {
  const { envelope } = goldenFile(stem);
  if (envelope.ok) return goldenData(stem);
  return new CtlReplyFailure(
    envelope.error?.code ?? "failed",
    envelope.error?.message ?? "failed",
    envelope.data,
  );
}

export const commandsData = (commands as { envelope: { data: unknown } }).envelope.data;

const base = (
  id: string,
  title: string,
  over: Partial<TaskDefinition>,
): TaskDefinition => ({
  id,
  title,
  description: "",
  enabled: true,
  requires: { servers: [], commands: [] },
  writesSecrets: [],
  steps: [],
  triggers: {
    manual: true,
    cli: false,
    selfMcp: { enabled: false, approval: "every-run" },
    schedule: null,
    onAuthFailure: [],
  },
  createdFrom: { kind: "manual" },
  ...over,
});

export const portalToken = goldenData<{ task: TaskDefinition }>("task-show.waiting").task;
export const nightlyReport = goldenData<{ task: TaskDefinition }>(
  "task-show.scheduled",
).task;
export const draftCleanup = base("draft-cleanup", "Clean up", {
  enabled: false,
  steps: [
    {
      id: "ask",
      title: "Ask Claude",
      type: "prompt",
      prompt: "Tidy the scratch folder",
      allowedTools: ["Read"],
    },
  ],
});

export const stockTasks: TaskDefinition[] = [portalToken, nightlyReport, draftCleanup];

export const stockRuns = (): TaskRun[] =>
  goldenData<{ runs: TaskRun[] }>("task-history.all").runs;

export const runLog = (): TaskRun => goldenData<{ run: TaskRun }>("task-history.run").run;

/** Two tasks that renew the login of a server of the Logins fixtures: one for a server that
 * is signed in, one for a server whose probe failed. */
const loginTask = (id: string, server: string, enabled = true) => ({
  id,
  title: `Renew the ${server} login`,
  enabled,
  kind: "login" as const,
  refreshes: [server],
  triggers: { ...stockTasks[0].triggers, onAuthFailure: [server] },
  lastRun: null,
  nextRun: null,
  waiting: false,
});

export const loginTasks: TaskLsData = {
  tasks: [loginTask("erp-token", "srv-erp"), loginTask("issues-token", "srv-issues")],
  invalid: [],
};

export const taskCtlFixtures: Array<[string, unknown]> = [
  ["task ls --all", goldenReply("task-ls.all")],
  ["task ls", goldenReply("task-ls.default")],
  ["task show portal-token", goldenReply("task-show.waiting")],
  ["task show nightly-report", goldenReply("task-show.scheduled")],
  ["task history --limit 50", goldenReply("task-history.all")],
  ["task history --run run-fixture-ok", goldenReply("task-history.run")],
];
