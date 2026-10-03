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
