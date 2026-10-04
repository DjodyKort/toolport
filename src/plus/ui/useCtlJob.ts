import { useCallback, useEffect, useRef, useState } from "react";
import { runCtl, type CtlJob, type CtlResult } from "../bridge/ctl";

const MAX_LINES = 500;

export type JobPhase = "idle" | "running" | "done";

export interface JobState {
  phase: JobPhase;
  /** stderr lines so far, newest last. */
  lines: string[];
  result: CtlResult | null;
  /** The bridge itself failed (no result at all). */
  failure: string | null;
  cancelling: boolean;
}

export type JobOutcome =
  | { kind: "ok"; data: unknown }
  | { kind: "cancelled" }
  | { kind: "error"; code: string | null; message: string };

const IDLE: JobState = {
  phase: "idle",
  lines: [],
  result: null,
  failure: null,
  cancelling: false,
};

/** The final state of a job: the envelope's data, a cancel, or what went wrong. */
export function outcomeOf(state: JobState): JobOutcome | null {
  if (state.phase !== "done") return null;
  const { result, failure } = state;
  if (failure) return { kind: "error", code: "bridge", message: failure };
  if (!result) return null;
  if (result.cancelled) return { kind: "cancelled" };
  const envelope = result.envelope;
  if (envelope?.ok) return { kind: "ok", data: envelope.data };
  if (envelope?.error)
    return { kind: "error", code: envelope.error.code, message: envelope.error.message };
  return {
    kind: "error",
    code: "bridge",
    message: result.parseError ?? `toolportctl exited with ${result.exitCode}`,
  };
}

const URL_IN_LINE = /https?:\/\/[^\s"'<>]+/;

/** The first address a job printed, such as the consent URL of a sign-in. */
export function firstAddress(lines: string[]): string | null {
  for (const line of lines) {
    const found = URL_IN_LINE.exec(line);
    if (found) return found[0];
  }
  return null;
}

export interface CtlJobControl {
  state: JobState;
  /** Starts a run. The secret goes to the child's stdin and is kept nowhere else. */
  start: (
    argv: string[],
    options?: { stdinSecret?: string },
  ) => Promise<CtlResult | null>;
  cancel: () => Promise<void>;
  reset: () => void;
}

/** One `plus_ctl` run at a time, with its stderr lines and its way to stop it. */
export function useCtlJob(): CtlJobControl {
  const [state, setState] = useState<JobState>(IDLE);
  const current = useRef<{ id: number; job: CtlJob | null }>({ id: 0, job: null });

  useEffect(
    () => () => {
      current.current.id += 1;
      void current.current.job?.cancel().catch(() => {});
    },
    [],
  );

  const start = useCallback(
    async (argv: string[], options: { stdinSecret?: string } = {}) => {
      const id = ++current.current.id;
      const live = () => current.current.id === id;
      setState({ ...IDLE, phase: "running" });
      const job = runCtl(argv, {
        stdinSecret: options.stdinSecret,
        onStderr: (line) => {
          if (!live()) return;
          setState((prev) => ({
            ...prev,
            lines: [...prev.lines, line].slice(-MAX_LINES),
          }));
        },
      });
      current.current.job = job;
      try {
        const result = await job.result;
        if (current.current.job === job) current.current.job = null;
        if (live()) setState((prev) => ({ ...prev, phase: "done", result }));
        return result;
      } catch (error) {
        if (current.current.job === job) current.current.job = null;
        const message = error instanceof Error ? error.message : String(error);
        if (live()) setState((prev) => ({ ...prev, phase: "done", failure: message }));
        return null;
      }
    },
    [],
  );

  const cancel = useCallback(async () => {
    const job = current.current.job;
    if (!job) return;
    setState((prev) => ({ ...prev, cancelling: true }));
    await job.cancel().catch(() => {});
  }, []);

  const reset = useCallback(() => {
    current.current.id += 1;
    void current.current.job?.cancel().catch(() => {});
    current.current.job = null;
    setState(IDLE);
  }, []);

  return { state, start, cancel, reset };
}
