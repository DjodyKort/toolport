import { invoke } from "@tauri-apps/api/core";
import type { FolderProfile } from "@/lib/types";

/** Single IPC entry for every Toolport+ extension command (see `src-tauri/src/plus`). */
export function plusInvoke<T>(command: string, args: unknown = {}): Promise<T> {
  return invoke<T>("plus_invoke", { command, args });
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

export interface AuthLogin {
  server: string;
  name: string;
  flow: "browser" | "stdio";
  consentUrl: string | null;
  signedIn: boolean;
  message: string;
}

/** True when a click can do something; `fix_config` only has a label to read. */
export function fixIsActionable(fix: AuthFixAction): boolean {
  return fix.ipc !== null || fix.action === "reauth" || fix.action === "reconsent";
}

/** Runs the fix a row offers and returns the sentence to show the user. */
export async function plusAuthFix(fix: AuthFixAction): Promise<string> {
  if (fix.ipc !== null) {
    if (!fix.ipc.command.startsWith("plus.auth.")) {
      throw new Error(`unsupported fix route: ${fix.ipc.command}`);
    }
    await plusInvoke(fix.ipc.command, fix.ipc.args);
    return `Checked ${fix.server} again.`;
  }
  if (fix.action === "reauth" || fix.action === "reconsent") {
    const login = await plusInvoke<AuthLogin>("plus.auth.login", { server: fix.server });
    return login.message;
  }
  throw new Error(`${fix.server} has no one-click fix`);
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

export interface FolderProfileRow {
  root: string;
  applies: boolean;
  profile: string | null;
  wouldApply: string | null;
  rule: string | null;
  reason: string;
  launchProfile: string | null;
  tokens: number;
}

export interface FolderProfiles {
  enabled: boolean;
  mappings: FolderProfile[];
  folders: FolderProfileRow[];
}

export function plusFolderProfiles(
  args: { cwd?: string; roots?: string[] } = {},
): Promise<FolderProfiles> {
  return plusInvoke<FolderProfiles>("plus.context.folderProfiles", args);
}

export function plusSetFolderProfiles(enabled: boolean): Promise<{ enabled: boolean }> {
  return plusInvoke<{ enabled: boolean }>("plus.context.folderProfilesSet", { enabled });
}
