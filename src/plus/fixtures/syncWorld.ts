import type { CtlReply } from "./ctlWorld";

const PLAN = "fixture-sync-plan";
const MINUTE = 60_000;

export interface SyncWorldOptions {
  /** `false`: nothing is set up yet, the first-run form shows. */
  configured?: boolean;
  /** The keychain is locked: status says paused. */
  locked?: boolean;
  /** Upstream Sync (Pro) is on and owns the Servers part. */
  upstream?: boolean;
  /** `empty`: nothing on the remote yet. `same`: the remote is as this machine has it. */
  remote?: "ahead" | "empty" | "same";
  /** Rows both machines changed that the last pull left unsettled. */
  conflicts?: string[];
  /** The passphrase is not in the keychain yet. */
  noPassphrase?: boolean;
  /** `status` fails with this message. */
  statusError?: string;
}

const PARTS = [
  ["servers", "Servers", true],
  ["access", "Clients and access sets", true],
  ["tasks", "Tasks", true],
  ["settings", "Toolport settings", true],
  ["claudeUser", "Claude Code user files", false],
  ["memory", "Memory", false],
  ["secrets", "Secrets", false],
] as const;

/** The sync commands against one remote: a server that changed on the other machine, rows both
 * machines changed, a plan hash an apply must name and an undo for the last apply. */
