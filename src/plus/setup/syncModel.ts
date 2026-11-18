import type { View } from "@/lib/types";
import { ago } from "../tasks/model";

export interface SyncPart {
  id: string;
  label: string;
  on: boolean;
  default: boolean;
  handledBy: string | null;
}

export type SyncTransport =
  { kind: "git"; url: string; branch: string } | { kind: "folder"; path: string };

export interface SyncStatus {
  configured: boolean;
  enabled: boolean;
  transport: SyncTransport | null;
  parts: SyncPart[];
  passphrase: "stored" | "missing" | "locked";
  keyringLocked: boolean;
  paused: boolean;
  upstream: { active: boolean };
  lastSync: { direction: string; at: string; revision: string; machine: string } | null;
  lastCheck: { at: string; changes: number } | null;
  pending: string[];
  machines: { machine: string; at: string | null; thisMachine: boolean }[];
  schedule: {
    task: string;
    present: boolean;
    enabled: boolean;
    cron: string | null;
  } | null;
  undoable: string | null;
}

export type SyncState =
  "add" | "here" | "incoming" | "mine" | "conflict" | "needs" | "skip";

export interface SyncRow {
  id: string;
  label: string;
  part: string;
  state: SyncState;
  note: string;
}

export interface SyncPlan {
  applied: boolean;
  dryRun: boolean;
  plan: string;
  rows: SyncRow[];
  summary: Record<SyncState, number>;
  remote?: { machine: string; madeAt: string };
  pending?: string[];
  needsYou?: { id: string; label: string; note: string }[];
  empty?: boolean;
  note?: string;
  skipped?: boolean | unknown[];
  reason?: string;
}

export interface PushPreview {
  dryRun: boolean;
  pushed?: boolean;
  parts: { id: string; label: string; count: number }[];
  notIncluded: string[];
  totalSize: number;
}

export const SYNC_STATE_LABEL: Record<SyncState, string> = {
  add: "Will be added",
  here: "Already here",
  incoming: "Changed on the other machine",
  mine: "Changed only here",
  conflict: "Changed on both machines",
  needs: "Needs you",
  skip: "Skipped",
};

/** Rows a pull would write on this machine. */
export const CHANGING: SyncState[] = ["add", "incoming"];

export function changes(plan: SyncPlan, taken: string[]): SyncRow[] {
  return plan.rows.filter(
    (row) =>
      CHANGING.includes(row.state) ||
      (row.state === "conflict" && taken.includes(row.id)),
  );
}

export function setupArgv(options: {
  git?: string;
  branch?: string;
  folder?: string;
  parts?: string[];
  passphrase?: boolean;
  schedule?: boolean;
  enabled?: boolean;
  forgetPassphrase?: boolean;
}): string[] {
  const argv = ["sync", "setup"];
  if (options.git) argv.push("--git", options.git);
  if (options.git && options.branch) argv.push("--branch", options.branch);
  if (options.folder) argv.push("--folder", options.folder);
  if (options.parts) argv.push("--parts", options.parts.join(","));
  if (options.passphrase) argv.push("--passphrase-stdin");
  if (options.schedule !== undefined)
    argv.push("--schedule", options.schedule ? "on" : "off");
  if (options.enabled !== undefined)
    argv.push(options.enabled ? "--enable" : "--disable");
  if (options.forgetPassphrase) argv.push("--forget-passphrase");
  return argv;
}

export function pullArgv(options: {
  take?: string[];
  keep?: string[];
  plan?: string;
}): string[] {
  const argv = ["sync", "pull"];
  for (const row of options.take ?? []) argv.push("--take", row);
  if (options.plan) argv.push("--apply", "--plan", options.plan);
  for (const row of options.keep ?? []) argv.push("--keep", row);
  return argv;
}

export function rowName(id: string): string {
  const at = id.indexOf(":");
  return at < 0 ? id : id.slice(at + 1);
}

const PAGE_OF_ROW: Record<string, { view: View; label: string }> = {
  server: { view: "servers", label: "Open Servers" },
  task: { view: "tasks", label: "Open Tasks" },
};

/** The page where a row of the bundle is reviewed, named by the kind in front of its id. */
export function pageOfRow(id: string): { view: View; label: string } | null {
  const at = id.indexOf(":");
  return at < 0 ? null : (PAGE_OF_ROW[id.slice(0, at)] ?? null);
}

export function whenText(iso: string | null, now = Date.now()): string {
  const at = iso ? Date.parse(iso) : NaN;
  return Number.isNaN(at) ? "not yet" : ago(at, now);
}

export function placeOf(transport: SyncTransport): string {
  return transport.kind === "git" ? transport.url : transport.path;
}
