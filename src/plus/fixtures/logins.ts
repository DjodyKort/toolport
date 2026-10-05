import type { AuthRow } from "../api";
import type { ServerInfoData, ServerLsData, StatusData } from "../bridge/data";
import { CtlReplyHeld, ctlReplyFailure } from "./ctlReply";
import { loginTasks } from "../tasks/fixtures";

/** A registry for the Logins & secrets screen: two OAuth servers that answer 401, two that are
 * signed in, a server with API-token secrets (one stored, one not), a server whose token is
 * stored, and local servers that need no login. The names are made up; every value has the
 * shape of the real `toolportctl --json` output (`logins.test.ts` checks that against the
 * shapes of the golden envelopes). A secret value is never in `data`, except the one
 * placeholder that `secret get --reveal` returns. */

const SINCE = 1_790_000_000;
const PROBED = 1_790_000_300;

const row = (
  server: string,
  state: AuthRow["state"],
  reason: string,
  fix: AuthRow["fix"],
  over: Partial<AuthRow> = {},
): AuthRow => ({
  server,
  state,
  reason,
  since: SINCE,
  expiresAt: null,
  ttlSecs: null,
  lastProbe: PROBED,
  fix,
  ...over,
});

const reauth = (server: string): AuthRow["fix"] => ({
  action: "reauth",
  server,
  label: `Sign in to ${server} again`,
  command: `toolportctl auth login ${server}`,
  ipc: null,
});

export const loginsAuthRows: AuthRow[] = [
  row("srv-issues", "needs_reauth", "HTTP 401 invalid_token", reauth("srv-issues")),
  row("srv-design", "needs_reauth", "HTTP 401 Unauthorized", reauth("srv-design")),
  row("srv-corp", "ok", "ok", null),
  row("srv-wiki", "ok", "ok", null),
  row("srv-mail", "ok", "ok", null),
  row("srv-erp", "ok", "ok", null),
];

export const loginsServerLs: ServerLsData = {
  activeProfile: "default",
  servers: [
    { id: "srv-docs", name: "docs-search", transport: "stdio", enabled: true },
    { id: "srv-corp", name: "corp-tools", transport: "http", enabled: true },
    { id: "srv-wiki", name: "wiki-reader", transport: "http", enabled: true },
    { id: "srv-mail", name: "mail-bridge", transport: "stdio", enabled: true },
    { id: "srv-issues", name: "issue-tracker", transport: "http", enabled: true },
    { id: "srv-design", name: "design-files", transport: "http", enabled: true },
    { id: "srv-erp", name: "acme-erp", transport: "stdio", enabled: true },
    { id: "srv-notes", name: "scratch-notes", transport: "stdio", enabled: false },
  ],
};

const info = (
  id: string,
  over: Partial<ServerInfoData> & Pick<ServerInfoData, "name" | "transport">,
): ServerInfoData => ({
  id,
  command: over.transport === "stdio" ? "node" : null,
  args: over.transport === "stdio" ? ["server.js"] : [],
  url: over.transport === "http" ? "https://example.test/mcp" : null,
  cwd: null,
  source: null,
  env: [],
  disabledTools: [],
  declareClientCapabilities: false,
  forwardInstructions: false,
  profiles: ["default"],
  ...over,
});

export const loginsServerInfos: Record<string, ServerInfoData> = {
  "srv-docs": info("srv-docs", { name: "docs-search", transport: "stdio" }),
  "srv-corp": info("srv-corp", { name: "corp-tools", transport: "http" }),
  "srv-wiki": info("srv-wiki", { name: "wiki-reader", transport: "http" }),
  "srv-mail": info("srv-mail", {
    name: "mail-bridge",
    transport: "stdio",
    env: [{ key: "MAIL_TOKEN", secret: true }],
  }),
  "srv-issues": info("srv-issues", { name: "issue-tracker", transport: "http" }),
  "srv-design": info("srv-design", { name: "design-files", transport: "http" }),
  "srv-erp": info("srv-erp", {
    name: "acme-erp",
    transport: "stdio",
    env: [
      { key: "ERP_API_KEY", secret: true },
      { key: "ERP_WEBHOOK_SECRET", secret: true },
      { key: "ERP_BASE_URL", secret: false },
    ],
  }),
  "srv-notes": info("srv-notes", { name: "scratch-notes", transport: "stdio" }),
};

