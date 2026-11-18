import type {
  AttentionCheckName,
  AttentionData,
  AttentionHiddenRow,
  AttentionRow,
  AttentionSettings,
} from "../bridge/data";
import type { CtlReply } from "./ctlWorld";

const MIN = 60_000;
const HOUR = 60 * MIN;

export type AttentionScenario =
  "busy" | "clear" | "unchecked" | "updates" | "server-update";

export interface AttentionWorldOptions {
  /** Without a scenario `attention ls` fails the way a build without toolportctl does, so the bell
   * stays out. */
  scenario?: AttentionScenario;
}

const iso = (at: number) => new Date(at).toISOString().replace(/\.\d+Z$/, "Z");

function busy(now: number): AttentionRow[] {
  return [
    {
      id: "tasks:waiting:run-export-notes",
      urgency: "needs-you",
      title: "Task export-notes is waiting for you",
      detail: "Stopped at step 3 of 4: confirm upload.",
      source: "Tasks",
      since: iso(now - 25 * MIN),
      action: { label: "Continue", screen: "tasks" },
    },
    {
      id: "credentials:secret:alpha-api",
      urgency: "needs-you",
      title: "alpha-api is missing a secret",
      detail: "API_KEY is not set, so the server cannot start.",
      source: "Credentials",
      action: { label: "Set secret", screen: "servers" },
    },
    {
      id: "credentials:login:beta-mail",
      urgency: "needs-you",
      title: "beta-mail needs a new sign-in",
      detail: "The login stopped working. Check it again, or sign in once more.",
      source: "Credentials",
      since: iso(now - 3 * HOUR),
      action: { label: "Sign in", screen: "servers" },
    },
    {
      id: "tasks:failed:nightly-sync",
      urgency: "worth-a-look",
      title: "Task nightly-sync failed",
      detail: "Step 2 of 3 (update-plugins) ended with an error.",
      source: "Tasks",
      since: iso(now - 6 * HOUR),
      action: { label: "Run again", screen: "tasks" },
    },
    {
      id: "library:behind",
      urgency: "worth-a-look",
      title: "The library is 2 commits behind its remote",
      detail: "A pull would fast-forward it.",
      source: "Library",
      action: { label: "Review", screen: "library" },
    },
  ];
}

function updates(now: number): AttentionRow[] {
  return [
    {
      id: "updates:tap:acme-skills:1a2b3c4d>5e6f7a8b",
      urgency: "worth-a-look",
      title: "The skill tap acme-skills has 3 new commits",
      detail: "Nothing is applied until you update it.",
      source: "Updates",
      since: iso(now - 2 * HOUR),
      action: { label: "Review", screen: "library" },
      skip: { label: "5e6f7a8b" },
    },
  ];
}

function serverUpdate(now: number): AttentionRow[] {
  return [
    {
      id: "updates:server:alpha-api:4f2a1c3a>9be07d2b",
      urgency: "worth-a-look",
      title: "alpha-api: 2 new commits on main",
      detail:
        "Nothing is applied until you review the update; it only fast-forwards main to origin/main.",
      source: "Updates",
      since: iso(now - HOUR),
      action: {
        label: "Review update",
        screen: "servers",
        command: ["toolportctl", "server", "repo", "update", "alpha-api", "--dry-run"],
      },
      skip: { label: "9be07d2b" },
    },
  ];
}

const CHECKS: AttentionCheckName[] = ["credentials", "updates", "tasks", "library"];
const NOTIFIES_BY_DEFAULT: AttentionCheckName[] = ["credentials", "tasks"];

const fail = (code: string, message: string): CtlReply => ({
  ok: false,
  error: { code, message },
});

function flagValue(argv: string[], flag: string): string | undefined {
  const at = argv.indexOf(flag);
  return at === -1 ? undefined : argv[at + 1];
}

/** The answers of `toolportctl attention` for the fixture app. Hiding, restoring and the switches
 * keep their state for the life of the world, like the real store file does. */
