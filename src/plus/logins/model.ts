import type { AuthRow, AuthStateName } from "../api";
import type { ServerInfoData, ServerLsData } from "../bridge/data";

/** The vault key of a remote server's bearer token; `secret set` takes it like any other key. */
export const HTTP_AUTH_KEY = "__http_auth__";

export type LoginKind = "oauth" | "token";
export type RowState = AuthStateName | "untracked";
export type Tone = "success" | "warning" | "destructive" | "secondary";

export interface Settled<V> {
  ok: boolean;
  value?: V;
  error?: unknown;
}

export interface LoginRow {
  id: string;
  name: string;
  transport: string;
  kind: LoginKind;
  typeLabel: string;
  /** Env keys the server declares as secrets (names only). */
  secretKeys: string[];
  auth: AuthRow | null;
  state: RowState;
  /** The server can be signed in with `auth login`. */
  canSignIn: boolean;
  /** The server has a secret to set. */
  canSetSecret: boolean;
}

export interface Summary {
  total: number;
  signedIn: number;
  needSignIn: number;
  needNames: string[];
  other: number;
  lastProbe: number | null;
}

const NEEDS_SIGN_IN: ReadonlySet<RowState> = new Set([
  "needs_reauth",
  "revoked",
  "expiring",
]);
const BROKEN: ReadonlySet<RowState> = new Set([
  "needs_reauth",
  "revoked",
  "expiring",
  "misconfigured",
  "unreachable",
]);

export const needsSignIn = (state: RowState) => NEEDS_SIGN_IN.has(state);
export const isBroken = (state: RowState) => BROKEN.has(state);

const LABEL: Record<RowState, string> = {
  ok: "Signed in",
  expiring: "Expiring",
  needs_reauth: "Login needed",
  revoked: "Access revoked",
  misconfigured: "Misconfigured",
  unreachable: "Unreachable",
  unknown: "Not checked yet",
  untracked: "Not checked yet",
};

const TONE: Record<RowState, Tone> = {
  ok: "success",
  expiring: "warning",
  needs_reauth: "warning",
  revoked: "destructive",
  misconfigured: "destructive",
  unreachable: "warning",
  unknown: "secondary",
  untracked: "secondary",
};

export const stateLabel = (state: RowState) => LABEL[state];

const PLAIN_REASONS: Record<string, string> = {
  ok: "The login works.",
  token_expiring: "The token is about to expire.",
  token_expired: "The token has expired.",
  no_token: "No token is stored for this server.",
  no_token_file: "No token is stored for this server.",
  bad_token_file: "The stored token cannot be read.",
  invalid_grant: "The refresh token was rejected, so a new sign-in is needed.",
  revoked: "Access was revoked.",
  token_revoked: "Access was revoked.",
  account_inactive: "The account is inactive.",
  invalid_auth: "The server rejected the token.",
  not_authed: "The server rejected the token.",
  unauthorized: "The server rejected the token.",
  invalidtoken: "The server rejected the token.",
  access_denied: "The server refused access.",
  accessexception: "The server refused the request.",
  missing_config: "The server's address or settings are missing.",
  bad_endpoint: "The server's address is not allowed.",
  invalid_client: "The server no longer accepts the registered client.",
  deleted_client: "The server no longer accepts the registered client.",
  unauthorized_client: "The server no longer accepts the registered client.",
  transient_failures: "The server could not be reached.",
  unknown: "Not checked yet.",
};

/** The reason a probe gives, in words for a person; a reason with no sentence of its own is
 * shown as it is. */
export const plainReason = (reason: string) => PLAIN_REASONS[reason] ?? reason;

/** What one Test of a row shows next to the server: running, the result, or why it could not run. */
export type RowTest =
  | { phase: "running" }
  | { phase: "done"; state: RowState; reason: string; at: number }
  | { phase: "failed"; message: string };

interface ProbedServer {
  server: string;
  state: AuthStateName;
  reason: string;
  lastProbe: number | null;
}

/** The result of `auth probe --server <id> --force` for that one server. */
export function testResultOf(
  data: { servers?: ProbedServer[]; failures?: unknown[] },
  server: string,
  now: number,
): RowTest {
  const failed = (data.failures ?? []).find(
    (entry) => (entry as { server?: unknown }).server === server,
  ) as { error?: unknown } | undefined;
  if (failed) {
    return { phase: "failed", message: String(failed.error ?? "the probe failed") };
  }
  const row = data.servers?.find((entry) => entry.server === server);
  if (!row) return { phase: "failed", message: "the probe returned no result" };
  return {
    phase: "done",
    state: row.state,
    reason: row.reason,
    at: row.lastProbe ?? now,
  };
}
export const stateTone = (state: RowState) => TONE[state];

