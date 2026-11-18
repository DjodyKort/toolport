/** The `data` of `toolportctl client direct ls|add|rm`. The Rust goldens under
 * plus/fixtures/ctl-envelopes (client-direct-*) are the source; data.test.ts parses them against
 * these shapes. */

export type DirectState =
  "ok" | "stale" | "customized" | "missing" | "orphan" | "unrecorded" | "relocated";

export interface DirectRow {
  client: string;
  clientName: string;
  /** The entry's name in the client's file. */
  entry: string;
  /** The server's id; kept after the server is gone, which is how an orphan names it. */
  server: string;
  serverName: string | null;
  state: DirectState;
  /** The client's config file. */
  path: string;
  /** Set for a remote entry, which holds the address only. */
  url?: string;
  /** What the state offers, as the command line words it. */
  action?: string;
}

export interface DirectListData {
  entries: DirectRow[];
}

export interface PlanStep {
  op: string;
  detail: string;
  path?: string;
  diff?: { before: string; after: string };
}

export interface DirectPlan {
  summary: string;
  steps: PlanStep[];
  warnings: string[];
  undo: string;
}

export type DirectAction =
  "added" | "updated" | "adopted" | "unchanged" | "removed" | "forgotten" | "absent";

export interface DirectOutcome {
  action: DirectAction;
  changed: boolean;
  client: string;
  clientName: string;
  dryRun: boolean;
  entry: string;
  path: string;
  plan: DirectPlan;
  planHash: string;
  result: null | {
    applied: boolean;
    changed: string[];
    undo: string;
    backups: string[];
  };
  server: string;
  serverName: string;
}
