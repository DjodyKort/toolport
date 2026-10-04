import type { AuthRow, AuthRows } from "../api";

const base = {
  since: 1_790_000_000,
  expiresAt: null,
  ttlSecs: null,
  lastProbe: 1_790_000_300,
};

const rows: AuthRow[] = [
  {
    ...base,
    server: "google-docs",
    state: "revoked",
    reason: "revoked",
    fix: {
      action: "reconsent",
      server: "google-docs",
      label: "Re-consent access for google-docs",
      command: "toolportctl auth login google-docs",
      ipc: null,
    },
  },
  {
    ...base,
    server: "figma",
    state: "needs_reauth",
    reason: "invalid_grant",
    fix: {
      action: "reauth",
      server: "figma",
      label: "Sign in to figma again",
      command: "toolportctl auth login figma",
      ipc: null,
    },
  },
  {
    ...base,
    server: "notion",
    state: "misconfigured",
    reason: "invalid_client",
    fix: {
      action: "fix_config",
      server: "notion",
      label: "Check the OAuth client configuration of notion",
      command: null,
      ipc: null,
    },
  },
  {
    ...base,
    server: "linear",
    state: "expiring",
    reason: "token_expiring",
    expiresAt: 1_790_000_900,
    ttlSecs: 600,
    fix: {
      action: "reauth",
      server: "linear",
      label: "Sign in to linear again",
      command: "toolportctl auth login linear",
      ipc: null,
    },
  },
  {
    ...base,
    server: "odoo",
    state: "unreachable",
    reason: "unreachable",
    fix: {
      action: "retry",
      server: "odoo",
      label: "Re-check odoo",
      command: null,
      ipc: { command: "plus.auth.probe", args: { server: "odoo", force: true } },
    },
  },
  { ...base, server: "github", state: "ok", reason: "ok", fix: null },
];

export const plusAuthRowsFixture: AuthRows = {
  counts: {
    ok: 1,
    expiring: 1,
    needs_reauth: 1,
    revoked: 1,
    misconfigured: 1,
    unreachable: 1,
    unknown: 0,
  },
  rows,
};