const counts = (rows: AuthRow[]) => {
  const tally = {
    ok: 0,
    expiring: 0,
    needs_reauth: 0,
    revoked: 0,
    misconfigured: 0,
    unreachable: 0,
    unknown: 0,
  };
  for (const entry of rows) tally[entry.state] += 1;
  return tally;
};

export const loginsStatus: StatusData = {
  version: "0.0.0-fixture",
  dataDir: "/fixture/data",
  registry: {
    path: "/fixture/data/registry.json",
    exists: true,
    readable: true,
    error: null,
  },
  serverCount: loginsServerLs.servers.length,
  profileCount: 1,
  activeProfile: "default",
  secretsBackend: "encrypted-file",
  auth: { counts: counts(loginsAuthRows), servers: loginsAuthRows },
  gateway: {
    present: true,
    path: "/fixture/bin/toolport-gateway",
    build: null,
    builds: [],
  },
};

export const statusFor = (rows: AuthRow[]): StatusData => ({
  ...loginsStatus,
  auth: { counts: counts(rows), servers: rows },
});

const worst = (rows: AuthRow[]) =>
  rows.filter((entry) => entry.fix !== null).map((entry) => entry.server);

export const statuslineOf = (rows: AuthRow[]) => {
  const tally = counts(rows);
  const names = worst(rows).slice(0, 3);
  const parts = [
    [tally.revoked, "revoked"],
    [tally.needs_reauth, "need re-auth"],
    [tally.misconfigured, "misconfigured"],
    [tally.expiring, "expiring"],
    [tally.unreachable, "unreachable"],
  ]
    .filter(([n]) => (n as number) > 0)
    .map(([n, label]) => `${n} ${label}`);
  return {
    auth: {
      ok: tally.ok,
      expiring: tally.expiring,
      needs_reauth: tally.needs_reauth,
      revoked: tally.revoked,
      misconfigured: tally.misconfigured,
      unreachable: tally.unreachable,
      worst: names,
      text:
        parts.length === 0
          ? `auth ok (${tally.ok})`
          : `auth: ${parts.join(", ")} [${names.join(", ")}]`,
    },
  };
};

export const hookOf = (rows: AuthRow[]) => {
  const base = statuslineOf(rows);
  const issues = rows.filter((entry) => entry.fix !== null);
  if (issues.length === 0) return base;
  return {
    ...base,
    hookSpecificOutput: {
      hookEventName: "SessionStart",
      additionalContext: `${base.auth.text}\n${issues
        .map((entry) => `${entry.server}: ${entry.state} (${entry.fix!.label})`)
        .join("\n")}`,
    },
  };
};

export const loginsStatusline = statuslineOf(loginsAuthRows);
export const loginsHook = hookOf(loginsAuthRows);

/** A world where every login works: the statusline, the hook and the rows say so. */
export const signedInRows: AuthRow[] = loginsAuthRows.map((entry) =>
  entry.fix ? { ...entry, state: "ok", reason: "ok", fix: null } : entry,
);

const probeOf = (
  rows: AuthRow[],
  mode: string,
  ran: string[],
  cached: string[] = [],
) => ({
  mode,
  probes: [...ran, ...cached].map((server) => ({
    server,
    ran: ran.includes(server),
    skipped: ran.includes(server) ? null : 120,
    nextDueAt: SINCE + 3600,
    tracked: {
      state: rows.find((entry) => entry.server === server)?.state ?? "unknown",
      reason: rows.find((entry) => entry.server === server)?.reason ?? "ok",
      since: SINCE,
      transient: null,
    },
  })),
  failures: [],
  counts: counts(rows.filter((entry) => ran.includes(entry.server))),
  servers: rows.filter((entry) => ran.includes(entry.server)),
});

const loginReport = (server: string, name: string, message: string) => ({
  server,
  name,
  flow: "browser",
  consentUrl: null,
  signedIn: true,
  message,
  probe: null,
  servers: [],
});

