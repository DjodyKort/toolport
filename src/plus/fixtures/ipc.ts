import type { CtlEnvelope, CtlResult } from "../bridge/ctl";
import { setClock } from "../tasks/clock";
import type { HooksScenario, MeasureScenario } from "./compressionWorld";
import { FIXTURE_NOW, type TaskWorldOptions } from "./taskWorld";
import {
  fixtureServers,
  type LoginSeed,
  type RestartScript,
  type SignInScript,
} from "./serverWorld";
import type { LibraryAuto, LibraryScenario, LibraryWorldOptions } from "./libraryWorld";
import {
  createCtlWorld,
  type CtlReply,
  type CtlWorld,
  type WorldOptions,
} from "./ctlWorld";

interface Job {
  argv: string[];
  stdin: string | undefined;
}

let world: CtlWorld = createCtlWorld();
let down = false;
const jobs = new Map<string, Job>();
let counter = 0;

/** Starts a fresh in-memory toolportctl. `down` makes every start fail the way a build without the
 * bundled toolportctl does. */
export function resetPlusFixture(options: WorldOptions & { down?: boolean } = {}) {
  world = createCtlWorld(options);
  down = options.down === true;
  jobs.clear();
}

/** Keeps the app fixture's access sets and the fork's world as one state: an upstream
 * `set_access_server` call (`write`) goes into the world, and the registry's memberships of the
 * world's servers then follow the world, so what toolportctl wrote shows in upstream's reads. */
export function plusFixtureAccess(
  registry: { profiles: Array<{ id: string; enabledServerIds: string[] }> },
  write?: Record<string, unknown>,
) {
  if (write)
    world.servers.setAccessServer(
      String(write.profileId),
      String(write.serverId),
      write.included === true,
    );
  const known = new Set(world.servers.servers.map((s) => s.id));
  for (const { id, serverIds } of world.servers.sets) {
    const profile = registry.profiles.find((p) => p.id === id);
    if (profile)
      profile.enabledServerIds = [
        ...profile.enabledServerIds.filter((serverId) => !known.has(serverId)),
        ...serverIds,
      ];
  }
}

const LIBRARY_SCENARIOS = [
  "plan",
  "done",
  "held",
  "blocked",
  "behind",
  "empty",
  "catchup",
  "busy",
];

const LIBRARY_AUTO = ["on", "held", "failed", "paused"];

const RESTART_SCENARIOS: Record<string, RestartScript> = {
  busy: { kind: "busy" },
  partial: { kind: "partial" },
  none: { kind: "none" },
  slow: { kind: "ok", delayMs: 6000 },
};

const LOGIN_SCENARIOS: Record<string, Record<string, LoginSeed>> = {
  ok: { "fixture-1": { state: "ok", checkedAgo: 300, fixedBy: "secret" } },
  rejected: {
    "fixture-1": {
      state: "needs_reauth",
      reason: "the key was rejected with HTTP 401 on the last 3 checks",
      checkedAgo: 240,
      sinceAgo: 26 * 3600,
      fixedBy: "secret",
    },
    "fixture-2": {
      state: "revoked",
      reason: "access was revoked at the provider",
      checkedAgo: 600,
      sinceAgo: 3 * 3600,
      fixedBy: "sign-in",
    },
  },
};

/** `?plus-login=ok|rejected` gives servers a login check, `?plus-signin=wait` makes a sign-in wait
 * on the browser and `?plus-vault=locked|empty` changes the keychain. */
function loginWorld(params: URLSearchParams) {
  const seeds = LOGIN_SCENARIOS[params.get("plus-login") ?? ""];
  const signIn: Record<string, SignInScript> =
    params.get("plus-signin") === "wait"
      ? { "fixture-1": { kind: "consent", delayMs: 600_000 } }
      : {};
  const vault =
    params.get("plus-vault") === "empty" || !seeds ? [] : ["fixture-1/API_KEY"];
  return {
    logins: seeds ?? {},
    signIn,
    vault,
    vaultLocked: params.get("plus-vault") === "locked",
  };
}

