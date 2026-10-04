import { useCallback, useEffect, useRef, useState } from "react";
import { CtlError, runCtl, type CtlResult } from "../bridge/ctl";
import type { CommandRow, CommandsData } from "../bridge/data";
import { commandLine } from "../allcommands/model";
import { useRunFlow, type RunFlowControl } from "../allcommands/useRunFlow";
import { PlanPreview, outcomeOf, useCtlQuery, type CtlQuery, type PlanV1 } from "../ui";
import { policyOf } from "./model";

type Settled<T> = { key: string; tick: number; data: T | null; error: unknown };

function failure(result: CtlResult): CtlError {
  const error = result.envelope?.error;
  return new CtlError(
    error?.message ?? result.parseError ?? `toolportctl exited with ${result.exitCode}`,
    error?.code ?? "bridge",
    result,
  );
}

/** One read command. `doctor` exits 1 when a check fails but still prints its data, so the
 * data counts as the answer whenever the envelope has it; the error stays for the screen to
 * show beside it. */
export function useRead<T>(
  argv: readonly string[],
  options: { enabled?: boolean } = {},
): CtlQuery<T> {
  const key = JSON.stringify(argv);
  const enabled = options.enabled ?? true;
  const [tick, setTick] = useState(0);
  const [settled, setSettled] = useState<Settled<T> | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    runCtl<T>(JSON.parse(key) as string[]).result.then(
      (result) => {
        if (!alive) return;
        const data = result.envelope?.data ?? null;
        const failed = data === null || !!result.envelope?.error;
        setSettled({
          key,
          tick,
          data: data as T | null,
          error: failed ? failure(result) : null,
        });
      },
      (error) => alive && setSettled({ key, tick, data: null, error }),
    );
    return () => {
      alive = false;
    };
  }, [key, tick, enabled]);

  const reload = useCallback(() => setTick((n) => n + 1), []);
  const same = settled?.key === key;
  const fresh = enabled && same && settled.tick === tick;
  const status = !fresh ? "loading" : settled.error ? "error" : "ready";
  return {
    status,
    data: same ? settled.data : null,
    error: fresh ? settled.error : null,
    reload,
  };
}

/** The registry the policy of each write is read from. */
export function useRegistry(): CtlQuery<CommandsData> {
  return useCtlQuery<CommandsData>(["commands"]);
}

/** A doctor exits 1 when a check fails but still prints the checks: that is an answer, not a
 * failure of the read. */
export function answered<T>(query: CtlQuery<T>): CtlQuery<T> {
  return query.data ? { ...query, status: "ready", error: null } : query;
}

export function rowsOf(registry: CtlQuery<CommandsData>): CommandRow[] | null {
  return registry.data?.commands ?? null;
}

/** One write of the screen: preview, confirm, apply, result (D-059). */
export interface WriteSpec {
  /** The registry id the policy is read from, e.g. `sync push`. */
  command: string;
  title: string;
  /** The apply argv, without the preview flag. */
  argv: string[];
  confirmLabel?: string;
  /** What is typed to confirm a destructive tier; the command id by default. */
  phrase?: string;
  /** Words the data of a command that answers in its own shape as a plan. */
  adapt?: (data: Record<string, unknown>, done: boolean) => PlanV1 | null;
  /** The plan of a command that has no preview, worked out from the form. */
  planned?: PlanV1;
  /** What a command without a preview reports once it ran, in the past tense. */
  done?: string;
  /** Called with the data of a preview that finished (`previewed`) and of an apply. */
  onResult?: (data: unknown, previewed: boolean) => void;
}

export interface WriteControl {
  flow: RunFlowControl;
  spec: WriteSpec | null;
  refused: string | null;
  busy: boolean;
  begin: (spec: WriteSpec) => void;
  dismiss: () => void;
}

export function useWrite(rows: CommandRow[] | null, onApplied: () => void): WriteControl {
  const flow = useRunFlow();
  const { begin: beginFlow, reset } = flow;
  const [spec, setSpec] = useState<WriteSpec | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const handled = useRef<unknown>(null);
  const applied = useRef(onApplied);
  useEffect(() => {
    applied.current = onApplied;
  });

  const begin = useCallback(
    (next: WriteSpec) => {
      const policy = policyOf(rows, next.command);
      if (!policy) {
        setRefused(
          rows
            ? `Toolport does not know how safe \`${next.command}\` is, so it does not run it from here.`
            : "The command list has not loaded yet. Try again in a moment.",
        );
        return;
      }
      if (policy.terminal) {
        setRefused(
          `\`${next.command}\` needs a terminal. Copy its command line and run it there.`,
        );
        return;
      }
      setRefused(null);
      setSpec(next);
      beginFlow({
        title: next.title,
        line: commandLine(next.argv),
        tier: policy.tier,
        phrase: next.phrase ?? next.command,
        mode: policy.previewFlag ? "preview" : "direct",
        confirmFirst: false,
        argv: next.argv,
        previewArgv: policy.previewFlag ? [...next.argv, policy.previewFlag] : undefined,
        detail: next.planned ? <PlanPreview data={{ plan: next.planned }} /> : undefined,
      });
    },
    [rows, beginFlow],
  );

  const state = flow.apply.state;
  const previewState = flow.preview.state;
  const current = useRef(spec);
  useEffect(() => {
    current.current = spec;
  });
  const handledPreview = useRef<unknown>(null);
  useEffect(() => {
    if (previewState.phase !== "done" || handledPreview.current === previewState) return;
    handledPreview.current = previewState;
    const outcome = outcomeOf(previewState);
    if (outcome?.kind === "ok") current.current?.onResult?.(outcome.data, true);
  }, [previewState]);
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
