import type { AuthRow } from "../api";
import type {
  ClientLsData,
  CommandRow,
  DoctorData,
  ProfileLsData,
  ServerInfoData,
  ServerLsData,
  StatusData,
} from "../bridge/data";
import type { ClientDirectLsData } from "../types/client";

/** `status` as the CLI prints it. `directEntries` only appears once a client has one. */
export type StatusDoc = Omit<StatusData, "auth"> & {
  auth: { counts: StatusData["auth"]["counts"]; servers: AuthRow[] };
  directEntries?: number;
};

export type ServerRowData = ServerLsData["servers"][number];
export type ProfileData = ProfileLsData["profiles"][number];
export type ClientData = ClientLsData["clients"][number];
export type DirectEntry = ClientDirectLsData["entries"][number];
export type DoctorCheck = DoctorData["checks"][number];
export type Tier = "read" | "write" | "destructive";

export type ServerState =
  "connected" | "login" | "failed" | "starting" | "idle" | "disabled";

export type ServerGroup = "attention" | "connected" | "waiting";

export interface ServerView {
  id: string;
  name: string;
  transport: string;
  /** In the active profile. */
  enabled: boolean;
  state: ServerState;
  /** Why it needs attention, in the CLI's own words where it has them. */
  reason: string | null;
  /** Tools the gateway reported for it, `null` while it has not connected. */
  tools: number | null;
  /** A login that still works but is about to end. */
  expiring: boolean;
  authState: string | null;
  profiles: Array<{ id: string; name: string }>;
}

const LOGIN_STATES = ["needs_reauth", "revoked"];

type BuildServer = { id: string; state: string; tools: number };

/** The best answer any running gateway has for each server: connected beats connecting beats
 * failed, so a second gateway that has not reached it yet does not hide the first. */
export function gatewayServers(status: StatusDoc): Map<string, BuildServer> {
  const best = new Map<string, BuildServer>();
  const rank = (state: string) =>
    state === "connected" ? 2 : state === "connecting" ? 1 : 0;
  const builds = status.gateway.builds.length
    ? status.gateway.builds
    : status.gateway.build
      ? [status.gateway.build]
      : [];
  for (const build of builds) {
    for (const server of build.servers) {
      const seen = best.get(server.id);
      if (!seen || rank(server.state) > rank(seen.state)) best.set(server.id, server);
    }
  }
  return best;
}

function authFor(status: StatusDoc, server: ServerRowData): AuthRow | null {
  const rows = status.auth.servers;
  return (
    rows.find((row) => row.server === server.id) ??
    rows.find((row) => row.server.toLowerCase() === server.name.toLowerCase()) ??
    null
  );
}

const AUTH_WORDS: Record<string, string> = {
  needs_reauth: "Sign-in needed",
  revoked: "Access was revoked",
  misconfigured: "The sign-in setup is wrong",
  unreachable: "The server did not answer",
  expiring: "The login is about to expire",
};

function reasonOf(auth: AuthRow): string {
  const head = AUTH_WORDS[auth.state] ?? auth.state;
  return auth.reason && auth.reason !== auth.state ? `${head}: ${auth.reason}` : head;
}

export function buildServerViews(
  ls: ServerLsData,
  profiles: ProfileLsData,
  status: StatusDoc,
): ServerView[] {
  const live = gatewayServers(status);
  return ls.servers.map((server) => {
    const auth = authFor(status, server);
    const gateway = live.get(server.id);
    const profileRefs = profiles.profiles
      .filter((profile) => profile.servers.some((member) => member.id === server.id))
      .map((profile) => ({ id: profile.id, name: profile.name }));
    const base = {
      id: server.id,
      name: server.name,
      transport: server.transport,
      enabled: server.enabled,
      authState: auth?.state ?? null,
      profiles: profileRefs,
      tools: gateway?.state === "connected" ? gateway.tools : null,
      expiring: auth?.state === "expiring",
    };
    if (!server.enabled) {
      return { ...base, state: "disabled", reason: null };
    }
    if (auth && LOGIN_STATES.includes(auth.state)) {
      return { ...base, state: "login", reason: reasonOf(auth) };
    }
    if (gateway?.state === "failed") {
      return {
        ...base,
        state: "failed",
        reason: auth ? reasonOf(auth) : "The gateway could not start it",
      };
    }
    if (auth && (auth.state === "misconfigured" || auth.state === "unreachable")) {
      return { ...base, state: "failed", reason: reasonOf(auth) };
    }
    if (gateway?.state === "connected") {
      return { ...base, state: "connected", reason: null };
    }
    if (gateway?.state === "connecting") {
      return { ...base, state: "starting", reason: null };
    }
    return { ...base, state: "idle", reason: null };
  });
}

