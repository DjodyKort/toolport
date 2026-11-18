import type { DetectedClient } from "@/lib/types";
import { isGatewayDetected } from "@/lib/types";
import type { DirectAction, DirectOutcome, DirectRow, DirectState } from "./data";

export type RowAction = "rewrite" | "remove" | "forget" | "track" | "repoint";
export type Tone = "success" | "warning" | "off" | "info";

export interface StateCopy {
  label: string;
  tone: Tone;
  meaning: string;
  actions: RowAction[];
}

export const STATES: Record<DirectState, StateCopy> = {
  ok: {
    label: "In sync",
    tone: "success",
    meaning: "The entry matches what Toolport wrote.",
    actions: ["remove"],
  },
  stale: {
    label: "Stale",
    tone: "warning",
    meaning: "The launcher path moved: toolportctl was reinstalled somewhere else.",
    actions: ["rewrite", "remove"],
  },
  customized: {
    label: "Edited since",
    tone: "warning",
    meaning: "Someone changed the entry by hand after Toolport wrote it.",
    actions: ["rewrite", "remove"],
  },
  missing: {
    label: "Missing",
    tone: "warning",
    meaning: "Toolport recorded it, but the client's file no longer has it.",
    actions: ["rewrite", "forget"],
  },
  orphan: {
    label: "Orphan",
    tone: "warning",
    meaning: "The server or the client's gateway entry is gone; only the record remains.",
    actions: ["remove"],
  },
  unrecorded: {
    label: "Not tracked",
    tone: "info",
    meaning: "It looks like an entry Toolport wrote, but nothing was recorded for it.",
    actions: ["track"],
  },
  relocated: {
    label: "Moved file",
    tone: "warning",
    meaning: "The client's config file moved; the record points at the old path.",
    actions: ["repoint", "forget"],
  },
};

export const ACTION_LABEL: Record<RowAction, string> = {
  rewrite: "Rewrite",
  remove: "Remove entry",
  forget: "Forget",
  track: "Track it",
  repoint: "Re-point",
};

/** What the dialog of a row action asks, and whether it removes something. */
export interface ActionCopy {
  title: (row: DirectRow) => string;
  confirm: string;
  destructive: boolean;
  /** The command that carries it out. */
  verb: "add" | "rm";
}

export const ACTION_COPY: Record<RowAction, ActionCopy> = {
  rewrite: {
    title: (row) => `Rewrite ${row.entry}?`,
    confirm: "Rewrite entry",
    destructive: false,
    verb: "add",
  },
  remove: {
    title: (row) => `Remove ${row.entry} from ${row.clientName}?`,
    confirm: "Remove entry",
    destructive: true,
    verb: "rm",
  },
  forget: {
    title: (row) => `Forget ${row.entry}?`,
    confirm: "Forget",
    destructive: false,
    verb: "rm",
  },
  track: {
    title: (row) => `Track ${row.entry}?`,
    confirm: "Track entry",
    destructive: false,
    verb: "add",
  },
  repoint: {
    title: (row) => `Re-point ${row.entry} to ${row.clientName}'s file?`,
    confirm: "Re-point entry",
    destructive: false,
    verb: "add",
  },
};

export const DONE: Record<RowAction, (row: DirectRow) => string> = {
  rewrite: (row) => `Rewrote ${row.entry} in ${row.clientName}`,
  remove: (row) => `Removed ${row.entry} from ${row.clientName}`,
  forget: (row) => `Forgot ${row.entry}`,
  track: (row) => `Now tracking ${row.entry}`,
  repoint: (row) => `Re-pointed ${row.entry}`,
};

/** The actions a row offers. A vanished entry of an orphan has nothing to remove, so it is
 * forgotten instead; a missing entry's Forget is the same record-only change. */
export function actionsOf(row: DirectRow, presentInFile: boolean): RowAction[] {
  if (row.state === "orphan" && !presentInFile) return ["forget"];
  return STATES[row.state].actions;
}

/** An edited entry is replaced or removed only on purpose: the plan shows what goes. */
export const needsForce = (row: DirectRow) => row.state === "customized";

/** The write that takes an outcome back, or `null` when none would. Removing or forgetting is
 * undone by adding again; a new entry by removing it. An update or an adoption has no earlier
 * file to restore, so it offers none. */
export function undoOf(outcome: DirectOutcome): "add" | "rm" | null {
  const undo: Partial<Record<DirectAction, "add" | "rm">> = {
    added: "rm",
    removed: "add",
    forgotten: "add",
  };
  return undo[outcome.action] ?? null;
}

/** The entries of a client's own file that Toolport did not write, by name. */
export function foreignEntries(client: DetectedClient, written: Set<string>) {
  return client.servers.filter(
    (server) => !isGatewayDetected(server) && !written.has(server.name),
  );
}

export function describe(row: DirectRow): string {
  const where = row.url ? "Remote entry" : "Launcher entry";
  const target = row.url ?? `starts ${row.serverName ?? row.server} through toolportctl`;
  return `${where} in ${row.path} · ${target}`;
}
