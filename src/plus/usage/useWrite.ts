import { useCallback, useEffect, useRef, useState } from "react";
import type { CommandRow, CommandsData } from "../bridge/data";
import { commandLine } from "../allcommands/model";
import { useRunFlow, type RunFlowControl } from "../allcommands/useRunFlow";
import { outcomeOf, useCtlQuery, type PlanV1 } from "../ui";
import { policyOf } from "./model";

export function useRegistryRows(): CommandRow[] | null {
  return useCtlQuery<CommandsData>(["commands"]).data?.commands ?? null;
}

/** One write of the screen: preview, confirm, apply, result (D-059). */
export interface WriteSpec {
  /** The registry id the policy is read from, e.g. `obs otel enable`. */
  command: string;
  title: string;
  /** The apply argv. */
  argv: string[];
  /** The preview argv when it is not the apply argv plus the registry's preview flag. */
  previewArgv?: string[];
  confirmLabel?: string;
  /** What is typed to confirm a destructive tier. */
  phrase: string;
  /** Forces the typed confirmation although the policy tier is lower. */
  typed?: boolean;
  /** Reads a preview or a result as a plan; without it the data is listed as it is. */
  plan?: (data: unknown) => PlanV1 | null;
  /** A line shown above the plan, such as "this downloads the engine". */
  notice?: string;
}

export interface WriteControl {
  flow: RunFlowControl;
  spec: WriteSpec | null;
  refused: string | null;
  begin: (spec: WriteSpec) => void;
  dismiss: () => void;
}

/** Which dialog of a write is showing: the preview in progress, the plan to confirm, or the
 * result. */
export function dialogPhases(flow: RunFlowControl) {
  const { dialog, preview, apply } = flow;
  const previewOk = !!preview.state.result?.envelope?.ok;
  const previewing =
    apply.state.phase === "idle" &&
    (preview.state.phase === "running" || (preview.state.phase === "done" && !previewOk));
  const applying = apply.state.phase !== "idle";
  const open =
    dialog === "confirm" || (dialog === "review" && previewOk) || previewing || applying;
  return { previewOk, previewing, applying, open };
}

export function useWrite(rows: CommandRow[] | null, onApplied: () => void): WriteControl {
  const flow = useRunFlow();
  const { begin: beginFlow, reset } = flow;
  const [spec, setSpec] = useState<WriteSpec | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const handled = useRef<unknown>(null);
  const origin = useRef<HTMLElement | null>(null);
  const applied = useRef(onApplied);
  useEffect(() => {
    applied.current = onApplied;
  });

  const begin = useCallback(
    (next: WriteSpec) => {
      const policy = policyOf(rows, next.command);
      if (!policy || policy.terminal) {
        setRefused(
          policy
            ? `\`${next.command}\` needs a terminal. Copy its command line and run it there.`
            : rows
              ? `Toolport does not know how safe \`${next.command}\` is, so it does not run it from here.`
              : "The command list has not loaded yet. Try again in a moment.",
        );
        return;
      }
      const flag = next.previewArgv ? null : policy.previewFlag;
      const previewArgv = next.previewArgv ?? (flag ? [...next.argv, flag] : undefined);
      const active = document.activeElement;
      origin.current =
        active instanceof HTMLElement && active !== document.body ? active : null;
      setRefused(null);
      setSpec(next);
      beginFlow({
        title: next.title,
        line: commandLine(next.argv),
        tier: next.typed ? "destructive" : policy.tier,
        phrase: next.phrase,
        mode: previewArgv ? "preview" : "direct",
        confirmFirst: false,
        argv: next.argv,
        previewArgv,
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

  const open = !!spec && dialogPhases(flow).open;
  const wasOpen = useRef(false);
  useEffect(() => {
    const closed = wasOpen.current && !open;
    wasOpen.current = open;
    if (!closed) return;
    // Radix puts focus back where its own dialog started, which is the previous dialog of the
    // same write; the control that began the write is where a keyboard user left off.
    const timer = setTimeout(() => {
      const target = origin.current;
      if (target?.isConnected) target.focus();
    }, 0);
    return () => clearTimeout(timer);
  }, [open]);

  const dismiss = useCallback(() => {
    setRefused(null);
    setSpec(null);
    reset();
  }, [reset]);

  return { flow, spec, refused, begin, dismiss };
}