export function groupOf(view: ServerView): ServerGroup {
  if (view.state === "login" || view.state === "failed" || view.expiring)
    return "attention";
  return view.state === "connected" ? "connected" : "waiting";
}

export const GROUP_TITLES: Record<ServerGroup, string> = {
  attention: "Needs attention",
  connected: "Connected",
  waiting: "Not running",
};

export function groupServers(
  views: ServerView[],
  query = "",
): Array<{ group: ServerGroup; servers: ServerView[] }> {
  const needle = query.trim().toLowerCase();
  const shown = needle
    ? views.filter(
        (view) =>
          view.name.toLowerCase().includes(needle) ||
          view.transport.toLowerCase().includes(needle),
      )
    : views;
  return (["attention", "connected", "waiting"] as const)
    .map((group) => ({ group, servers: shown.filter((view) => groupOf(view) === group) }))
    .filter((entry) => entry.servers.length > 0);
}

export function stateLabel(view: Pick<ServerView, "state" | "expiring">): string {
  if (view.state === "connected") return view.expiring ? "Login expiring" : "Connected";
  return {
    login: "Login needed",
    failed: "Failed",
    starting: "Starting",
    idle: "Not started",
    disabled: "Not in profile",
    connected: "Connected",
  }[view.state];
}

export type Tone = "success" | "warning" | "destructive" | "secondary" | "info";

export function stateTone(view: Pick<ServerView, "state" | "expiring">): Tone {
  switch (view.state) {
    case "connected":
      return view.expiring ? "warning" : "success";
    case "login":
      return "warning";
    case "failed":
      return "destructive";
    case "starting":
      return "info";
    default:
      return "secondary";
  }
}

export function serverSummary(view: ServerView): string {
  if (view.reason) return view.reason;
  if (view.state === "connected")
    return `${view.tools ?? 0} tool${view.tools === 1 ? "" : "s"}`;
  if (view.state === "disabled") return "Not in the active profile";
  if (view.state === "starting") return "Connecting";
  return "No tools yet";
}

export function initials(name: string): string {
  return (
    name
      .replace(/[^A-Za-z0-9]/g, "")
      .slice(0, 2)
      .toUpperCase() || "?"
  );
}

export interface StripFact {
  label: string;
  value: string;
  detail: string;
  tone: "success" | "warning" | "destructive" | "muted";
}

/** The four facts of the strip above the tabs: gateway, tool catalog, logins, clients. */
export function stripFacts(status: StatusDoc, clients: ClientData[] | null): StripFact[] {
  const build = status.gateway.build;
  const running = status.gateway.builds.length > 0;
  const gateway: StripFact = !status.gateway.present
    ? {
        label: "Gateway",
        value: "Binary missing",
        detail: "Run doctor to see where Toolport looked",
        tone: "destructive",
      }
    : !running
      ? {
          label: "Gateway",
          value: "Not running",
          detail: "It starts when a client connects",
          tone: "muted",
        }
      : build?.building
        ? {
            label: "Gateway",
            value: "Starting",
            detail: `${build.serversConnected} of ${build.serversTotal} servers connected`,
            tone: "warning",
          }
        : {
            label: "Gateway",
            value: "Running",
            detail: `${build?.role ?? "daemon"}, pid ${build?.pid ?? "?"}, v${status.version}`,
            tone: "success",
          };
  const catalog: StripFact = build
    ? {
        label: "Catalog",
        value: `${build.toolsSoFar.toLocaleString("en")} tools`,
        detail: `from ${build.serversConnected} of ${build.serversTotal} servers${
          build.serversFailed > 0 ? `, ${build.serversFailed} failed` : ""
        }`,
        tone: build.serversFailed > 0 ? "warning" : "muted",
      }
    : {
        label: "Catalog",
        value: "No tool cache",
        detail: `${status.serverCount} servers registered`,
        tone: "muted",
      };
  const needLogin = status.auth.servers.filter((row) => LOGIN_STATES.includes(row.state));
  const logins: StripFact =
    needLogin.length > 0
      ? {
          label: "Logins",
          value: `${needLogin.length} need a sign-in`,
          detail: needLogin
            .slice(0, 4)
            .map((row) => row.server)
            .join(", "),
          tone: "warning",
        }
      : {
          label: "Logins",
          value: "All signed in",
          detail: `${status.auth.counts.ok} checked`,
          tone: "success",
        };
  const managed =
    clients?.filter((client) => client.gateway === "managed").length ?? null;
  const direct = status.directEntries ?? 0;
  const clientFact: StripFact = {
    label: "Clients",
    value: managed === null ? "Reading" : `${managed} managed`,
    detail:
      clients === null
        ? ""
        : `${clients.length} detected${direct > 0 ? `, ${direct} direct entries` : ""}`,
    tone: "muted",
  };
  return [gateway, catalog, logins, clientFact];
}

