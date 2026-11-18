import { useEffect, useState } from "react";
import { Check, Loader2, TriangleAlert, Undo2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { errorText } from "../settings/useCtl";
import { switchApply, switchBack, switchPlan } from "./commands";
import type { RepoDoneData, RepoSwitchPlan } from "./data";

const short = (id: string | null) => (id ?? "").slice(0, 8);

type Stage =
  | { kind: "loading" }
  | { kind: "plan"; plan: RepoSwitchPlan }
  | { kind: "working"; what: string }
  | { kind: "done"; result: RepoDoneData; undone: boolean }
  | { kind: "error"; message: string };

/** Switching the checkout a server runs from to the branch another machine is on: the plan, the
 * one confirm, then the result with its Undo. Nothing is stashed, forced or pushed. */
export default function RepoSwitchDialog({
  serverName,
  open,
  onOpenChange,
  onChanged,
}: {
  serverName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChanged?: () => void;
}) {
  const [stage, setStage] = useState<Stage>({ kind: "loading" });

  useEffect(() => {
    let alive = true;
    switchPlan(serverName).then(
      ({ plan }) => alive && setStage({ kind: "plan", plan }),
      (error: unknown) => alive && setStage({ kind: "error", message: errorText(error) }),
    );
    return () => {
      alive = false;
    };
  }, [serverName]);

  async function run(what: string, work: () => Promise<RepoDoneData>, undone: boolean) {
    setStage({ kind: "working", what });
    try {
      setStage({ kind: "done", result: await work(), undone });
      onChanged?.();
    } catch (error) {
      setStage({ kind: "error", message: errorText(error) });
    }
  }

  const plan = stage.kind === "plan" ? stage.plan : null;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Switch {serverName} to another branch</DialogTitle>
          <DialogDescription>
            {plan
              ? `${plan.machine} is on ${plan.toBranch} at ${short(plan.toHead)}`
              : "The branch another machine is on"}
          </DialogDescription>
        </DialogHeader>
        {(stage.kind === "loading" || stage.kind === "working") && (
          <p className="flex items-center gap-2 text-meta text-muted-foreground">
            <Loader2 className="size-4 animate-spin" />
            {stage.kind === "loading" ? "Fetching and reading the checkout" : stage.what}
          </p>
        )}
        {stage.kind === "error" && (
          <p role="alert" className="text-meta text-destructive">
            {stage.message}
          </p>
        )}
        {plan?.refusal && (
          <p role="alert" className="flex items-start gap-2 text-meta">
            <TriangleAlert className="mt-0.5 size-4 shrink-0 text-warning" />
            <span>Toolport will not switch this checkout: {plan.refusal.message}.</span>
          </p>
        )}
        {plan && !plan.refusal && (
          <div className="flex flex-col gap-2 text-meta">
            <p>
              Checks out <code>{plan.toBranch}</code> in {plan.root}; you leave{" "}
              <code>{plan.fromBranch}</code> at {short(plan.fromHead)} with every commit
              kept.
              {plan.createsBranch &&
                " A local branch that tracks the remote one is made."}
            </p>
            <p className="text-muted-foreground">
              {plan.build.command
                ? `Then it runs ${plan.build.command} and restarts the server. `
                : "Then the server restarts. "}
              Nothing is stashed, forced or pushed.
            </p>
          </div>
        )}
        {stage.kind === "done" && (
          <div className="flex flex-col gap-2 text-meta" role="status">
            <p className="flex items-center gap-2 font-semibold">
              <Check className="size-4 text-success" />
              {stage.undone
                ? `${serverName} is back on ${stage.result.branch}`
                : `${serverName} is on ${stage.result.branch}`}
            </p>
            {stage.result.restart && <p>{stage.result.restart.detail}</p>}
            {stage.result.warnings.map((warning) => (
              <p key={warning} className="text-warning">
                {warning}
              </p>
            ))}
          </div>
        )}
        <DialogFooter>
          {stage.kind === "done" ? (
            <>
              {!stage.undone && (
                <Button
                  variant="outline"
                  onClick={() =>
                    void run(
                      `Moving ${serverName} back`,
                      () => switchBack(serverName),
                      true,
                    )
                  }
                >
                  <Undo2 /> Undo
                </Button>
              )}
              <Button onClick={() => onOpenChange(false)}>Close</Button>
            </>
          ) : (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              {plan && !plan.refusal && (
                <Button
                  onClick={() =>
                    void run(
                      `Switching ${serverName}`,
                      () => switchApply(serverName, plan.planHash),
                      false,
                    )
                  }
                >
                  Switch to {plan.toBranch}
                </Button>
              )}
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
