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