export const CONSENT_URL = "https://auth.example.test/authorize?client=toolport";

const entries: Array<[string, unknown]> = [
  ["task ls", loginTasks],
  ["status", loginsStatus],
  ["server ls", loginsServerLs],
  ["auth statusline", loginsStatusline],
  ["auth hook", loginsHook],
  [
    "auth probe",
    probeOf(
      loginsAuthRows,
      "due",
      loginsAuthRows.filter((r) => r.fix).map((r) => r.server),
      loginsAuthRows.filter((r) => !r.fix).map((r) => r.server),
    ),
  ],
  [
    "auth probe --force",
    probeOf(
      loginsAuthRows,
      "all",
      loginsAuthRows.map((r) => r.server),
    ),
  ],
  [
    "secret get srv-erp ERP_API_KEY",
    { server: "srv-erp", key: "ERP_API_KEY", set: true },
  ],
  [
    "secret get srv-erp ERP_WEBHOOK_SECRET",
    ctlReplyFailure("not_found", "ERP_WEBHOOK_SECRET is not set for srv-erp"),
  ],
  [
    "secret get srv-mail MAIL_TOKEN",
    { server: "srv-mail", key: "MAIL_TOKEN", set: true },
  ],
  [
    "secret get srv-erp ERP_API_KEY --reveal",
    { server: "srv-erp", key: "ERP_API_KEY", set: true, value: "fixture-vaulted-value" },
  ],
  [
    "secret get srv-mail MAIL_TOKEN --reveal",
    { server: "srv-mail", key: "MAIL_TOKEN", set: true, value: "fixture-vaulted-value" },
  ],
  [
    "secret set srv-erp ERP_API_KEY",
    { server: "srv-erp", key: "ERP_API_KEY", stored: true },
  ],
  [
    "secret set srv-erp ERP_WEBHOOK_SECRET",
    { server: "srv-erp", key: "ERP_WEBHOOK_SECRET", stored: true },
  ],
  [
    "secret set srv-mail MAIL_TOKEN",
    { server: "srv-mail", key: "MAIL_TOKEN", stored: true },
  ],
  [
    "secret rm srv-erp ERP_API_KEY",
    { server: "srv-erp", key: "ERP_API_KEY", removed: true },
  ],
  [
    "secret rm srv-mail MAIL_TOKEN",
    { server: "srv-mail", key: "MAIL_TOKEN", removed: true },
  ],
  [
    "auth login srv-issues",
    new CtlReplyHeld(
      ["Open this URL to sign in:", `  ${CONSENT_URL}`],
      loginReport("srv-issues", "issue-tracker", "Signed in to issue-tracker."),
    ),
  ],
  [
    "auth login srv-issues --no-open",
    new CtlReplyHeld(
      ["Open this URL to sign in:", `  ${CONSENT_URL}`],
      loginReport("srv-issues", "issue-tracker", "Signed in to issue-tracker."),
    ),
  ],
  [
    "auth login srv-design",
    loginReport("srv-design", "design-files", "Signed in to design-files."),
  ],
  [
    "auth login srv-corp",
    loginReport("srv-corp", "corp-tools", "Signed in to corp-tools."),
  ],
  [
    "auth login srv-wiki",
    loginReport("srv-wiki", "wiki-reader", "Signed in to wiki-reader."),
  ],
  [
    "auth login srv-erp",
    ctlReplyFailure(
      "unsupported",
      "acme-erp signs in with an API token, so there is no browser sign-in. Next: toolportctl secret set srv-erp ERP_API_KEY (value on stdin or --value-env <VAR>), then toolportctl auth probe --server srv-erp --force",
    ),
  ],
];

for (const server of loginsServerLs.servers) {
  entries.push([`server info ${server.id}`, loginsServerInfos[server.id]]);
  for (const force of [true, false]) {
    entries.push([
      `auth probe --server ${server.id}${force ? " --force" : ""}`,
      probeOf(loginsAuthRows, "one", [server.id]),
    ]);
  }
}

/** Replies of the Logins & secrets screens, per argv joined with spaces. */
export const loginsCtlFixtures: Array<[string, unknown]> = entries;