const HOOK_SCENARIOS = [
  "adopt",
  "on",
  "paused",
  "paused-open",
  "missing",
  "edited",
  "none",
];

/** `?plus-bell` shows the footer bell (`attentionFromSearch`). `?plus-self=missing|enabled|disabled|opted-out|not-enabled`, `?plus-doctor=warn|fail` and
 * `?plus-bridge=down` pick the state the fixture page starts in. `?plus-restart=busy|partial|none|slow`
 * picks how a server restart answers, `?plus-measured=fresh|stale`, `?plus-measure=slow|fail` and `?plus-bundles=none` set the Context screen's world,
 * `?plus-compression=adopt|on|paused|paused-open|missing|edited|none` the hooks of its Hooks & compression tab and
 * `?plus-measurements=compared|empty` its measurements, and `?plus-default=<access set id>` the default access; the servers follow
 * `?auth-guidance` of the app fixture. */
export function plusFixtureFromSearch(search: string) {
  const params = new URLSearchParams(search);
  const self = params.get("plus-self");
  const doctor = params.get("plus-doctor");
  resetPlusFixture({
    self: ["missing", "enabled", "disabled", "opted-out", "not-enabled"].includes(
      self ?? "",
    )
      ? (self as WorldOptions["self"])
      : "enabled",
    doctor: doctor === "warn" || doctor === "fail" ? doctor : "ok",
    context: {
      measured: ["fresh", "stale"].includes(params.get("plus-measured") ?? "")
        ? (params.get("plus-measured") as "fresh" | "stale")
        : "none",
      measure: ["slow", "fail"].includes(params.get("plus-measure") ?? "")
        ? (params.get("plus-measure") as "slow" | "fail")
        : "ok",
      bundles: params.get("plus-bundles") !== "none",
    },
    compression: {
      hooks: HOOK_SCENARIOS.includes(params.get("plus-compression") ?? "")
        ? (params.get("plus-compression") as HooksScenario)
        : "adopt",
      verify: params.get("plus-verify") === "fail" ? "fail" : "pass",
      measure: ["compared", "empty"].includes(params.get("plus-measurements") ?? "")
        ? (params.get("plus-measurements") as MeasureScenario)
        : "refused",
    },
    down: params.get("plus-bridge") === "down",
    servers: {
      servers: fixtureServers(params.has("auth-guidance")),
      restart: restartScripts(params.get("plus-restart")),
      defaultAccess: params.get("plus-default"),
      ...loginWorld(params),
    },
    tasks: tasksFromSearch(params),
    library: libraryFromSearch(params),
    attention: attentionFromSearch(params),
  });
}

if (typeof location !== "undefined") plusFixtureFromSearch(location.search);

/** `?plus-bell` shows the footer bell with five rows, `?plus-bell=clear` with none and
 * `?plus-bell=unchecked` with one area that could not be read; without it `attention ls` fails and
 * the bell stays out. */
function attentionFromSearch(params: URLSearchParams): WorldOptions["attention"] {
  if (!params.has("plus-bell")) return {};
  const scenario = params.get("plus-bell");
  return {
    scenario: scenario === "clear" || scenario === "unchecked" ? scenario : "busy",
  };
}

/** `?plus-library=plan|done|held|blocked|behind|empty|catchup|busy` picks the state of the Library
 * screen; `plus-library-chmod`, `-drift`, `-secret` and `-dirty` add one more thing that is wrong;
 * `plus-library-auto=on|held|failed|paused` sets where auto-sync stands (off without it). */
function libraryFromSearch(params: URLSearchParams): LibraryWorldOptions {
  const scenario = params.get("plus-library") ?? "";
  const auto = params.get("plus-library-auto") ?? "";
  return {
    scenario: LIBRARY_SCENARIOS.includes(scenario)
      ? (scenario as LibraryScenario)
      : "plan",
    chmod: params.has("plus-library-chmod"),
    drift: Number(params.get("plus-library-drift") ?? 0),
    secret: params.has("plus-library-secret"),
    dirty: params.has("plus-library-dirty"),
    auto: LIBRARY_AUTO.includes(auto) ? (auto as LibraryAuto) : "off",
  };
}

