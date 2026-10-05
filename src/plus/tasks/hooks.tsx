import { useCallback, useEffect, useRef, useState } from "react";
import { ctlData } from "../bridge/ctl";
import type { CommandRow } from "../bridge/data";
import { commandLine } from "../allcommands/model";
import { useRunFlow } from "../allcommands/useRunFlow";
import { policyOf } from "../system/model";
import type { WriteControl, WriteSpec } from "../system/hooks";
import { outcomeOf, useCtlQuery, type CtlQuery } from "../ui";
import type {
  TaskHistoryData,
  TaskHistoryRunData,
  TaskLsData,
  TaskRun,
} from "../types/tasks";
import { isActive } from "./model";

export const RUN_POLL_MS = 1500;

export function useTaskList(): CtlQuery<TaskLsData> {
  return useCtlQuery<TaskLsData>(["task", "ls", "--all"]);
}

export function useHistory(limit = 50): CtlQuery<TaskHistoryData> {
  return useCtlQuery<TaskHistoryData>(["task", "history", "--limit", String(limit)]);
}

/** One run read again until it ends: its steps move from waiting to running to done as the
 * runner works, and a step that waits for you stays in view with its instructions. */
export function useRunPoll(runId: string | null, pollMs: number, initial?: TaskRun) {
  const [run, setRun] = useState<TaskRun | null>(initial ?? null);
  const [error, setError] = useState<unknown>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!runId) return;
    let alive = true;
    let timer: number | undefined;
    const poll = async () => {
      try {
        const data = await ctlData<TaskHistoryRunData>([
          "task",
          "history",
          "--run",
          runId,
        ]);
        if (!alive) return;
        setRun(data.run);
        setError(null);
        if (!isActive(data.run)) return;
      } catch (failure) {
        if (!alive) return;
        setError(failure);
      }
      timer = window.setTimeout(() => void poll(), pollMs);
    };
    void poll();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [runId, pollMs, tick]);
  const refresh = useCallback(() => setTick((n) => n + 1), []);
  return { run, error, refresh };
}

export interface TaskWriteSpec extends WriteSpec {
  /** The task file, sent on the child's stdin (it holds names of secrets, never values). */
  stdin?: string;
}

export interface TaskWriteControl extends Omit<WriteControl, "begin"> {
  begin: (spec: TaskWriteSpec) => void;
}

/** One write of the Tasks screen: preview, confirm, apply, result (D-059). Same as the
 * System screen's, with the task file on stdin for `task add` and `task edit`. */
export function useTaskWrite(
  rows: CommandRow[] | null,
  onApplied: () => void,
): TaskWriteControl {
  const flow = useRunFlow();
  const { begin: beginFlow, reset } = flow;
  const [spec, setSpec] = useState<TaskWriteSpec | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const handled = useRef<unknown>(null);
  const applied = useRef(onApplied);
  const current = useRef<TaskWriteSpec | null>(null);
  useEffect(() => {
    applied.current = onApplied;
    current.current = spec;
  });

  const begin = useCallback(
    (next: TaskWriteSpec) => {
      const policy = policyOf(rows, next.command);
      if (!policy) {
        setRefused(
          rows
            ? `Toolport does not know how safe \`${next.command}\` is, so it does not run it from here.`
            : "The command list has not loaded yet. Try again in a moment.",
        );
        return;
      }
      setRefused(null);
      setSpec(next);
      const stdin = next.stdin;
      beginFlow(
        {
          title: next.title,
          line: commandLine(next.argv),
          tier: policy.tier,
          phrase: next.phrase ?? next.command,
          mode: policy.previewFlag ? "preview" : "direct",
          confirmFirst: false,
          argv: next.argv,
          previewArgv: policy.previewFlag
            ? [...next.argv, policy.previewFlag]
            : undefined,
        },
        stdin === undefined ? undefined : () => stdin,
      );
    },
    [rows, beginFlow],
  );

  const state = flow.apply.state;
  useEffect(() => {
    if (state.phase !== "done" || handled.current === state) return;
    handled.current = state;
    const outcome = outcomeOf(state);
    if (outcome?.kind !== "ok") return;
    current.current?.onResult?.(outcome.data, false);
    applied.current();
  }, [state]);

  const dismiss = useCallback(() => {
    setRefused(null);
    setSpec(null);
    reset();
  }, [reset]);

  return { flow, spec, refused, busy: flow.busy, begin, dismiss };
}
