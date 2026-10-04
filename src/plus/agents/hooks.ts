import { useCallback, useEffect, useRef, useState } from "react";
import { CtlError, runCtl, type CtlResult } from "../bridge/ctl";
import type { CommandRow, CommandsData } from "../bridge/data";
import { commandLine } from "../allcommands/model";
import { useRunFlow, type RunFlowControl } from "../allcommands/useRunFlow";
import { outcomeOf, useCtlQuery, type CtlQuery } from "../ui";
import { policyOf } from "./model";

type Settled<T> = {
  key: string;
  tick: number;
  data: T | null;
  error: unknown;
};

function failure(result: CtlResult): CtlError {
  const error = result.envelope?.error;
  return new CtlError(
    error?.message ?? result.parseError ?? `toolportctl exited with ${result.exitCode}`,
    error?.code ?? "bridge",
    result,
  );
}

/** One read command. `lint`, `audit` and `status --strict` exit 1 when they find something but
 * still print their data, so the data counts as the answer whenever the envelope has it. */
export function useRead<T>(argv: readonly string[]): CtlQuery<T> {
  const key = JSON.stringify(argv);
  const [tick, setTick] = useState(0);
  const [settled, setSettled] = useState<Settled<T> | null>(null);

  useEffect(() => {
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
  }, [key, tick]);

  const reload = useCallback(() => setTick((n) => n + 1), []);
  const same = settled?.key === key;
  const fresh = same && settled.tick === tick;
  const status = !fresh ? "loading" : settled.error ? "error" : "ready";
  return {
    status,
    data: same ? settled.data : null,
    error: fresh ? settled.error : null,
    reload,
  };
}

/** The registry rows the policy of each write is read from. */
export function useRegistryRows(): CommandRow[] | null {
  const query = useCtlQuery<CommandsData>(["commands"]);
  return query.data?.commands ?? null;
}

/** One write of the screen: preview, confirm, apply, result (D-059). */
export interface WriteSpec {
  /** The registry id the policy is read from, e.g. `agents clean`. */
  command: string;
  /** What the dialogs call it, e.g. `Remove agent scout`. */
  title: string;
  /** The apply argv, without the preview flag. */
  argv: string[];
  confirmLabel?: string;
  /** What is typed to confirm a destructive tier. */
  phrase: string;
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
        phrase: next.phrase,
        mode: policy.previewFlag ? "preview" : "direct",
        confirmFirst: false,
        argv: next.argv,
        previewArgv: policy.previewFlag ? [...next.argv, policy.previewFlag] : undefined,
      });
    },
    [rows, beginFlow],
  );

  const state = flow.apply.state;
  useEffect(() => {
    if (state.phase !== "done" || handled.current === state) return;
    handled.current = state;
    if (outcomeOf(state)?.kind === "ok") applied.current();
  }, [state]);

  const dismiss = useCallback(() => {
    setRefused(null);
    setSpec(null);
    reset();
  }, [reset]);

  return { flow, spec, refused, busy: flow.busy, begin, dismiss };
}