/** `?plus-tasks=morning` shows the tasks of the design at its fixed time (Tuesday 09:58 in Amsterdam),
 * `empty` shows none and `edge` adds a 1.x routine task and an unreadable task file;
 * `?plus-tasks-live` keeps a started run live for a minute. */
function tasksFromSearch(params: URLSearchParams): TaskWorldOptions {
  const scenario = params.get("plus-tasks");
  if (scenario === null) return {};
  setClock(FIXTURE_NOW);
  return {
    scenario: scenario === "empty" || scenario === "edge" ? scenario : "default",
    stepMs: params.has("plus-tasks-live") ? 60_000 : undefined,
  };
}

function restartScripts(scenario: string | null): Record<string, RestartScript> {
  const script = RESTART_SCENARIOS[scenario ?? ""];
  return script ? { "fixture-0": script, "fixture-1": script, "fixture-2": script } : {};
}

function envelopeOf(command: string, reply: CtlReply): CtlEnvelope {
  return reply.ok
    ? { ok: true, command, schemaVersion: 1, data: reply.data }
    : {
        ok: false,
        command,
        schemaVersion: 1,
        data: reply.data,
        error: reply.error,
      };
}

/** Answers at once, or after the reply's delay when it has one (a restart that waits on gateways). */
function resultOf(job: string, run: Job): CtlResult | Promise<CtlResult> {
  const reply = world.run(run.argv, run.stdin);
  const result: CtlResult = {
    job,
    exitCode: reply.ok ? 0 : 1,
    signal: null,
    cancelled: false,
    envelope: envelopeOf(run.argv[0], reply),
    parseError: null,
    stderr: reply.stderr ?? [],
    truncated: false,
  };
  const answer = () =>
    reply.delayMs
      ? new Promise<CtlResult>((resolve) =>
          setTimeout(() => resolve(result), reply.delayMs),
        )
      : result;
  if (!reply.stderr?.length) return answer();
  // Imported here: ctl.ts imports the mocked tauri module, whose factory imports this file.
  return import("../bridge/ctl").then(({ route }) => {
    reply.stderr?.forEach((line, seq) => route({ job, seq, kind: "stderr", line }));
    return answer();
  });
}

/** Upstream's secret commands, answered by the same vault the `secret` verbs use. */
function secretCall(argv: string[], value?: unknown) {
  const reply = world.run(argv, value === undefined ? undefined : String(value));
  if (!reply.ok) throw new Error(reply.error?.message ?? "the secret command failed");
  return world.servers.registry();
}

/** The IPC commands of the fork bridge, answered by the in-memory toolportctl, and the secret
 * commands upstream's Secrets dialog calls. */
export function plusFixtureIpc(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case "plus_ctl": {
      if (down)
        throw new Error("toolportctl not found: no toolportctl next to the Toolport app");
      const job = `fixture-job-${++counter}`;
      jobs.set(job, {
        argv: args.argv as string[],
        stdin: (args.stdinSecret as string | null | undefined) ?? undefined,
      });
      return job;
    }
    case "plus_ctl_result": {
      const job = String(args.job);
      const run = jobs.get(job);
      if (!run) throw new Error(`unknown job: ${job}`);
      jobs.delete(job);
      return resultOf(job, run);
    }
    case "skills_inventory":
      return world.library.inventory();
    case "secret_status":
      return world.servers.secretStatus(String(args.serverId), args.keys as string[]);
    case "set_secret":
      return secretCall(
        ["secret", "set", String(args.serverId), String(args.key)],
        args.value,
      );
    case "delete_secret":
      return secretCall(["secret", "rm", String(args.serverId), String(args.key)]);
    case "plus_ctl_cancel":
      jobs.delete(String(args.job));
      return null;
    default:
      throw new Error(`Unimplemented fixture command: ${command}`);
  }
}
