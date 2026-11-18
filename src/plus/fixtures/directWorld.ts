import type { CtlReply } from "./ctlWorld";
import type { WorldServer } from "./serverWorld";
import type {
  DirectAction,
  DirectOutcome,
  DirectRow,
  DirectState,
} from "../clients/data";

export interface DirectClientSeed {
  id: string;
  name: string;
  path: string;
  /** Why this client cannot hold a direct entry. */
  unsupported?: string;
}

export interface DirectEntrySeed {
  client: string;
  /** The entry's name, which is the server's name. */
  entry: string;
  server: string;
  state?: DirectState;
  /** The entry is no longer in the client's file; removing it only drops the record. */
  gone?: boolean;
}

export interface DirectWorldOptions {
  clients?: DirectClientSeed[];
  entries?: DirectEntrySeed[];
  /** Servers that cannot get a direct entry, by id, with the reason. */
  refuse?: Record<string, string>;
}

const OFFERS: Partial<Record<DirectState, string>> = {
  stale: "rewrite",
  customized: "rewrite",
  missing: "rewrite or forget",
  orphan: "remove",
  unrecorded: "track",
  relocated: "re-point",
};

const TRADEOFF =
  "A direct entry bypasses the Toolport gateway: its tools are not covered by access sets, approvals or lazy discovery, and every client with a direct entry starts its own process of the server instead of sharing one.";
const REMOTE_TRADEOFF =
  "A direct entry bypasses the Toolport gateway: its tools are not covered by access sets, approvals or lazy discovery. Toolport writes no credentials into the client: the client signs in to the server by itself.";

const fail = (code: string, message: string): CtlReply => ({
  ok: false,
  error: { code, message },
});

/** `client direct ls|add|rm` in memory, over the servers of the server world: the plan, its hash,
 * the refusals and the seven states as the real command answers them. */