const SEVERITY: Record<RowState, number> = {
  revoked: 0,
  needs_reauth: 1,
  misconfigured: 2,
  expiring: 3,
  unreachable: 4,
  unknown: 5,
  untracked: 5,
  ok: 6,
};

const STATES = new Set<string>([
  "ok",
  "expiring",
  "needs_reauth",
  "revoked",
  "misconfigured",
  "unreachable",
  "unknown",
]);

function isAuthRow(value: unknown): value is AuthRow {
  if (value === null || typeof value !== "object") return false;
  const row = value as Record<string, unknown>;
  return (
    typeof row.server === "string" &&
    typeof row.state === "string" &&
    STATES.has(row.state) &&
    typeof row.reason === "string"
  );
}

/** The auth rows of a `status` answer; a row that does not have the documented shape is
 * left out instead of breaking the screen. */
export function authRowsOf(servers: unknown): AuthRow[] {
  return Array.isArray(servers) ? servers.filter(isAuthRow) : [];
}

export const isRemote = (transport: string) =>
  transport === "http" || transport === "sse";

const SIGN_IN_FIXES = new Set(["reauth", "reconsent"]);
const SECRET_SET_FIX = /^toolportctl secret set \S+ (\S+)/;

/** The key a token fix names. A remote server declares no secret key, so the probe's own fix
 * ("Set the API token", `secret set <server> <key>`) is what says it signs in with a token. */
function fixTokenKey(auth: AuthRow | null): string | null {
  const fix = auth?.fix;
  if (!fix || fix.action !== "fix_config" || !fix.command) return null;
  const key = SECRET_SET_FIX.exec(fix.command)?.[1];
  return key && key !== "<KEY>" ? key : null;
}

/** The keys Set secret offers: the declared ones, or the bearer token of a remote server
 * that declares none (a sign-in that turned out to need a token). */
export function secretKeysOf(row: Pick<LoginRow, "secretKeys" | "transport">): string[] {
  if (row.secretKeys.length > 0) return row.secretKeys;
  return isRemote(row.transport) ? [HTTP_AUTH_KEY] : [];
}

function classify(
  transport: string,
  secretKeys: string[],
  auth: AuthRow | null,
): Pick<LoginRow, "kind" | "typeLabel" | "canSignIn" | "canSetSecret"> {
  if (secretKeys.length > 0) {
    const ownFlow =
      !isRemote(transport) && !!auth?.fix && SIGN_IN_FIXES.has(auth.fix.action);
    return {
      kind: "token",
      typeLabel: "API token",
      canSignIn: ownFlow,
      canSetSecret: true,
    };
  }
  return {
    kind: "oauth",
    typeLabel: isRemote(transport) ? "OAuth" : "Account",
    canSignIn: true,
    canSetSecret: false,
  };
}

export interface Roster {
  rows: LoginRow[];
  /** Servers that need no login at all (a local server with no secret and no probe). */
  noLogin: number;
}

/** Joins the registry's servers with their auth state and their declared secrets. A server
 * belongs on the Logins list when it has a tracked login, a secret to set or a remote address
 * to sign in to. The CLI does not say which flow a server uses, so the kind is read from what
 * it declares: a secret key means a token, anything else signs in. */
export function buildRoster(
  servers: ServerLsData["servers"],
  infos: Record<string, Settled<ServerInfoData>>,
  authRows: AuthRow[],
): Roster {
  const byServer = new Map(authRows.map((row) => [row.server, row]));
  const used = new Set<string>();
  const rows: LoginRow[] = [];
  let noLogin = 0;
  for (const server of servers) {
    const auth = byServer.get(server.id) ?? byServer.get(server.name) ?? null;
    if (auth) used.add(auth.server);
    const info = infos[server.id];
    const declared =
      info?.ok && info.value
        ? info.value.env.filter((e) => e.secret).map((e) => e.key)
        : [];
    const tokenKey =
      declared.length === 0 && isRemote(server.transport) ? fixTokenKey(auth) : null;
    const secretKeys = tokenKey ? [tokenKey] : declared;
    if (!auth && secretKeys.length === 0 && !isRemote(server.transport)) {
      noLogin += 1;
      continue;
    }
    rows.push({
      id: server.id,
      name: server.name,
      transport: server.transport,
      secretKeys,
      auth,
      state: auth?.state ?? "untracked",
      ...classify(server.transport, secretKeys, auth),
    });
  }
  for (const auth of authRows) {
    if (used.has(auth.server)) continue;
    rows.push({
      id: auth.server,
      name: auth.server,
      transport: "unknown",
      secretKeys: [],
      auth,
      state: auth.state,
      ...classify("unknown", [], auth),
    });
  }
  rows.sort(
    (a, b) =>
      Number(isBroken(b.state)) - Number(isBroken(a.state)) ||
      Number(b.kind === "oauth") - Number(a.kind === "oauth") ||
      SEVERITY[a.state] - SEVERITY[b.state] ||
      a.name.localeCompare(b.name),
  );
  return { rows, noLogin };
}

