import { ctlData } from "../bridge/ctl";
import type { ProfileInspectData } from "../types/profile";
import type { InspectData } from "../types/inspect";
import { errorText } from "../ui";
import { looksLikeLogin, type ProfileData } from "./model";

export interface InspectedTool {
  name: string;
  description: string;
}

export type InspectState = "pending" | "ok" | "login" | "failed";

export interface InspectRow {
  id: string;
  name: string;
  state: InspectState;
  tools: InspectedTool[];
  message: string | null;
}

export interface InspectRun {
  rows: InspectRow[];
  /** Why the whole-profile command was not enough: what it stopped at. */
  stoppedAt: string | null;
  done: boolean;
}

const POOL = 3;

const pending = (profile: ProfileData): InspectRow[] =>
  profile.servers.map((server) => ({
    id: server.id,
    name: server.name,
    state: "pending",
    tools: [],
    message: null,
  }));

export function summarize(rows: InspectRow[]) {
  const count = (state: InspectState) => rows.filter((row) => row.state === state).length;
  return {
    ok: count("ok"),
    login: count("login"),
    failed: count("failed"),
    pending: count("pending"),
    tools: rows.reduce((sum, row) => sum + row.tools.length, 0),
  };
}

function failedRow(row: InspectRow, error: unknown): InspectRow {
  const { message } = errorText(error);
  return { ...row, state: looksLikeLogin(message) ? "login" : "failed", message };
}

/** `profile inspect` stops at the first server that fails, such as a 401. When it does, each
 * server is asked on its own, so one that needs a login is reported and the others still
 * answer. */
export async function inspectProfile(
  profile: ProfileData,
  options: { signal: AbortSignal; onUpdate: (run: InspectRun) => void },
): Promise<InspectRun> {
  const { signal, onUpdate } = options;
  let rows = pending(profile);
  let stoppedAt: string | null = null;
  const publish = (done: boolean) => {
    const run = { rows, stoppedAt, done };
    onUpdate(run);
    return run;
  };
  publish(false);
  if (rows.length === 0) return publish(true);
  try {
    const data = await ctlData<ProfileInspectData>(["profile", "inspect", profile.id], {
      signal,
    });
    rows = rows.map((row) => {
      const found = data.servers.find((entry) => entry.id === row.id);
      return found
        ? { ...row, state: "ok", tools: found.tools }
        : { ...row, state: "failed", message: "The command did not report this server" };
    });
    return publish(true);
  } catch (error) {
    if (signal.aborted) return publish(false);
    stoppedAt = errorText(error).message;
  }
  publish(false);
  let next = 0;
  const worker = async () => {
    while (next < rows.length && !signal.aborted) {
      const index = next++;
      const row = rows[index];
      let result: InspectRow;
      try {
        const data = await ctlData<InspectData>(["inspect", row.id], { signal });
        const found =
          data.servers.find((entry) => entry.id === row.id) ?? data.servers[0];
        result = { ...row, state: "ok", tools: found?.tools ?? [] };
      } catch (error) {
        if (signal.aborted) return;
        result = failedRow(row, error);
      }
      rows = rows.map((candidate, i) => (i === index ? result : candidate));
      publish(false);
    }
  };
  await Promise.all(Array.from({ length: Math.min(POOL, rows.length) }, worker));
  return publish(!signal.aborted);
}