export function createDirectWorld(
  options: DirectWorldOptions,
  servers: () => WorldServer[],
) {
  const clients = options.clients ?? [
    { id: "claude-code", name: "Claude Code", path: "/Users/example/.claude.json" },
  ];
  const entries = (options.entries ?? []).map((seed) => ({
    ...seed,
    state: seed.state ?? ("ok" as DirectState),
  }));
  let revision = 0;
  const log: string[][] = [];

  const find = (key: string) =>
    servers().find((s) => s.id === key || s.name.toLowerCase() === key.toLowerCase());

  function rowOf(seed: (typeof entries)[number]): DirectRow {
    const client = clients.find((c) => c.id === seed.client)!;
    const server = servers().find((s) => s.id === seed.server);
    return {
      client: client.id,
      clientName: client.name,
      entry: seed.entry,
      server: seed.server,
      serverName: server?.name ?? null,
      state: seed.state,
      path: client.path,
      ...(server?.transport !== "stdio" && server?.url ? { url: server.url } : {}),
      ...(OFFERS[seed.state] ? { action: OFFERS[seed.state] } : {}),
    };
  }

  function outcome(
    verb: "add" | "rm",
    action: DirectAction,
    ctx: { client: (typeof clients)[number]; server: WorldServer; entry: string },
    dryRun: boolean,
  ): DirectOutcome {
    const remote = ctx.server.transport !== "stdio";
    const hash = `plan-${revision}-${verb}-${ctx.client.id}-${ctx.server.id}-${action}`;
    const line = remote
      ? `{"type":"http","url":"${ctx.server.url}"}`
      : `{"args":["direct","run","${ctx.server.id}"],"command":"/Applications/Toolport.app/Contents/MacOS/toolportctl"}`;
    const steps =
      action === "unchanged" || action === "absent"
        ? []
        : verb === "add"
          ? [
              {
                op: "merge",
                detail: `${action === "added" ? "add" : "replace"} the entry '${ctx.entry}' and leave every other entry alone`,
                path: ctx.client.path,
                diff: {
                  before:
                    action === "added" ? "(absent)" : `entry '${ctx.entry}' as it stands`,
                  after: `+ "${ctx.entry}": ${line}`,
                },
              },
              {
                op: "note",
                detail: `record '${ctx.entry}' under plus.directEntries.${ctx.client.id} in the registry`,
              },
              {
                op: "note",
                detail:
                  "a backup of the client file is made first; the write goes through the client's own lock and conflict check",
              },
            ]
          : action === "forgotten"
            ? [
                {
                  op: "note",
                  detail: `drop the record of '${ctx.entry}' under plus.directEntries.${ctx.client.id}`,
                },
              ]
            : [
                {
                  op: "merge",
                  detail: `remove the entry '${ctx.entry}' and leave every other entry alone`,
                  path: ctx.client.path,
                  diff: { before: `entry '${ctx.entry}'`, after: "(absent)" },
                },
                {
                  op: "note",
                  detail: `drop the record of '${ctx.entry}' under plus.directEntries.${ctx.client.id}`,
                },
              ];
    return {
      action,
      changed: steps.length > 0,
      client: ctx.client.id,
      clientName: ctx.client.name,
      dryRun,
      entry: ctx.entry,
      path: ctx.client.path,
      plan: {
        summary:
          verb === "add"
            ? `Give ${ctx.client.name} a direct entry '${ctx.entry}' for ${ctx.server.name}`
            : `Remove the direct entry '${ctx.entry}' from ${ctx.client.name}`,
        steps,
        warnings: verb === "add" ? [remote ? REMOTE_TRADEOFF : TRADEOFF] : [],
        undo:
          verb === "add"
            ? `toolportctl client direct rm ${ctx.server.id} --client ${ctx.client.id}`
            : `toolportctl client direct add ${ctx.server.id} --client ${ctx.client.id}`,
      },
      planHash: hash,
      result: dryRun
        ? null
        : {
            applied: true,
            changed: [ctx.client.path],
            undo: "",
            backups: [`${ctx.client.path}.bak`],
          },
      server: ctx.server.id,
      serverName: ctx.server.name,
    };
  }

  function ls(argv: string[]): CtlReply {
    const at = argv.indexOf("--client");
    const only = at >= 0 ? argv[at + 1] : null;
    if (only && !clients.some((c) => c.id === only)) {
      return fail("not_found", `client '${only}' is not supported`);
    }
    const rows = entries.filter((e) => !only || e.client === only).map(rowOf);
    return { ok: true, data: { entries: rows } };
  }

  function write(verb: "add" | "rm", argv: string[]): CtlReply {
    const flag = (name: string) => argv.indexOf(name);
    const value = (name: string) => (flag(name) >= 0 ? argv[flag(name) + 1] : null);
    const key = argv[3];
    const clientId = value("--client");
    const dryRun = flag("--dry-run") >= 0;
    const force = flag("--force") >= 0;
    const planHash = value("--plan-hash");
    if (!key || !clientId) return fail("usage", `usage: client direct ${verb}`);
    const client = clients.find((c) => c.id === clientId);
    const recorded = entries.find(
      (e) => e.client === clientId && (e.server === key || e.entry === key),
    );
    const server = find(key) ?? servers().find((s) => s.id === recorded?.server);
    if (!server && !recorded) return fail("not_found", `Server '${key}' not found`);
    if (!client) return fail("not_found", `client '${clientId}' is not supported`);
    if (client.unsupported) return fail("unsupported", client.unsupported);

    const target = server ?? {
      id: recorded!.server,
      name: recorded!.entry,
      transport: "stdio" as const,
      url: null,
    };
    const entry = recorded?.entry ?? target.name;
    const ctx = { client, server: target as WorldServer, entry };
    let action: DirectAction;
    if (verb === "add") {
      const refusal = options.refuse?.[target.id];
      if (refusal) {
        return fail(
          "refused",
          `${target.name} cannot get a direct entry: ${refusal}. It stays reachable through the gateway.`,
        );
      }
      const present = recorded !== undefined && recorded.state !== "missing";
      if (!present) action = "added";
      else if (recorded.state === "ok") action = "unchanged";
      else if (recorded.state === "unrecorded") action = "adopted";
      else if (recorded.state === "customized" && !force) {
        return fail(
          "conflict",
          `${client.name}'s entry '${entry}' was changed after Toolport wrote it; use --force to replace it`,
        );
      } else action = "updated";
    } else if (!recorded) {
      action = "absent";
    } else if (recorded.state === "missing" || recorded.gone) {
      action = "forgotten";
    } else if (recorded.state === "customized" && !force) {
      return fail(
        "conflict",
        `${client.name}'s entry '${entry}' was changed after Toolport wrote it; use --force to remove it`,
      );
    } else action = "removed";

    const planned = outcome(verb, action, ctx, true);
    if (dryRun) return { ok: true, data: planned };
    if (planHash && planHash !== planned.planHash) {
      return fail(
        "conflict",
        "the plan changed since it was shown (the client file or the server differs); run it with --dry-run again",
      );
    }
    log.push(argv);
    revision += 1;
    if (verb === "add") {
      if (recorded) recorded.state = "ok";
      else entries.push({ client: clientId, entry, server: target.id, state: "ok" });
    } else if (recorded) {
      entries.splice(entries.indexOf(recorded), 1);
    }
    return { ok: true, data: outcome(verb, action, ctx, false) };
  }

  return {
    entries,
    /** The argv of every write that went through, in order. */
    get writes() {
      return log;
    },
    /** Someone edits the client's file by hand, so a plan made before no longer applies. */
    touch() {
      revision += 1;
    },
    run(argv: string[]): CtlReply {
      const [, , verb] = argv;
      if (verb === "ls") return ls(argv);
      if (verb === "add" || verb === "rm") return write(verb, argv);
      throw new Error(`Unimplemented fixture command: plus_ctl ${argv.join(" ")}`);
    },
  };
}