export interface Countable {
  name: string;
  state: RowState;
  lastProbe: number | null;
}

export const countableLogins = (rows: LoginRow[]): Countable[] =>
  rows.map((row) => ({
    name: row.name,
    state: row.state,
    lastProbe: row.auth?.lastProbe ?? null,
  }));

export const countableAuth = (rows: AuthRow[]): Countable[] =>
  rows.map((row) => ({ name: row.server, state: row.state, lastProbe: row.lastProbe }));

export function summarize(items: Countable[]): Summary {
  const need = items.filter((item) => needsSignIn(item.state));
  const times = items.flatMap((item) => (item.lastProbe ? [item.lastProbe] : []));
  return {
    total: items.length,
    signedIn: items.filter((item) => item.state === "ok").length,
    needSignIn: need.length,
    needNames: need.map((item) => item.name),
    other: items.filter((item) => isBroken(item.state) && !needsSignIn(item.state))
      .length,
    lastProbe: times.length > 0 ? Math.max(...times) : null,
  };
}

/** `HH:MM` for today and a short date with the time for an older probe. */
export function formatWhen(seconds: number, now: number = Date.now()): string {
  const when = new Date(seconds * 1000);
  const today = new Date(now);
  const sameDay =
    when.getFullYear() === today.getFullYear() &&
    when.getMonth() === today.getMonth() &&
    when.getDate() === today.getDate();
  const time = { hour: "2-digit", minute: "2-digit" } as const;
  return sameDay
    ? when.toLocaleTimeString("en-GB", time)
    : when.toLocaleString("en-GB", { day: "numeric", month: "short", ...time });
}

export function isoWhen(seconds: number): string {
  return new Date(seconds * 1000).toISOString();
}

export function expiresText(row: AuthRow): string | null {
  if (row.ttlSecs === null || row.ttlSecs === undefined) return null;
  if (row.ttlSecs <= 0) return "expired";
  const minutes = Math.ceil(row.ttlSecs / 60);
  return minutes >= 120
    ? `expires in ${Math.round(minutes / 60)} h`
    : `expires in ${minutes} min`;
}

export const STATUSLINE_SNIPPET = `"statusLine": { "type": "command", "command": "toolportctl auth statusline" }`;

export const HOOK_SNIPPET = `"hooks": {
  "SessionStart": [
    { "hooks": [{ "type": "command", "command": "toolportctl auth hook" }] }
  ]
}`;

/** Every secret key the registry declares, grouped by server for the Secrets tab. */
export function secretRows(
  servers: ServerLsData["servers"],
  infos: Record<string, Settled<ServerInfoData>>,
): Array<{ id: string; name: string; keys: string[] }> {
  return servers
    .flatMap((server) => {
      const info = infos[server.id];
      const keys =
        info?.ok && info.value
          ? info.value.env.filter((e) => e.secret).map((e) => e.key)
          : [];
      return keys.length > 0 ? [{ id: server.id, name: server.name, keys }] : [];
    })
    .sort((a, b) => a.name.localeCompare(b.name));
}

export const presenceKey = (server: string, key: string) => `${server}\u0000${key}`;

export type Presence = "set" | "unset" | "error";

export const LOGIN_TABS = [
  { id: "logins", label: "Logins" },
  { id: "secrets", label: "Secrets" },
  { id: "integrations", label: "Integrations" },
] as const;

export type LoginTabId = (typeof LOGIN_TABS)[number]["id"];

export function loginArgv(server: string, noOpen: boolean): string[] {
  return ["auth", "login", server, ...(noOpen ? ["--no-open"] : [])];
}