export function createAttentionWorld(options: AttentionWorldOptions = {}) {
  const hidden = new Map<string, AttentionHiddenRow>();
  const undo: Array<{ id: string; previous: AttentionHiddenRow | undefined }> = [];
  const settings: AttentionSettings = {
    checks: CHECKS.map((name) => ({ name, on: true })),
    notify: {
      enabled: false,
      categories: CHECKS.map((name) => ({
        name,
        on: NOTIFIES_BY_DEFAULT.includes(name),
      })),
    },
  };

  function rows(now: number): AttentionRow[] {
    if (options.scenario === "clear") return [];
    if (options.scenario === "server-update") return [...busy(now), ...serverUpdate(now)];
    return options.scenario === "updates" ? [...busy(now), ...updates(now)] : busy(now);
  }

  function ls(withHidden: boolean): CtlReply {
    const now = Date.now();
    const all = rows(now);
    const items = all.filter((row) => !hidden.has(row.id));
    const unchecked = options.scenario === "unchecked";
    const data: AttentionData = {
      checkedAt: iso(now - 2 * MIN),
      counts: {
        needsYou: items.filter((i) => i.urgency === "needs-you").length,
        worthALook: items.filter((i) => i.urgency === "worth-a-look").length,
      },
      checks: CHECKS.map((name) =>
        unchecked && name === "tasks"
          ? { name, ok: false, error: "the data directory could not be resolved" }
          : { name, ok: true },
      ),
      items,
      hiddenCount: hidden.size,
    };
    if (withHidden) data.hidden = [...hidden.values()];
    return { ok: true, data };
  }

  function hide(kind: "snooze" | "skip" | "dismiss", argv: string[]): CtlReply {
    const id = argv[2];
    const row = rows(Date.now()).find((r) => r.id === id);
    if (!row) return fail("not_found", `no row ${id}`);
    if (kind !== "snooze" && row.urgency === "needs-you") {
      return fail("usage", `${row.title} needs you, so it can only be snoozed`);
    }
    if (kind === "skip" && !row.skip)
      return fail("usage", `${row.title} cannot be skipped`);
    const until =
      kind === "snooze"
        ? (flagValue(argv, "--until") ??
          iso(Date.now() + (flagValue(argv, "--for") === "1h" ? HOUR : 7 * 24 * HOUR)))
        : undefined;
    undo.push({ id: row.id, previous: hidden.get(row.id) });
    hidden.set(row.id, {
      ...row,
      hidden: { kind, at: iso(Date.now()), ...(until && { until }) },
    });
    return {
      ok: true,
      data: {
        id: row.id,
        title: row.title,
        hidden: { kind, state: kind, ...(until && { until }) },
      },
    };
  }

  return {
    run(argv: string[]): CtlReply {
      if (!options.scenario) {
        return fail("unavailable", "attention is not part of this fixture");
      }
      const verb = argv[1];
      if (verb === "ls") return ls(argv.includes("--hidden"));
      if (verb === "snooze" || verb === "skip" || verb === "dismiss") {
        return hide(verb, argv);
      }
      if (verb === "restore") {
        if (!hidden.delete(argv[2] ?? ""))
          return fail("not_found", `${argv[2]} is not hidden`);
        return { ok: true, data: { id: argv[2], restored: true } };
      }
      if (verb === "undo") {
        const last = undo.pop();
        if (!last) return fail("not_found", "there is nothing to undo");
        if (last.previous) hidden.set(last.id, last.previous);
        else hidden.delete(last.id);
        return { ok: true, data: { id: last.id, undone: true } };
      }
      if (verb === "settings") return { ok: true, data: settings };
      if (verb === "set") {
        const [scope, name] = (argv[2] ?? "").split(".");
        const on = argv[3] === "on";
        if (scope === "notify" && name === undefined) settings.notify.enabled = on;
        else {
          const list = scope === "check" ? settings.checks : settings.notify.categories;
          const entry = list.find((row) => row.name === name);
          if (!entry) return fail("usage", `unknown switch ${argv[2]}`);
          entry.on = on;
        }
        return { ok: true, data: { key: argv[2], on, settings } };
      }
      if (verb === "check") {
        return {
          ok: true,
          data: {
            checkedAt: iso(Date.now()),
            off: !settings.checks.find((c) => c.name === "updates")?.on,
            sources: [{ source: "acme-skills", ok: true }],
          },
        };
      }
      if (verb === "notify") {
        return { ok: true, data: { sent: true } };
      }
      return fail("usage", `unknown command: ${argv.join(" ")}`);
    },
  };
}
