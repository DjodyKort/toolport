import type { CtlEnvelope, CtlResult } from "../bridge/ctl";
import { registerAttentionFeed } from "../shell/attentionFeed";
import { setClock } from "../tasks/clock";
import { FIXTURE_NOW, type TaskWorldOptions } from "./taskWorld";
import {
  fixtureServers,
  type LoginSeed,
  type RestartScript,
  type SignInScript,
} from "./serverWorld";
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

const HOUR = 3_600_000;

/** The three rows of the attention mockup, so the footer bell can be seen in the fixture. */
function registerFixtureBell() {
  const now = Date.now();
  const snapshot = {
    checkedAt: now - 120_000,
    items: [
      {
        id: "secret",
        urgency: "needs-you" as const,
        title: "alpha-api is missing a secret",
        detail: "API_KEY is not set. The server cannot start without it.",
        source: "Servers",
        since: now - HOUR,
      },
      {
        id: "task",
        urgency: "needs-you" as const,
        title: "Task portal-token is waiting for you",
        detail: "It stopped at a step that only you can do.",
        source: "Tasks",
        since: now - 25 * 60_000,
      },
      {
        id: "login",
        urgency: "needs-you" as const,
        title: "beta needs a new sign-in",
        detail: "The login stopped working. Check it again, or sign in once more.",
        source: "Servers",
        since: now - 3 * HOUR,
      },
    ],
  };
  registerAttentionFeed({ snapshot: () => snapshot, subscribe: () => () => {} });
}

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

/** `?plus-bell` shows the footer bell with three rows. `?plus-self=missing|enabled|disabled|opted-out|not-enabled`, `?plus-doctor=warn|fail` and
 * `?plus-bridge=down` pick the state the fixture page starts in. `?plus-restart=busy|partial|none|slow`
 * picks how a server restart answers and `?plus-default=<access set id>` the default access; the servers follow
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
    down: params.get("plus-bridge") === "down",
    servers: {
      servers: fixtureServers(params.has("auth-guidance")),
      restart: restartScripts(params.get("plus-restart")),
      defaultAccess: params.get("plus-default"),
      ...loginWorld(params),
    },
    tasks: tasksFromSearch(params),
  });
  if (params.has("plus-bell")) registerFixtureBell();
}

if (typeof location !== "undefined") plusFixtureFromSearch(location.search);

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

/** The three IPC commands of the fork bridge, answered by the in-memory toolportctl. */
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
    case "secret_status":
      return world.servers.secretStatus(String(args.serverId), args.keys as string[]);
    case "plus_ctl_cancel":
      jobs.delete(String(args.job));
      return null;
    default:
      throw new Error(`Unimplemented fixture command: ${command}`);
  }
}
