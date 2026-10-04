import { useCallback, useRef, useState, type ReactNode } from "react";
import { useCtlJob, type CtlJobControl } from "../ui";
import type { Tier } from "./model";

export type FlowPhase = "preview" | "apply";
export type StdinReader = (phase: FlowPhase) => string | undefined;

export interface FlowSpec {
  /** What the dialogs call the run, e.g. the command id. */
  title: string;
  /** The exact command line, shown in every confirmation (never holds a secret). */
  line: string;
  /** Extra context for the confirmation, such as the arguments of a tool. */
  detail?: ReactNode;
  tier: Tier;
  /** What the user types to confirm a destructive run. */
  phrase: string;
  mode: "run" | "preview" | "direct";
  /** A read that still asks first (it costs something). */
  confirmFirst: boolean;
  /** The apply, or the whole run for a read. */
  argv: string[];
  previewArgv?: string[];
}

type Dialog = "none" | "confirm" | "review";

export interface RunFlowControl {
  busy: boolean;
  spec: FlowSpec | null;
  dialog: Dialog;
  preview: CtlJobControl;
  apply: CtlJobControl;
  begin: (spec: FlowSpec, readStdin?: StdinReader) => void;
  reviewAgain: () => void;
  closeDialog: () => void;
  confirm: () => void;
  reset: () => void;
}

/** The safe way to run a write (D-059): preview with the dry run, show the plan, confirm
 * (typing a phrase when the policy tier is `destructive`), then apply. A read runs at once;
 * a writer without a dry run is confirmed first and shows its exact command line. */
export function useRunFlow(): RunFlowControl {
  const preview = useCtlJob();
  const apply = useCtlJob();
  const { reset: resetPreview, start: startPreview } = preview;
  const { reset: resetApply, start: startApply } = apply;
  const [spec, setSpec] = useState<FlowSpec | null>(null);
  const [dialog, setDialog] = useState<Dialog>("none");
  const stdin = useRef<StdinReader | undefined>(undefined);
  const token = useRef(0);

  const busy = preview.state.phase === "running" || apply.state.phase === "running";

  const options = useCallback((phase: FlowPhase) => {
    const value = stdin.current?.(phase);
    return value ? { stdinSecret: value } : {};
  }, []);

  const reset = useCallback(() => {
    token.current += 1;
    resetPreview();
    resetApply();
    setSpec(null);
    setDialog("none");
  }, [resetPreview, resetApply]);

  const begin = useCallback(
    (next: FlowSpec, readStdin?: StdinReader) => {
      const mine = ++token.current;
      resetPreview();
      resetApply();
      stdin.current = readStdin;
      setSpec(next);
      if (next.mode === "run" && !next.confirmFirst) {
        setDialog("none");
        void startApply(next.argv, options("apply"));
      } else if (next.mode === "preview" && next.previewArgv) {
        setDialog("none");
        void startPreview(next.previewArgv, options("preview")).then((result) => {
          if (token.current === mine && result?.envelope?.ok) setDialog("review");
        });
      } else {
        setDialog("confirm");
      }
    },
    [resetPreview, startPreview, resetApply, startApply, options],
  );

  const confirm = useCallback(() => {
    if (!spec) return;
    setDialog("none");
    void startApply(spec.argv, options("apply"));
  }, [spec, startApply, options]);

  return {
    busy,
    spec,
    dialog,
    preview,
    apply,
    begin,
    reviewAgain: () => setDialog("review"),
    closeDialog: () => setDialog("none"),
    confirm,
    reset,
  };
}
