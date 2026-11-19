import type {
  PackageBatchDone,
  PackageChange,
  PackageKind,
  PackageUpdateDone,
  PackageUpdatePlan,
  SourceRowData,
  SourceState,
} from "../servers/data";
import type { CtlReply } from "./ctlWorld";

export interface PackageSeed {
  kind?: PackageKind;
  package?: string;
  /** The version the server runs. */
  from?: string;
  /** The newest version the last check found. */
  to?: string;
  /** What a fresh lookup finds instead of `to`, so a plan made from the stored check goes stale. */
  newer?: string;
  /** Calls running in the gateways; `null` means no gateway answers. */
  inFlight?: number | null;
  /** The release file for a GitHub install; `checksum: null` means none is published. */
  release?: { asset: string; size: number; checksum: string | null; file: string };
  /** The plan is refused with this reason. */
  refusal?: { code: string; message: string };
  /** A fetch fails with this reason, as an unreachable registry does. */
  offline?: string;
  /** The apply fails with this reason (the checksum did not match). */
  applyFails?: string;
  /** What the Source row says when the plan would not: a check that failed or never ran, a
   * runner that fetches the newest version itself. */
  sourceState?: SourceState;
  sourceError?: string;
  /** The server's source records an image digest that follows this tag. */
  tracks?: string;
  /** Someone edits the pin to this version just before the first apply, so the reviewed plan is
   * stale and the apply is refused. */
  conflictOnce?: string;
}

interface State {
  seed: Required<
    Omit<
      PackageSeed,
      | "release"
      | "refusal"
      | "offline"
      | "applyFails"
      | "newer"
      | "conflictOnce"
      | "sourceState"
      | "sourceError"
      | "tracks"
    >
  > &
    Pick<
      PackageSeed,
      | "release"
      | "refusal"
      | "offline"
      | "applyFails"
      | "newer"
      | "conflictOnce"
      | "sourceState"
      | "sourceError"
      | "tracks"
    >;
  running: string;
  applied: { from: string; to: string } | null;
}

function fail(code: string, message: string, data?: unknown): CtlReply {
  return { ok: false, error: { code, message }, ...(data ? { data } : {}) };
}

function flagValue(argv: string[], flag: string): string | null {
  const at = argv.indexOf(flag);
  return at >= 0 && argv[at + 1] ? argv[at + 1] : null;
}

/** `server update ...` of the in-memory toolportctl: pinned packages and releases by server id
 * that move when an update is applied and move back on undo. */
