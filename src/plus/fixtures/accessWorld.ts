import type { CtlReply } from "./ctlWorld";
import type { ReachClientSeed } from "./reachWorld";
import type { ServerWorld, WorldSet } from "./serverWorld";
import type {
  AccessSetListData,
  AccessSetRow,
  RouteRow,
  RoutesData,
  ToolsData,
  WriteOutcome,
} from "../access/data";

export interface AccessWorldOptions {
  /** Narrowed tools: access set id, server id and the only tools it exposes. */
  narrowed?: Array<{ set: string; server: string; tools: string[] }>;
}

const RULE =
  "A folder route outranks a client's own access set: inside a routed folder the gateway uses the folder's set for every client.";
const TOOLS_RULE =
  "A server with no entry in a set exposes all its tools; an entry lists the only tools it exposes, and an empty entry none. The names are the tool names the server itself reports.";
const MODES: Record<string, string> = {
  full: "the client sees every tool in the list",
  lazy: "the client finds tools through toolport_search_tools",
  grouped: "the client browses one server at a time",
  inherit: "the global mode applies",
};

const fail = (code: string, message: string, data?: unknown): CtlReply => ({
  ok: false,
  error: { code, message },
  ...(data === undefined ? {} : { data }),
});

/** The same quoting the CLI uses in the undo lines it prints. */
const quote = (text: string) =>
  /^[A-Za-z0-9\-_./:@]+$/.test(text) ? text : `'${text.replace(/'/g, "'\\''")}'`;

const slug = (name: string) =>
  name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");

const normalized = (path: string) => {
  let text = path.trim().replace(/\\/g, "/");
  while (text.length > 1 && text.endsWith("/")) text = text.slice(0, -1);
  return text;
};

/** `access-set`, `client scope` and `folder route` of the in-memory toolportctl, writing into the
 * same sets, clients and folders the reach and server worlds read. */
