import { CtlReplyFailure } from "../fixtures/ctlReply";
import type { PlanV1 } from "../ui";
import type { AttentionItem, AttentionLsData } from "../types/attention";
import { commandsData, stockItems } from "./fixtures";

/** A made-up secret that must never reach an argv or the page: a row's detail may carry it
 * (a feed that got something wrong), and the tests look for it everywhere else. */
export const CANARY = "canary-token-0123456789-do-not-show";

export interface AttentionState {
  items: AttentionItem[];
  /** Hidden rows by id: the date they return, or `null` for good. */
  hidden: Record<string, string | null>;
  /** Argv (joined) of the actions that ran for real. */
  ran: string[];
}

const fail = (code: string, message: string) => new CtlReplyFailure(code, message);
const FILE = "/data/plus/attention.json";

const pad = (n: number) => String(n).padStart(2, "0");

const today = () => {
  const now = new Date();
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
};

function plan(summary: string, detail: string, undo: string): PlanV1 {
  return {
    summary,
    steps: [{ op: "note", detail }],
    effects: {},
    warnings: [],
    undo,
  };
}

const result = (changed: string[], undo: string) => ({
  applied: true,
  changed,
  undo,
  backups: [],
});

/** A stateful stand-in for `toolportctl` as the Attention screen sees it. A dismissal hides the
 * row from the next `attention ls`, and an action that ran for real removes the row it solves,
 * so a test or the smoke walk sees the list change the way the real one does. */
export function createAttentionWorld(seed: Partial<AttentionState> = {}) {
  const state: AttentionState = {
    items: stockItems(),
    hidden: {},
    ran: [],
    ...seed,
  };

  const visible = () =>
    state.items.filter((item) => {
      if (!(item.id in state.hidden)) return true;
      const until = state.hidden[item.id];
      return until !== null && until <= today();
    });

  function list(level: string | undefined): AttentionLsData {
    const rows = visible();
    const count = (wanted: string) => rows.filter((row) => row.level === wanted).length;
    return {
      counts: { needsYou: count("needs-you"), look: count("look"), fyi: count("fyi") },
      items: rows.filter((row) => !level || row.level === level),
    };
  }

  function dismiss(argv: string[], dry: boolean): unknown {
    const id = argv[2];
    if (!id || id.startsWith("--"))
      return fail("usage", "attention dismiss needs a row id");
    const at = argv.indexOf("--until");
    const until = at >= 0 ? argv[at + 1] : null;
    if (until !== null && !/^\d{4}-\d{2}-\d{2}$/.test(until ?? ""))
      return fail(
        "invalid_input",
        `--until needs a date like 2026-10-06, got "${until}"`,
      );
    const undo = `toolportctl attention dismiss ${id} --until ${today()}`;
    const summary = until ? `Hide ${id} until ${until}` : `Hide ${id} for good`;
    if (dry)
      return {
        dryRun: true,
        id,
        until,
        plan: plan(summary, `write the entry in ${FILE}`, undo),
      };
    state.hidden[id] = until;
    return { dryRun: false, id, until, result: result([FILE], undo) };
  }

  const solved = (argv: string[]) => {
    const line = argv.join(" ");
    const item = state.items.find(
      (row) => row.action && row.action.command.slice(1).join(" ") === line,
    );
    if (item) state.items = state.items.filter((row) => row !== item);
    state.ran.push(line);
  };

  function act(argv: string[], dry: boolean): unknown {
    const base = argv.filter((word) => word !== "--dry-run");
    const line = base.join(" ");
    const known = state.items.some(
      (row) => row.action && row.action.command.slice(1).join(" ") === line,
    );
    if (!known) return fail("not_found", `no row offers "${line}"`);
    if (dry)
      return {
        dryRun: true,
        plan: plan(`Run ${line}`, `toolportctl ${line}`, "toolportctl attention ls"),
      };
    solved(base);
    return { dryRun: false, result: result([`/data/plus/${base[0]}.json`], "") };
  }

  function reply(argv: string[]): unknown {
    const dry = argv.includes("--dry-run");
    const [cmd, verb] = argv;
    if (cmd === "commands") return commandsData;
    if (cmd === "attention" && verb === "ls") {
      const at = argv.indexOf("--level");
      return list(at >= 0 ? argv[at + 1] : undefined);
    }
    if (cmd === "attention" && verb === "dismiss") return dismiss(argv, dry);
    if (
      (cmd === "task" && (verb === "resume" || verb === "run")) ||
      (cmd === "auth" && verb === "probe") ||
      (cmd === "compression" && verb === "presets") ||
      (cmd === "context" && verb === "bundle")
    )
      return act(argv, dry);
    return undefined;
  }

  return { state, reply, list, visible };
}

export type AttentionWorld = ReturnType<typeof createAttentionWorld>;

/** A row whose detail carries the canary: a feed that leaked something it should not. */
export const leakyItem: AttentionItem = {
  id: "auth:srv-leaky",
  level: "needs-you",
  title: "leaky needs a new sign-in",
  detail: `The login stopped working (${CANARY}).`,
  from: "auth",
  target: { route: "servers", params: { tab: "logins", server: "srv-leaky" } },
  action: {
    label: "Check again",
    command: ["toolportctl", "auth", "probe", "--server", "srv-leaky", "--force"],
  },
  since: new Date(Date.now() - 60 * 60_000).toISOString(),
};