export type Policy = {
  id: string;
  tier: Tier;
  previewFlag: string | null;
  /** Starts a process with inherited stdio: the app can only show its command line. */
  terminal: boolean;
};

/** What the registry says about a command: its tier and the flag that previews it. A command
 * the registry does not have, or that it marks as planned, has no policy: the app does not run
 * what it cannot classify. */
export function policyOf(rows: CommandRow[] | null, id: string): Policy | null {
  const row = rows?.find(
    (candidate) => candidate.kind === "command" && candidate.id === id,
  );
  if (!row || row.planned || !row.tier) return null;
  const preview = row.preview?.mode === "flag" ? row.preview.flag : null;
  const terminal = row.surface === "terminal" || row.needs.includes("terminal-only");
  return { id, tier: row.tier, previewFlag: preview, terminal };
}

export interface ClientView {
  id: string;
  name: string;
  path: string;
  gateway: string;
  /** The profile id the client is scoped to, or `null` when it follows the active one. */
  scope: string | null;
  profile: ProfileData | null;
  followsActive: boolean;
  /** Entries of the client's own config that Toolport did not write and no server owns. */
  orphans: string[];
  /** Entries that duplicate a registered server; sync takes them out. */
  redundant: string[];
  launchers: DirectEntry[];
  seen: Array<{ id: string; name: string; tools: number | null; state: ServerState }>;
  tools: number;
  connectedServers: number;
}

const isRegistered = (servers: ServerView[], entry: string) =>
  servers.some(
    (server) =>
      server.id.toLowerCase() === entry.toLowerCase() ||
      server.name.toLowerCase() === entry.toLowerCase(),
  );

export function buildClientViews(
  clients: ClientData[],
  profiles: ProfileLsData,
  servers: ServerView[],
): ClientView[] {
  const active = profiles.profiles.find(
    (profile) => profile.id === profiles.activeProfile,
  );
  return clients.map((client) => {
    const scoped = client.scope
      ? (profiles.profiles.find(
          (profile) =>
            profile.id === client.scope ||
            profile.name.toLowerCase() === client.scope?.toLowerCase(),
        ) ?? null)
      : null;
    const profile = scoped ?? (client.scope ? null : (active ?? null));
    const seen = (profile?.servers ?? []).map((member) => {
      const view = servers.find((candidate) => candidate.id === member.id);
      return {
        id: member.id,
        name: member.name,
        tools: view?.tools ?? null,
        state: view?.state ?? ("idle" as ServerState),
      };
    });
    return {
      id: client.id,
      name: client.name,
      path: client.path,
      gateway: client.gateway,
      scope: client.scope,
      profile,
      followsActive: !client.scope,
      orphans: client.entries.filter((entry) => !isRegistered(servers, entry)),
      redundant: client.entries.filter((entry) => isRegistered(servers, entry)),
      launchers: client.launchers as DirectEntry[],
      seen,
      tools: seen.reduce((sum, row) => sum + (row.tools ?? 0), 0),
      connectedServers: seen.filter((row) => row.state === "connected").length,
    };
  });
}

export function profileUsers(
  profile: ProfileData,
  active: boolean,
  clients: ClientData[] | null,
): string[] {
  const named = profile.clients.map(String);
  if (!active || !clients) return named;
  const following = clients
    .filter((client) => !client.scope && client.gateway === "managed")
    .map((client) => client.id);
  return [...new Set([...named, ...following])];
}

/** Server info is only about definitions: names of env keys, never values. */
export type ServerInfo = ServerInfoData;

export function launchLine(info: ServerInfo): string {
  if (info.url) return info.url;
  const words = [info.command ?? "", ...info.args].filter(Boolean);
  return words.join(" ") || "no launch command";
}

/** The fix the CLI names inside a doctor line: "...; run toolportctl skills sync". */
export function namedFix(check: DoctorCheck): string[] | null {
  const found =
    /;?\s*run toolportctl ([a-z][a-z0-9-]*(?: [a-z][a-z0-9-]*){0,2})\s*$/i.exec(
      check.detail,
    );
  return found ? found[1].trim().split(/\s+/) : null;
}

/** An error from a server that says it wants a login, as opposed to a plain failure. */
export function looksLikeLogin(message: string): boolean {
  return /\b401\b|unauthori[sz]ed|invalid_token|authenticat|sign[- ]?in|log[- ]?in|oauth/i.test(
    message,
  );
}
