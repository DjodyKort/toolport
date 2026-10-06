import commands from "../../../src-tauri/tests/fixtures/ctl-envelopes/commands.json";
import type { AttentionItem, AttentionLsData } from "../types/attention";

/** Envelope `data` of `attention ls|dismiss`: the real golden envelopes of
 * `src-tauri/tests/fixtures/ctl-envelopes` for the two rows they hold, and rows of the other
 * feeds written the way the backend emits them (contract section 9, D-095). The names are made
 * up. Every `since` is relative to the moment the fixture is built, so the ages stay plausible. */

interface Golden {
  envelope: { ok: boolean; data?: unknown };
}

const files = import.meta.glob<Golden>(
  "../../../src-tauri/tests/fixtures/ctl-envelopes/attention-*.json",
  { eager: true, import: "default" },
);

export function goldenData<T>(stem: string): T {
  const file = Object.entries(files).find(([path]) => path.endsWith(`/${stem}.json`));
  if (!file) throw new Error(`no golden envelope ${stem}`);
  return file[1].envelope.data as T;
}

export const commandsData = (commands as { envelope: { data: unknown } }).envelope.data;

const MINUTE = 60_000;
const ago = (minutes: number, now: number) =>
  new Date(now - minutes * MINUTE).toISOString();

const golden = () => goldenData<AttentionLsData>("attention-ls.default").items;

/** Every kind of row the feeds emit, one per kind and level: two that need you (one with an
 * action), the rest worth a look or for your information. */
export function stockItems(now: number = Date.now()): AttentionItem[] {
  const items = golden();
  const secret = items.find((item) => item.id === "secrets:srv-alpha:missing")!;
  const waiting = items.find((item) => item.id === "tasks:portal-token:waiting")!;
  return [
    { ...secret, since: ago(95, now) },
    { ...waiting, since: ago(25, now) },
    {
      id: "auth:srv-beta",
      level: "needs-you",
      title: "beta needs a new sign-in",
      detail: "The login stopped working. Check it again, or sign in once more.",
      from: "auth",
      target: { route: "servers", params: { tab: "logins", server: "srv-beta" } },
      action: {
        label: "Check again",
        command: ["toolportctl", "auth", "probe", "--server", "srv-beta", "--force"],
      },
      since: ago(3 * 60 + 10, now),
    },
    {
      id: "tasks:nightly-report:failed",
      level: "look",
      title: "Task nightly-report failed",
      detail: "Its last run stopped at the step that sends the report.",
      from: "tasks",
      target: {
        route: "tasks",
        params: { tab: "tasks", task: "nightly-report" },
      },
      action: {
        label: "Run again",
        command: ["toolportctl", "task", "run", "nightly-report"],
      },
      since: ago(26 * 60, now),
    },
    {
      id: "skills:invisible",
      level: "look",
      title: "2 skills are installed but Claude cannot see them",
      detail: "They sit in a folder that no profile loads.",
      from: "skills",
      target: { route: "library", params: { tab: "skills", filter: "invisible" } },
      action: null,
      since: ago(2 * 24 * 60, now),
    },
    {
      id: "source:acme-skills:behind",
      level: "look",
      title: "acme-skills is 12 changes behind",
      detail: "The checkout has not been updated since the source changed.",
      from: "sources",
      target: { route: "library", params: { tab: "sources", source: "acme-skills" } },
      action: null,
      since: ago(3 * 24 * 60, now),
    },
    {
      id: "bundle:acme-dev:ab12cd34",
      level: "look",
      title: "Bundle acme-dev changed since it was applied",
      detail: "The folder still has the older version of the bundle.",
      from: "context",
      target: {
        route: "context",
        params: {
          tab: "profiles",
          bundle: "acme-dev",
          folder: "/home/demo/work/acme-erp",
        },
      },
      action: {
        label: "Apply again",
        command: [
          "toolportctl",
          "context",
          "bundle",
          "apply",
          "acme-dev",
          "--cwd",
          "/home/demo/work/acme-erp",
        ],
      },
      since: ago(5 * 60, now),
    },
    {
      id: "compression:drift",
      level: "look",
      title: "Compression presets differ from the installed engine",
      detail: "The saved values were taken from an older version.",
      from: "compression",
      target: { route: "tokens", params: { tab: "compression" } },
      action: {
        label: "Refresh presets",
        command: ["toolportctl", "compression", "presets", "--refresh"],
      },
      since: ago(4 * 24 * 60, now),
    },
    {
      id: "library:behind",
      level: "fyi",
      title: "Your skills library is behind its remote",
      detail: "A sync would bring in 3 changes.",
      from: "sync",
      target: { route: "system", params: { tab: "sync" } },
      action: null,
      since: ago(7 * 60, now),
    },
    {
      id: "hooks:bash",
      level: "fyi",
      title: "One hook runs before every Bash call",
      detail: "It is yours; this only tells you it exists.",
      from: "context",
      target: { route: "context", params: { tab: "hooks", tool: "Bash" } },
      action: null,
      since: ago(6 * 24 * 60, now),
    },
  ];
}

/** The rows with an action, and the argv after `toolportctl` that each runs. */
export const actionArgvs = (items: AttentionItem[]) =>
  items.flatMap((item) => (item.action ? [item.action.command.slice(1)] : []));

export const makeItem = (over: Partial<AttentionItem> = {}): AttentionItem => ({
  id: "doctor:odd",
  level: "look",
  title: "Odd row",
  detail: "Something to look at.",
  from: "doctor",
  target: { route: "servers", params: { tab: "logins" } },
  action: null,
  since: new Date(Date.now() - 10 * MINUTE).toISOString(),
  ...over,
});