export function createAccessWorld(
  options: AccessWorldOptions,
  servers: ServerWorld,
  clients: ReachClientSeed[],
  folders: Array<{ path: string; set: string }>,
) {
  const narrowed = new Map<string, Map<string, string[]>>();
  for (const { set, server, tools } of options.narrowed ?? []) {
    narrowed.set(set, (narrowed.get(set) ?? new Map()).set(server, tools));
  }
  const discovery = new Map<string, string>();
  const sets = (): WorldSet[] => servers.sets;
  const find = (key: string) =>
    sets().find((s) => s.id === key.trim()) ??
    sets().find((s) => s.name.toLowerCase() === key.trim().toLowerCase());
  const server = (key: string) =>
    servers.servers.find((s) => s.id === key.trim() || s.name === key.trim());
  const nameOf = (id: string) => servers.servers.find((s) => s.id === id)?.name ?? id;
  const flags = (argv: string[], name: string) =>
    argv.flatMap((arg, i) => (arg === name && argv[i + 1] ? [argv[i + 1]] : []));
  const operands = (argv: string[], from: number) => {
    const out: string[] = [];
    for (let i = from; i < argv.length; i++) {
      if (["--server", "--tool"].includes(argv[i])) i++;
      else if (!argv[i].startsWith("--")) out.push(argv[i]);
    }
    return out;
  };

  const row = (set: WorldSet): AccessSetRow => ({
    id: set.id,
    name: set.name,
    default: servers.defaultAccess === set.id,
    servers: set.serverIds.map((id) => ({ id, name: nameOf(id) })),
    reached: set.serverIds.filter(
      (id) => servers.servers.find((s) => s.id === id)?.enabled,
    ).length,
    clients: clients.filter((c) => c.set === set.id).map((c) => c.id),
    folders: folders.filter((f) => f.set === set.id).map((f) => f.path),
  });

  function done(
    dryRun: boolean,
    plan: { summary: string; steps: string[]; warnings?: string[]; undo: string },
    changes: boolean,
    apply: () => void,
    extra: Partial<WriteOutcome> | Record<string, unknown> = {},
  ): CtlReply {
    if (!dryRun && changes) apply();
    const data = {
      dryRun,
      ...extra,
      plan: {
        effects: {},
        summary: plan.summary,
        steps: plan.steps.map((detail) => ({
          op: changes ? "registry" : "note",
          detail,
        })),
        warnings: plan.warnings ?? [],
        undo: plan.undo,
      },
      result: dryRun
        ? null
        : {
            applied: changes,
            changed: changes ? ["registry"] : [],
            undo: plan.undo,
            backups: [],
          },
    };
    return { ok: true, data };
  }

  function setAdd(argv: string[], dryRun: boolean): CtlReply {
    const name = (operands(argv, 2)[0] ?? "").trim();
    if (name === "") return fail("usage", "give the access set a name");
    if (name.startsWith("@"))
      return fail("usage", "access set names beginning with @ are reserved");
    const existing = sets().find(
      (s) => s.name.toLowerCase() === name.toLowerCase() || s.id === name,
    );
    if (existing) {
      return fail("conflict", `an access set called ${existing.name} exists already`);
    }
    const members: string[] = [];
    for (const key of flags(argv, "--server")) {
      const found = server(key);
      if (!found) return fail("not_found", `no server '${key.trim()}'`);
      if (!members.includes(found.id)) members.push(found.id);
    }
    const id = slug(name);
    return done(
      dryRun,
      {
        summary: `Add access set ${name}`,
        steps: [
          `create the access set ${name}`,
          ...members.map((m) => `list ${nameOf(m)} in ${name}`),
        ],
        warnings:
          members.length === 0
            ? [`${name} lists no server, so a client bound to it reaches nothing`]
            : [],
        undo: `toolportctl access-set rm ${quote(name)}`,
      },
      true,
      () => sets().push({ id, name, serverIds: members }),
      {
        name,
        ...(dryRun ? {} : { accessSet: { ...row({ id, name, serverIds: members }) } }),
      },
    );
  }

  function setRm(argv: string[], dryRun: boolean): CtlReply {
    const key = operands(argv, 2)[0];
    const detach = argv.includes("--detach");
    const leave = argv.includes("--leave");
    if (detach && leave) return fail("usage", "--detach and --leave are exclusive");
    const set = key ? find(key) : undefined;
    if (!set) {
      return fail(
        "not_found",
        `no access set called ${key ?? ""}; the access sets are: ${sets()
          .map((s) => s.name)
          .join(", ")}`,
      );
    }
    if (servers.defaultAccess === set.id) {
      return fail(
        "conflict",
        `${set.name} is the default access; choose another with \`access-set default <set>\` or \`access-set default --none\` first`,
      );
    }
    if (sets().length <= 1) {
      return fail("conflict", `${set.name} is the only access set and cannot be removed`);
    }
    const users = row(set);
    if (users.clients.length + users.folders.length > 0 && !detach && !leave) {
      return fail(
        "conflict",
        `${set.name} is still used by ${[
          users.clients.length > 0 &&
            `${users.clients.length} client(s) (${users.clients.join(", ")})`,
          users.folders.length > 0 &&
            `${users.folders.length} folder route(s) (${users.folders.join(", ")})`,
        ]
          .filter(Boolean)
          .join(
            " and ",
          )}. Choose what happens to them: --detach removes their binding (clients follow the default access), --leave deletes the set only (they reach nothing until bound again)`,
        { clients: users.clients, folders: users.folders },
      );
    }
    const restore = [
      `toolportctl access-set add ${quote(set.name)}${set.serverIds.map((id) => ` --server ${quote(id)}`).join("")}`,
      ...users.clients.map(
        (c) => `toolportctl client scope ${quote(c)} ${quote(set.name)}`,
      ),
      ...users.folders.map(
        (f) => `toolportctl folder route add ${quote(f)} ${quote(set.name)}`,
      ),
      ...(users.default ? [`toolportctl access-set default ${quote(set.name)}`] : []),
    ].join(" && ");
    return done(
      dryRun,
      {
        summary: `Delete access set ${set.name}`,
        steps: [
          `delete the access set ${set.name}`,
          ...(detach
            ? [
                ...users.clients.map((c) => `remove the binding of client ${c}`),
                ...users.folders.map((f) => `remove the binding of folder ${f}`),
              ]
            : []),
        ],
        warnings: detach
          ? ["detached clients follow the default access, which may list more servers"]
          : [
              ...users.clients.map(
                (c) =>
                  `client ${c} keeps naming ${set.name} and reaches nothing until it is bound again`,
              ),
              ...users.folders.map(
                (f) =>
                  `folder ${f} keeps naming ${set.name} and reaches nothing until it is bound again`,
              ),
            ],
        undo: restore,
      },
      true,
      () => {
        if (detach) {
          for (const c of clients) if (c.set === set.id) c.set = undefined;
          for (let i = folders.length - 1; i >= 0; i--)
            if (folders[i].set === set.id) folders.splice(i, 1);
        }
        sets().splice(sets().indexOf(set), 1);
      },
      { accessSet: { id: set.id, name: set.name } },
    );
  }

  function setDefault(argv: string[], dryRun: boolean): CtlReply {
    const none = argv.includes("--none");
    const key = operands(argv, 2)[0];
    if (none === (key !== undefined)) {
      return fail("usage", "usage: access-set default (<set> | --none) [--dry-run]");
    }
    const target = key ? find(key) : undefined;
    if (key && !target) return fail("not_found", `no access set called ${key}`);
    const current = sets().find((s) => s.id === servers.defaultAccess);
    const same = (target?.id ?? null) === (current?.id ?? null);
    return done(
      dryRun,
      {
        summary: same
          ? "The default access is up to date"
          : target
            ? `Make ${target.name} the default access`
            : "Return the default access to every enabled server",
        steps: [
          same
            ? "the default access is already this"
            : target
              ? `clients without their own set reach the enabled servers ${target.name} lists`
              : "clients without their own set reach every enabled server",
        ],
        undo: current
          ? `toolportctl access-set default ${quote(current.name)}`
          : "toolportctl access-set default --none",
      },
      !same,
      () => servers.setDefaultAccess(target?.id ?? null),
      { defaultAccess: target ? { id: target.id, name: target.name } : null },
    );
  }

  function setTools(argv: string[], dryRun: boolean): CtlReply {
    const [setKey, serverKey] = operands(argv, 2);
    if (!setKey) {
      const data: ToolsData = {
        rule: TOOLS_RULE,
        accessSets: sets().map((s) => ({
          id: s.id,
          name: s.name,
          narrowed: [...(narrowed.get(s.id) ?? new Map<string, string[]>())].map(
            ([id, tools]) => ({
              server: { id, name: nameOf(id) },
              listed: s.serverIds.includes(id),
              tools,
            }),
          ),
        })),
      };
      return { ok: true, data };
    }
    const set = find(setKey);
    if (!set) return fail("not_found", `no access set called ${setKey}`);
    const member = serverKey ? server(serverKey) : undefined;
    if (!member) return fail("not_found", `no server '${serverKey ?? ""}'`);
    if (!set.serverIds.includes(member.id)) {
      return fail(
        "conflict",
        `${member.name} is not listed in ${set.name}; list it there before narrowing its tools`,
      );
    }
    const before = narrowed.get(set.id)?.get(member.id) ?? null;
    const after = argv.includes("--all")
      ? null
      : argv.includes("--none")
        ? []
        : flags(argv, "--tool");
    const same = JSON.stringify(before) === JSON.stringify(after);
    const base = `toolportctl access-set tools ${quote(set.name)} ${quote(member.name)}`;
    return done(
      dryRun,
      {
        summary:
          after === null
            ? `Expose every tool of ${member.name} in ${set.name}`
            : after.length === 0
              ? `Expose no tool of ${member.name} in ${set.name}`
              : `Limit ${member.name} in ${set.name} to ${after.length} tool(s)`,
        steps: [
          same
            ? "the tool list is already this"
            : after === null
              ? `remove the tool list of ${member.name} in ${set.name}`
              : `expose only ${after.join(", ")} of ${member.name} in ${set.name}`,
        ],
        warnings:
          after && after.length > 0
            ? [
                "tool names are not checked against what the server lists; a misspelt name hides that tool",
              ]
            : [],
        undo:
          before === null
            ? `${base} --all`
            : before.length === 0
              ? `${base} --none`
              : `${base}${before.map((t) => ` --tool ${quote(t)}`).join("")}`,
      },
      !same,
      () => {
        const map = narrowed.get(set.id) ?? new Map<string, string[]>();
        if (after === null) map.delete(member.id);
        else map.set(member.id, after);
        narrowed.set(set.id, map);
      },
      {
        accessSet: { id: set.id, name: set.name },
        server: { id: member.id, name: member.name },
        tools: after,
        before,
      },
    );
  }

  function clientScope(argv: string[], dryRun: boolean): CtlReply {
    const [clientId, setKey] = operands(argv, 2);
    const none = argv.includes("--none");
    if (!clientId || none === (setKey !== undefined)) {
      return fail("usage", "usage: client scope <client> (<set> | --none) [--dry-run]");
    }
    const client = clients.find((c) => c.id === clientId);
    if (!client) {
      return fail(
        "not_found",
        `no client '${clientId}'; the clients are: ${clients.map((c) => c.id).join(", ")}`,
      );
    }
    const target = setKey ? find(setKey) : undefined;
    if (setKey && !target) return fail("not_found", `no access set called ${setKey}`);
    const current = client.set ? find(client.set) : undefined;
    const same = (target?.id ?? null) === (client.set ?? null);
    return done(
      dryRun,
      {
        summary: same
          ? `${clientId} is up to date`
          : target
            ? `Bind ${clientId} to the access set ${target.name}`
            : `Unbind ${clientId} from its access set`,
        steps: [
          same
            ? "nothing to change"
            : target
              ? `${clientId} reaches the enabled servers ${target.name} lists`
              : `${clientId} follows the default access`,
        ],
        warnings:
          target && folders.length > 0
            ? [`${RULE} Routed folders: ${folders.map((f) => f.path).join(", ")}.`]
            : [],
        undo: current
          ? `toolportctl client scope ${quote(clientId)} ${quote(current.name)}`
          : `toolportctl client scope ${quote(clientId)} --none`,
      },
      !same,
      () => {
        client.set = target?.id;
      },
      {
        client: clientId,
        accessSet: target ? { id: target.id, name: target.name } : null,
      },
    );
  }

  function folderRoute(argv: string[], dryRun: boolean): CtlReply {
    const verb = argv[2];
    if (verb === "ls") {
      const data: RoutesData = {
        rule: RULE,
        routes: folders.map((f): RouteRow => {
          const set = find(f.set);
          return {
            path: f.path,
            accessSet: set ? { id: set.id, name: set.name } : null,
            missing: !set,
            reference: f.set,
            servers: set
              ? set.serverIds
                  .filter((id) => servers.servers.find((s) => s.id === id)?.enabled)
                  .map((id) => ({ id, name: nameOf(id) }))
              : [],
          };
        }),
      };
      return { ok: true, data };
    }
    const [rawPath, setKey] = operands(argv, 3);
    const path = normalized(rawPath ?? "");
    const existing = folders.find((f) => normalized(f.path) === path);
    if (verb === "rm") {
      if (!existing) return fail("not_found", `no folder route for ${path}`);
      const set = find(existing.set);
      return done(
        dryRun,
        {
          summary: `Remove the folder route for ${path}`,
          steps: [
            `${path} and the folders below it stop using ${set?.name ?? existing.set}`,
          ],
          undo: `toolportctl folder route add ${quote(path)} ${quote(set?.name ?? existing.set)}`,
        },
        true,
        () => folders.splice(folders.indexOf(existing), 1),
        { path, accessSet: { name: set?.name ?? existing.set } },
      );
    }
    if (path === "") return fail("usage", "choose a project folder");
    if (!/^(\/|[A-Za-z]:\/)/.test(path)) {
      return fail(
        "usage",
        `${path} is not an absolute folder; the gateway matches the absolute folder a client reports`,
      );
    }
    const set = setKey ? find(setKey) : undefined;
    if (!set) return fail("not_found", `no access set called ${setKey ?? ""}`);
    const previous = existing ? find(existing.set) : undefined;
    const same = existing?.set === set.id;
    return done(
      dryRun,
      {
        summary: same
          ? `${path} is already routed to ${set.name}`
          : `Route ${path} to the access set ${set.name}`,
        steps: [
          same ? "nothing to change" : `${path} and the folders below it use ${set.name}`,
        ],
        warnings: clients.some((c) => c.set)
          ? [
              `${RULE} Clients with their own set: ${clients
                .filter((c) => c.set)
                .map((c) => c.id)
                .join(", ")}.`,
            ]
          : [],
        undo: existing
          ? `toolportctl folder route add ${quote(path)} ${quote(previous?.name ?? existing.set)}`
          : `toolportctl folder route rm ${quote(path)}`,
      },
      !same,
      () => {
        if (existing) existing.set = set.id;
        else folders.push({ path, set: set.id });
      },
      { path, accessSet: { id: set.id, name: set.name } },
    );
  }

  return {
    discovery,
    run(argv: string[]): CtlReply {
      const dryRun = argv.includes("--dry-run");
      if (argv[0] === "access-set") {
        const verb = argv[1];
        if (verb === "ls") {
          const data: AccessSetListData = {
            defaultAccess: servers.defaultAccess,
            accessSets: sets().map(row),
          };
          return { ok: true, data };
        }
        if (verb === "add") return setAdd(argv, dryRun);
        if (verb === "rm") return setRm(argv, dryRun);
        if (verb === "default") return setDefault(argv, dryRun);
        if (verb === "tools") return setTools(argv, dryRun);
      }
      if (argv[0] === "client" && argv[1] === "scope") return clientScope(argv, dryRun);
      if (argv[0] === "folder" && argv[1] === "route") return folderRoute(argv, dryRun);
      return fail("usage", `unsupported in the fixture: ${argv.join(" ")}`);
    },
    tool(name: string, args: Record<string, unknown>): CtlReply | null {
      if (name !== "client_discovery_set") return null;
      const client = String(args.client ?? "");
      const mode = String(args.mode ?? "");
      const wrap = (result: unknown, isError = false): CtlReply => ({
        ok: true,
        data: { tool: name, tier: 3, isError, result },
      });
      if (!(mode in MODES)) {
        return wrap(
          {
            error: {
              kind: "invalid_arguments",
              message: "mode is full, lazy, grouped or inherit",
            },
          },
          true,
        );
      }
      if (!clients.some((c) => c.id === client)) {
        return wrap(
          { error: { kind: "not_found", message: `no client '${client}'` } },
          true,
        );
      }
      const dryRun = args.dry_run !== false;
      if (!dryRun && args.confirm !== true) {
        return wrap(
          { error: { kind: "refused", message: "this change needs confirm=true" } },
          true,
        );
      }
      const was = discovery.get(client) ?? "inherit";
      const same = was === mode;
      const reply = done(
        dryRun,
        {
          summary: same
            ? `${client} already uses ${mode}`
            : `Set the discovery mode of ${client} to ${mode}`,
          steps: [
            same
              ? "nothing to change"
              : mode === "inherit"
                ? `${client} follows the global discovery mode again`
                : `${client} gets the ${mode} tool listing instead of the global mode`,
          ],
          undo: `client_discovery_set client=${client} mode=${was}`,
        },
        !same,
        () => {
          if (mode === "inherit") discovery.delete(client);
          else discovery.set(client, mode);
        },
        { client, mode, was, modes: MODES },
      );
      return wrap(reply.data);
    },
  };
}

export type AccessWorld = ReturnType<typeof createAccessWorld>;
