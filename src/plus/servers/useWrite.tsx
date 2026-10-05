import { useCallback, useEffect, useRef, useState } from "react";
import { commandLine } from "../allcommands/model";
import { useRunFlow, type RunFlowControl } from "../allcommands/useRunFlow";
import { PlanPreview, outcomeOf, type PlanV1 } from "../ui";
import { policyOf } from "./model";
import { toolArgv, toolPolicyOf } from "./mcpTools";
import { useServers } from "./useServers";

/** One write of the screen: preview, confirm, apply, result (D-059). The tier and the preview
 * flag come from the registry, not from the screen. */
export interface WriteSpec {
  /** The registry id the policy is read from, e.g. `server uninstall`. */
  command?: string;
  /** A self-management tool run through `mcp call` instead of a command; its policy is the
   * tool's row, and `stdin` holds its arguments. */
  tool?: string;
  stdin?: string;
  /** What the dialogs call it, e.g. `Remove acme-erp`. */
  title: string;
  /** The apply argv, without the preview flag. */
  argv: string[];
  confirmLabel?: string;
  /** What is typed to confirm a destructive tier. */
  phrase?: string;
  /** Words the data of an older command as a plan. */
  adapt?: (data: unknown, done: boolean) => PlanV1;
  /** The plan of a command that has no preview, worked out from the form. */
  planned?: PlanV1;
  after?: (data: unknown) => void;
}

export interface WriteControl {
  flow: RunFlowControl;
  spec: WriteSpec | null;
  refused: string | null;
  busy: boolean;
  begin: (spec: WriteSpec) => void;
}

export function useWrite(): WriteControl {
  const { rows, registry, reload } = useServers();
  const flow = useRunFlow();
  const { begin: beginFlow } = flow;
  const [spec, setSpec] = useState<WriteSpec | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const handled = useRef<unknown>(null);

  const begin = useCallback(
    (next: WriteSpec) => {
      if (next.tool) {
        const tool = toolPolicyOf(registry.data ?? null, next.tool);
        if (!tool) {
          setRefused(
            `Toolport cannot tell how safe \`${next.tool}\` is, so it does not run it from here.`,
          );
          return;
        }
        setRefused(null);
        setSpec(next);
        const argv = toolArgv(next.tool);
        beginFlow(
          {
            title: next.title,
            line: commandLine(argv),
            tier: tool.tier,
            phrase: next.phrase ?? next.title,
            mode: "direct",
            confirmFirst: false,
            argv,
            detail: next.planned ? (
              <PlanPreview data={{ plan: next.planned }} />
            ) : undefined,
          },
          () => next.stdin,
        );
        return;
      }
      const command = next.command ?? "";
      const policy = policyOf(rows, command);
      if (policy?.terminal) {
        setRefused(
          `\`${command}\` needs a terminal. Copy its command line and run it there.`,
        );
        return;
      }
      if (!policy) {
        setRefused(
          `Toolport does not know how safe \`${command}\` is, so it does not run it from here.`,
        );
        return;
      }
      setRefused(null);
      setSpec(next);
      const line = commandLine(next.argv);
      beginFlow({
        title: next.title,
        line,
        tier: policy.tier,
        phrase: next.phrase ?? next.title,
        mode: policy.previewFlag ? "preview" : "direct",
        confirmFirst: false,
        argv: next.argv,
        previewArgv: policy.previewFlag ? [...next.argv, policy.previewFlag] : undefined,
        detail: next.planned ? <PlanPreview data={{ plan: next.planned }} /> : undefined,
      });
    },
    [rows, registry.data, beginFlow],
  );

  const applied = flow.apply.state;
  useEffect(() => {
    if (applied.phase !== "done" || handled.current === applied) return;
    handled.current = applied;
    const outcome = outcomeOf(applied);
    if (outcome?.kind === "ok") spec?.after?.(outcome.data);
    reload();
  }, [applied, spec, reload]);

  return { flow, spec, refused, busy: flow.busy, begin };
}