export function createPackageWorld(
  seeds: Record<string, PackageSeed>,
  find: (key: string) => { id: string; name: string } | undefined,
  list: () => Array<{ id: string; name: string; transport: string }> = () => [],
) {
  const states = new Map<string, State>(
    Object.entries(seeds).map(([id, seed]) => [
      id,
      {
        seed: {
          kind: "npm",
          package: "@scope/alpha",
          from: "1.4.2",
          to: "1.5.0",
          inFlight: null,
          ...seed,
        },
        running: seed.from ?? "1.4.2",
        applied: null,
      },
    ]),
  );

  function changes(state: State, to: string): PackageChange[] {
    const { seed } = state;
    if (seed.release) {
      return [
        { field: "mcpmSource.current_version", from: state.running, to },
        {
          field: `file ${seed.release.file}`,
          from: "the installed binary",
          to: seed.release.asset,
        },
      ];
    }
    return [
      {
        field: "args[1]",
        from: `${seed.package}@${state.running}`,
        to: `${seed.package}@${to}`,
      },
    ];
  }

  function plan(
    name: string,
    id: string,
    state: State,
    argv: string[],
  ): PackageUpdatePlan {
    const { seed } = state;
    const fetch = argv.includes("--fetch");
    const explicit = flagValue(argv, "--to");
    const to = explicit ?? (fetch ? (seed.newer ?? seed.to) : seed.to);
    const same = to === state.running;
    const refusal =
      seed.refusal ??
      (same
        ? {
            code: "up_to_date",
            message: `${seed.package} is already on ${state.running}`,
          }
        : null);
    return {
      server: name,
      id,
      kind: seed.kind,
      package: seed.package,
      from: state.running,
      to,
      downgrade: false,
      digest: seed.tracks !== undefined,
      verified: fetch,
      changes: refusal ? [] : changes(state, to),
      release:
        seed.release && !refusal
          ? {
              tag: `v${to}`,
              asset: { name: seed.release.asset, size: seed.release.size },
              checksum: seed.release.checksum ? { source: seed.release.checksum } : null,
              file: seed.release.file,
              verifyCommand: false,
            }
          : null,
      refusal,
      inFlight: refusal ? null : seed.inFlight,
      planHash: `sha256:${state.running}>${to}`,
      apply: refusal
        ? null
        : `toolportctl server update ${name} --to ${to} --plan-hash sha256:${state.running}>${to}`,
      undo: `toolportctl server update ${name} --undo`,
    };
  }

  function doneOf(
    name: string,
    id: string,
    state: State,
    from: string,
    to: string,
    argv: string[],
    back: boolean,
  ): PackageUpdateDone {
    const busy = (state.seed.inFlight ?? 0) > 0 && !argv.includes("--restart-now");
    const restart = busy
      ? {
          status: "busy",
          detail: `a call is running; restart when it ends with: toolportctl server restart ${name}`,
        }
      : { status: "restarted", detail: `Restarted ${name} in 1 gateway process` };
    return {
      applied: true,
      server: name,
      id,
      kind: state.seed.kind,
      package: state.seed.package,
      from,
      to,
      changes: [],
      restart,
      warnings: [],
      undo: back
        ? `toolportctl server update ${name}`
        : `toolportctl server update ${name} --undo`,
    };
  }

  function done(
    name: string,
    id: string,
    state: State,
    from: string,
    to: string,
    argv: string[],
    back: boolean,
  ): CtlReply {
    return { ok: true, data: doneOf(name, id, state, from, to, argv, back) };
  }

  const operands = (argv: string[]) => {
    const rest = argv.slice(1);
    const end = rest.findIndex((part) => part.startsWith("--"));
    return end === -1 ? rest : rest.slice(0, end);
  };

  function sourceRow(server: { id: string; name: string }): SourceRowData {
    const state = states.get(server.id);
    const base: SourceRowData = {
      server: server.name,
      id: server.id,
      kind: "none",
      state: "unknown",
      package: null,
      tracks: null,
      pinned: null,
      latest: null,
      checkedAt: null,
      error: null,
      manual: null,
      branch: null,
      upstream: null,
      behind: 0,
      root: null,
    };
    if (!state) return base;
    const { seed } = state;
    const available = seed.to !== state.running;
    return {
      ...base,
      kind: seed.kind,
      state: seed.sourceState ?? (available ? "update_available" : "up_to_date"),
      package: seed.package,
      tracks: seed.tracks ?? null,
      pinned: state.running,
      latest: seed.sourceState ? null : seed.to,
      checkedAt: seed.sourceState === "unchecked" ? null : "2026-11-18T10:02:00+01:00",
      error: seed.sourceError ?? null,
      manual: seed.refusal?.code === "cannot_rewrite" ? seed.refusal.message : null,
    };
  }

  function batch(argv: string[]): CtlReply {
    const all = argv.includes("--all");
    const names = all
      ? [...states]
          .filter(([, state]) => state.seed.to !== state.running)
          .map(([id]) => id)
      : operands(argv).map((key) => find(key)?.id ?? key);
    const found = [];
    for (const key of names) {
      const server = find(key);
      const state = server ? states.get(server.id) : undefined;
      if (!server || !state)
        return fail("not_found", `no server '${key}' pins a package version`);
      found.push({ server, state });
    }
    found.sort((a, b) => a.server.name.localeCompare(b.server.name));
    const plans = found.map(({ server, state }) =>
      plan(server.name, server.id, state, argv.includes("--fetch") ? ["--fetch"] : []),
    );
    const planHash = `sha256:batch:${plans.map((p) => p.planHash).join("+")}`;
    const undoLine = `toolportctl server update ${plans.map((p) => p.server).join(" ")} --undo`;
    if (argv.includes("--undo")) {
      const results = found.map(({ server, state }) => {
        const applied = state.applied;
        if (!applied)
          return {
            server: server.name,
            id: server.id,
            ok: false,
            result: null,
            error: {
              code: "not_found",
              message: `no update of ${server.name} to take back`,
            },
          };
        state.running = applied.from;
        state.applied = null;
        return {
          server: server.name,
          id: server.id,
          ok: true,
          result: doneOf(
            server.name,
            server.id,
            state,
            applied.to,
            applied.from,
            argv,
            true,
          ),
          error: null,
        };
      });
      const data: PackageBatchDone = {
        applied: results.filter((r) => r.result).length,
        failed: results.filter((r) => r.error).length,
        results,
        undo: `toolportctl server update ${plans.map((p) => p.server).join(" ")}`,
      };
      return { ok: data.failed === 0, data };
    }
    if (argv.includes("--dry-run")) {
      return {
        ok: true,
        data: {
          batch: {
            count: plans.length,
            plans,
            planHash,
            apply: `toolportctl server update ${plans.map((p) => p.server).join(" ")} --plan-hash ${planHash}`,
            undo: undoLine,
          },
        },
      };
    }
    const given = flagValue(argv, "--plan-hash");
    if (given && given !== planHash)
      return fail(
        "conflict",
        "the plan has changed since it was reviewed; review it again",
        { planHash },
      );
    const results = found.map(({ server, state }, i) => {
      const current = plans[i];
      if (current.refusal) {
        return {
          server: server.name,
          id: server.id,
          ok: false,
          result: null,
          error: { code: "update_refused", message: current.refusal.message },
        };
      }
      if (current.release?.checksum === null && !argv.includes("--allow-unverified")) {
        return {
          server: server.name,
          id: server.id,
          ok: false,
          result: null,
          error: {
            code: "update_refused",
            message:
              "the release publishes no checksum for this file; pass --allow-unverified to install it anyway",
          },
        };
      }
      const from = state.running;
      state.applied = { from, to: current.to };
      state.running = current.to;
      return {
        server: server.name,
        id: server.id,
        ok: true,
        result: doneOf(server.name, server.id, state, from, current.to, argv, false),
        error: null,
      };
    });
    const data: PackageBatchDone = {
      applied: results.filter((r) => r.result).length,
      failed: results.filter((r) => r.error).length,
      results,
      undo: undoLine,
    };
    return { ok: data.failed === 0, data };
  }

  return {
    has: (id: string) => states.has(id),
    /** `argv` starts at the verb: `["update", <server>, ...flags]`. */
    /** `server source [<server>]`: `argv` starts at the verb. */
    source(argv: string[]): CtlReply {
      const key = argv[1];
      if (key) {
        const server = find(key);
        if (!server) return fail("not_found", `no server '${key}'`);
        return { ok: true, data: { sources: [sourceRow(server)] } };
      }
      return {
        ok: true,
        data: {
          sources: list()
            .filter((server) => server.transport === "stdio")
            .map(sourceRow),
        },
      };
    },
    run(argv: string[]): CtlReply {
      if (argv.includes("--all") || operands(argv).length > 1) return batch(argv);
      const key = argv[1];
      if (!key) return fail("usage", "usage: server update <id|name>");
      const server = find(key);
      if (!server) return fail("not_found", `no server '${key}'`);
      const state = states.get(server.id);
      if (!state) {
        return fail(
          "not_found",
          `${server.name} pins no package version and was not installed from a GitHub release`,
        );
      }
      if (argv.includes("--undo")) {
        const applied = state.applied;
        if (!applied)
          return fail("not_found", `no update of ${server.name} to take back`);
        if (argv.includes("--dry-run")) {
          return {
            ok: true,
            data: {
              plan: {
                server: server.name,
                id: server.id,
                from: applied.to,
                to: applied.from,
                refusal: null,
              },
            },
          };
        }
        state.running = applied.from;
        state.applied = null;
        return done(server.name, server.id, state, applied.to, applied.from, argv, true);
      }
      if (state.seed.offline && argv.includes("--fetch"))
        return fail("lookup_failed", state.seed.offline);
      const current = plan(server.name, server.id, state, argv);
      if (argv.includes("--dry-run")) return { ok: true, data: { plan: current } };
      if (current.refusal) {
        if (current.refusal.code === "up_to_date")
          return {
            ok: true,
            data: {
              applied: false,
              server: server.name,
              id: server.id,
              kind: state.seed.kind,
              package: state.seed.package,
              from: state.running,
              to: state.running,
              changes: [],
              restart: null,
              warnings: [],
              undo: `toolportctl server update ${server.name} --undo`,
            },
          };
        return fail("update_refused", current.refusal.message, {
          reason: current.refusal.code,
        });
      }
      if (state.seed.conflictOnce) {
        state.running = state.seed.conflictOnce;
        state.seed.conflictOnce = undefined;
      }
      const stale = plan(server.name, server.id, state, argv);
      const hash = flagValue(argv, "--plan-hash");
      if (hash && hash !== stale.planHash)
        return fail(
          "conflict",
          "the plan has changed since it was reviewed; review it again",
          { planHash: stale.planHash },
        );
      if (stale.release?.checksum === null && !argv.includes("--allow-unverified"))
        return fail(
          "update_refused",
          "the release publishes no checksum for this file; pass --allow-unverified to install it anyway",
          { reason: "unverified" },
        );
      if (state.seed.applyFails) return fail("update_failed", state.seed.applyFails);
      const from = state.running;
      state.applied = { from, to: stale.to };
      state.running = stale.to;
      state.seed.newer = undefined;
      return done(server.name, server.id, state, from, stale.to, argv, false);
    },
  };
}