export function createSyncWorld(options: SyncWorldOptions = {}) {
  const config = {
    configured: options.configured ?? true,
    enabled: true,
    transport: {
      kind: "git",
      url: "git@example.invalid:me/toolport-sync.git",
      branch: "main",
    } as { kind: "git"; url: string; branch: string } | { kind: "folder"; path: string },
    parts: new Set<string>(PARTS.filter(([, , on]) => on).map(([id]) => id)),
    schedule: false,
    passphrase: !options.noPassphrase,
  };
  let pending = [...(options.conflicts ?? [])];
  let incoming = (options.remote ?? "ahead") === "ahead";
  let applied: string | null = null;
  let before: { incoming: boolean; pending: string[] } | null = null;
  let pushes = 0;
  let lastSync: {
    direction: string;
    at: string;
    revision: string;
    machine: string;
  } | null =
    options.configured === false
      ? null
      : {
          direction: "push",
          at: new Date(Date.now() - 4 * MINUTE).toISOString(),
          revision: "r1",
          machine: "Studio",
        };

  const fail = (code: string, message: string): CtlReply => ({
    ok: false,
    error: { code, message },
  });

  function status() {
    const locked = options.locked ?? false;
    return {
      configured: config.configured,
      enabled: config.enabled,
      transport: config.configured ? config.transport : null,
      parts: PARTS.map(([id, label, byDefault]) => {
        const handled = options.upstream && id === "servers";
        return {
          id,
          label,
          default: byDefault,
          on: config.parts.has(id) && !handled,
          handledBy: handled ? "upstream" : null,
        };
      }),
      passphrase: locked ? "locked" : config.passphrase ? "stored" : "missing",
      keyringLocked: locked,
      paused: locked && config.configured && config.enabled,
      upstream: { active: options.upstream ?? false },
      lastSync,
      lastCheck: null,
      pending,
      remoteSeen: null,
      machines: [
        { machine: "Studio", at: null, thisMachine: true },
        {
          machine: "Laptop",
          at: new Date(Date.now() - 3 * 60 * MINUTE).toISOString(),
          thisMachine: false,
        },
      ],
      schedule: {
        task: "sync-pull",
        present: true,
        enabled: config.schedule,
        cron: "*/30 * * * *",
      },
      undoable: applied,
    };
  }

  function rows(take: string[]) {
    const settled = (id: string) => take.includes(id);
    return [
      {
        id: "server:alpha",
        label: "alpha",
        part: "servers",
        state: incoming ? "incoming" : "here",
        note: incoming
          ? "changed on the other machine: it is replaced, held if what it runs changed; the old one is saved for Undo"
          : "already here, same",
      },
      ...pending.map((id) => ({
        id,
        label: id.slice(id.indexOf(":") + 1),
        part: id.slice(0, id.indexOf(":")),
        state: "conflict",
        note: settled(id)
          ? "changed on both machines: replaced by the other machine's; the old one is saved for Undo"
          : "changed on both machines: yours is kept until you take the other machine's",
      })),
    ];
  }

  function summary(list: ReturnType<typeof rows>) {
    const count = (state: string) => list.filter((r) => r.state === state).length;
    return {
      add: 0,
      here: count("here"),
      incoming: count("incoming"),
      mine: 0,
      conflict: count("conflict"),
      needs: 0,
      skip: 0,
    };
  }

  return {
    run(key: string[], stdin?: string): CtlReply {
      const [, verb] = key;
      const flag = (name: string) => key[key.indexOf(name) + 1];
      const all = (name: string) =>
        key.flatMap((arg, index) => (arg === name ? [key[index + 1]] : []));
      if (verb === "status") {
        if (options.statusError) return fail("failed", options.statusError);
        return { ok: true, data: status() };
      }
      if (verb === "setup") {
        if (key.includes("--passphrase-stdin")) {
          if (!stdin || stdin.length < 12)
            return fail("usage", "the passphrase must be at least 12 characters");
          config.passphrase = true;
        }
        if (key.includes("--forget-passphrase")) config.passphrase = false;
        if (key.includes("--git"))
          config.transport = {
            kind: "git",
            url: flag("--git"),
            branch: flag("--branch") ?? "main",
          };
        if (key.includes("--folder"))
          config.transport = { kind: "folder", path: flag("--folder") };
        if (key.includes("--git") || key.includes("--folder")) config.configured = true;
        if (key.includes("--parts")) config.parts = new Set(flag("--parts").split(","));
        if (key.includes("--schedule")) config.schedule = flag("--schedule") === "on";
        if (key.includes("--enable")) config.enabled = true;
        if (key.includes("--disable")) config.enabled = false;
        return { ok: true, data: status() };
      }
      if (!config.configured) return fail("failed", "sync is not set up");
      if (verb === "pull" || verb === "plan") {
        if (!config.passphrase)
          return fail("failed", "no passphrase is stored on this machine");
        if (options.remote === "empty")
          return {
            ok: true,
            data: {
              empty: true,
              applied: false,
              dryRun: true,
              note: "nothing is on the remote yet: push from the machine that has the setup",
              rows: [],
            },
          };
        const take = all("--take");
        const keep = all("--keep");
        if (!key.includes("--apply")) {
          const list = rows(take);
          return {
            ok: true,
            data: {
              applied: false,
              dryRun: true,
              plan: PLAN,
              rows: list,
              summary: summary(list),
              remote: { machine: "Laptop", madeAt: new Date().toISOString() },
            },
          };
        }
        if (flag("--plan") !== PLAN)
          return fail("stale_plan", "this machine changed since the plan was made");
        before = { incoming, pending: [...pending] };
        const settledNow = [...take, ...keep];
        pending = pending.filter((id) => !settledNow.includes(id));
        incoming = false;
        applied = "run-1";
        lastSync = {
          direction: "pull",
          at: new Date().toISOString(),
          revision: "r2",
          machine: "Studio",
        };
        return {
          ok: true,
          data: {
            applied: true,
            dryRun: false,
            run: "run-1",
            failed: 0,
            steps: take.map((id) => ({ id, ok: true, note: "replaced" })),
            needsYou: before.incoming
              ? [
                  {
                    id: "server:alpha",
                    label: "1 server arrived from Laptop",
                    note: "It is held. Review it in Servers before it runs here.",
                  },
                ]
              : [],
            pending,
          },
        };
      }
      if (verb === "push") {
        if (key.includes("--dry-run"))
          return {
            ok: true,
            data: {
              dryRun: true,
              totalSize: 3674,
              parts: [
                { id: "servers", label: "Servers", count: 2 },
                { id: "settings", label: "Toolport settings", count: 9 },
              ],
              notIncluded: [
                "your code and anything uncommitted, stashed or unpushed",
                "server sign-in sessions (you sign in again)",
              ],
            },
          };
        if (pending.length > 0 && !key.includes("--keep-mine"))
          return fail(
            "failed",
            `the last pull left rows both machines changed: ${pending.join(", ")}. Settle them first`,
          );
        pushes += 1;
        lastSync = {
          direction: "push",
          at: new Date().toISOString(),
          revision: `r${2 + pushes}`,
          machine: "Studio",
        };
        return {
          ok: true,
          data: { dryRun: false, pushed: true, revision: lastSync.revision },
        };
      }
      if (verb === "undo") {
        if (!applied || !before) return fail("not_found", "no applied sync pull to undo");
        incoming = before.incoming;
        pending = before.pending;
        applied = null;
        before = null;
        return { ok: true, data: { run: "run-1", steps: [] } };
      }
      return fail("usage", `unknown sync verb ${verb}`);
    },
  };
}
