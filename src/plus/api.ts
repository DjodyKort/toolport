import { invoke } from "@tauri-apps/api/core";

export interface PlusPing {
  name: string;
  version: string;
  forkEgressDisabled: boolean;
}

/** Single IPC entry for every Toolport+ extension command (see `src-tauri/src/plus`). */
export function plusInvoke<T>(command: string, args: unknown = {}): Promise<T> {
  return invoke<T>("plus_invoke", { command, args });
}

export function plusPing(): Promise<PlusPing> {
  return plusInvoke<PlusPing>("plus.ping");
}

export type AuthStateName =
  | "unknown"
  | "ok"
  | "expiring"
  | "needs_reauth"
  | "revoked"
  | "misconfigured"
  | "unreachable";

export interface AuthFixAction {
  action: "reauth" | "reconsent" | "fix_config" | "retry";
  server: string;
  label: string;
  command: string | null;
  ipc: { command: string; args: Record<string, unknown> } | null;
}

export interface AuthRow {
  server: string;
  state: AuthStateName;
  reason: string;
  since: number;
  expiresAt: number | null;
  ttlSecs: number | null;
  lastProbe: number | null;
  fix: AuthFixAction | null;
}

export interface AuthRows {
  counts: Record<AuthStateName, number>;
  rows: AuthRow[];
}

export function plusAuthRows(): Promise<AuthRows> {
  return plusInvoke<AuthRows>("plus.auth.rows");
}

export interface LoadItem {
  kind: string;
  name: string;
  path: string | null;
  source: string;
  loaded: boolean;
  reason: string;
  tokens: number;
}

export interface Clobber {
  kind: string;
  key: string;
  winner: string;
  overridden: string[];
  relation: string;
}

export interface WhatLoads {
  profile: string | null;
  cwd: string;
  items: LoadItem[];
  clobbers: Clobber[];
  tokens_by_kind: Record<string, number>;
  total_tokens: number;
  notes: string[];
}

export function plusWhatLoads(
  args: { profile?: string; cwd?: string } = {},
): Promise<WhatLoads> {
  return plusInvoke<WhatLoads>("plus.context.whatLoads", args);
}
