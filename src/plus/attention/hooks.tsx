import { useCallback, useEffect, useRef, useState } from "react";
import type { CommandRow } from "../bridge/data";
import { commandLine } from "../allcommands/model";
import { useRunFlow } from "../allcommands/useRunFlow";
import { refreshAttentionCount } from "../attention";
import { policyOf } from "../system/model";
import type { WriteControl, WriteSpec } from "../system/hooks";
import { outcomeOf, useCtlQuery, type CtlQuery } from "../ui";
import type { AttentionLsData } from "../types/attention";

export function useAttentionList(): CtlQuery<AttentionLsData> {
  return useCtlQuery<AttentionLsData>(["attention", "ls"]);
}

export interface AttentionWriteControl extends WriteControl {
  refuse: (message: string) => void;
}

/** One run of the Attention screen: a dismissal or the action a row offers. Same flow as the
 * System screen's write (preview with the policy's dry run, confirm, apply, result: D-059), with
 * one difference: a command of the read tier runs at once, since asking about a read would only
 * be noise. A command the registry does not know, or one that needs a terminal, is refused. */
export function useAttentionWrite(
  rows: CommandRow[] | null,
  onApplied: () => void,
): AttentionWriteControl {
  const flow = useRunFlow();
  const { begin: beginFlow, reset } = flow;
  const [spec, setSpec] = useState<WriteSpec | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const handled = useRef<unknown>(null);
  const handledPreview = useRef<unknown>(null);
  const applied = useRef(onApplied);
  const current = useRef<WriteSpec | null>(null);
  useEffect(() => {
    applied.current = onApplied;
    current.current = spec;
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
        mode: policy.tier === "read" ? "run" : policy.previewFlag ? "preview" : "direct",
        confirmFirst: false,
        argv: next.argv,
        previewArgv: policy.previewFlag ? [...next.argv, policy.previewFlag] : undefined,
      });
    },
    [rows, beginFlow],
  );

  const previewState = flow.preview.state;
  useEffect(() => {
    if (previewState.phase !== "done" || handledPreview.current === previewState) return;
    handledPreview.current = previewState;
    const outcome = outcomeOf(previewState);
    if (outcome?.kind === "ok") current.current?.onResult?.(outcome.data, true);
  }, [previewState]);

  const state = flow.apply.state;
  useEffect(() => {
    if (state.phase !== "done" || handled.current === state) return;
    handled.current = state;
    const outcome = outcomeOf(state);
    if (outcome?.kind !== "ok") return;
    current.current?.onResult?.(outcome.data, false);
    applied.current();
    refreshAttentionCount();
  }, [state]);

  const dismiss = useCallback(() => {
    setRefused(null);
    setSpec(null);
    reset();
  }, [reset]);

  return {
    flow,
    spec,
    refused,
    busy: flow.busy,
    begin,
    dismiss,
    refuse: setRefused,
  };
}
