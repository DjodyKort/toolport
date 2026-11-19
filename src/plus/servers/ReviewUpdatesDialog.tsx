import { useEffect, useState } from "react";
import { Check, Loader2, RotateCw, TriangleAlert, Undo2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { CtlError } from "../bridge/ctl";
import { errorText } from "../settings/useCtl";
import { plural } from "../settings/model";
import { batchApply, batchPlan, batchUndo, restartServer } from "./commands";
import type {
  PackageBatchDone,
  PackageBatchPlan,
  PackageBatchResult,
  PackageUpdatePlan,
} from "./data";
import { shortVersion } from "./format";

type Stage =
  | { kind: "loading" }
  | { kind: "plan"; batch: PackageBatchPlan; stale?: boolean }
  | { kind: "working"; what: string }
  | { kind: "done"; done: PackageBatchDone; names: string[]; undone: boolean }
  | { kind: "error"; message: string };

/** A release without a checksum stays unticked until the person allows it. */
const needsAllow = (plan: PackageUpdatePlan) =>
  plan.release !== null && plan.release.checksum === null;

const runnable = (plan: PackageUpdatePlan) => plan.refusal === null;

function PlanItem({
  plan,
  ticked,
  onTick,
}: {
  plan: PackageUpdatePlan;
  ticked: boolean;
  onTick: (on: boolean) => void;
}) {
  const refused = plan.refusal !== null;
  return (
    <li className="flex flex-col gap-1 border-t py-2 first:border-t-0">
      <label className="flex items-start gap-2">
        <input
          type="checkbox"
          className="mt-1"
          aria-label={`Update ${plan.server}`}
          checked={ticked}
          disabled={refused}
          onChange={(e) => onTick(e.target.checked)}
        />
        <span className="min-w-0 flex-1 text-meta">
          <span className="font-semibold">{plan.server}</span>{" "}
          <span className="text-muted-foreground">
            {plan.kind === "github" ? "GitHub release" : plan.kind}
            {plan.digest ? " image" : ""} {plan.package}
          </span>
          {refused ? (
            <span className="mt-1 flex items-start gap-1 text-warning">
              <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
              {plan.refusal?.message}
            </span>
          ) : (
            <>
              <span className="block">
                {shortVersion(plan.from)} to {shortVersion(plan.to)}
                {plan.release && ` · ${plan.release.asset.name}`}
              </span>
              {plan.changes.map((change) => (
                <span
                  key={change.field}
                  className="block break-all text-muted-foreground"
                >
                  <code>{change.field}</code>: <code>{change.from}</code> to{" "}
                  <code>{change.to}</code>
                </span>
              ))}
              {needsAllow(plan) && (
                <span className="block text-warning">
                  No checksum is published, so the download cannot be verified. It runs
                  only while this is ticked.
                </span>
              )}
            </>
          )}
        </span>
      </label>
    </li>
  );
}

function ResultItem({
  item,
  onRestartNow,
}: {
  item: PackageBatchResult;
  onRestartNow: (server: string) => void;
}) {
  const result = item.result;
  const busy = result?.restart?.status === "busy";
  return (
    <li className="flex flex-col gap-0.5 border-t py-2 text-meta first:border-t-0">
      <span className="flex items-center gap-2 font-semibold">
        {item.ok ? (
          <Check className="size-4 text-success" />
        ) : (
          <TriangleAlert className="size-4 text-destructive" />
        )}
        {item.server}
      </span>
      {result && result.applied && (
        <span>
          {result.package} {shortVersion(result.from)} to {shortVersion(result.to)}
        </span>
      )}
      {result && !result.applied && <span>Already on {shortVersion(result.from)}</span>}
      {item.error && <span className="text-destructive">{item.error.message}</span>}
      {result?.restart &&
        (busy ? (
          <span>
            A call is still running, so it was not restarted.{" "}
            <Button size="sm" variant="outline" onClick={() => onRestartNow(item.server)}>
              <RotateCw /> Restart now
            </Button>
          </span>
        ) : (
          <span>{result.restart.detail}</span>
        ))}
      {result?.warnings.map((warning) => (
        <span key={warning} className="text-warning">
          {warning}
        </span>
      ))}
    </li>
  );
}

/** One plan for several servers: every server's change listed, the ones to run ticked, one
 * confirm, then each server's own result and restart state and one Undo for all of them. */
export default function ReviewUpdatesDialog({
  servers,
  open,
  onOpenChange,
  onChanged,
}: {
  servers: string[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChanged?: () => void;
}) {
  const [stage, setStage] = useState<Stage>({ kind: "loading" });
  const [ticked, setTicked] = useState<Record<string, boolean>>({});

  function show(batch: PackageBatchPlan, stale = false) {
    setTicked(
      Object.fromEntries(
        batch.plans.map((plan) => [plan.server, runnable(plan) && !needsAllow(plan)]),
      ),
    );
    setStage({ kind: "plan", batch, stale });
  }

  const names = servers.join("\u0000");
  useEffect(() => {
    let alive = true;
    batchPlan(names.split("\u0000"), { fetch: true }).then(
      ({ batch }) => alive && show(batch),
      (error: unknown) => alive && setStage({ kind: "error", message: errorText(error) }),
    );
    return () => {
      alive = false;
    };
  }, [names]);

  const picked = (batch: PackageBatchPlan) =>
    batch.plans.filter((plan) => ticked[plan.server]);

  async function apply(batch: PackageBatchPlan) {
    const chosen = picked(batch);
    const chosenNames = chosen.map((plan) => plan.server);
    setStage({ kind: "working", what: `Updating ${plural(chosen.length, "server")}` });
    try {
      // The hash of a plan over fewer servers is another hash, so the chosen ones are planned
      // again; if that differs from what was reviewed the person sees the new plan first.
      const fresh = (await batchPlan(chosenNames)).batch;
      const unchanged = fresh.plans.every((plan) => {
        const seen = batch.plans.find((p) => p.server === plan.server);
        return seen?.planHash === plan.planHash;
      });
      if (!unchanged) {
        show(fresh, true);
        return;
      }
      const done = await batchApply(fresh, { allowUnverified: chosen.some(needsAllow) });
      setStage({ kind: "done", done, names: chosenNames, undone: false });
      onChanged?.();
    } catch (error) {
      if (error instanceof CtlError && error.code === "conflict") {
        try {
          show((await batchPlan(servers, { fetch: true })).batch, true);
          return;
        } catch {
          // The first failure says more.
        }
      }
      setStage({ kind: "error", message: errorText(error) });
    }
  }

  async function undo(done: PackageBatchDone, chosen: string[]) {
    const applied = done.results.filter((r) => r.result?.applied).map((r) => r.server);
    setStage({ kind: "working", what: "Putting the servers back" });
    try {
      const back = await batchUndo(applied.length > 0 ? applied : chosen);
      setStage({ kind: "done", done: back, names: chosen, undone: true });
      onChanged?.();
    } catch (error) {
      setStage({ kind: "error", message: errorText(error) });
    }
  }

  async function restartNow(server: string, done: PackageBatchDone, chosen: string[]) {
    try {
      await restartServer(server);
      setStage({
        kind: "done",
        names: chosen,
        undone: false,
        done: {
          ...done,
          results: done.results.map((r) =>
            r.server === server && r.result
              ? {
                  ...r,
                  result: {
                    ...r.result,
                    restart: { status: "restarted", detail: "Restarted" },
                  },
                }
              : r,
          ),
        },
      });
    } catch (error) {
      setStage({ kind: "error", message: errorText(error) });
    }
  }

  const batch = stage.kind === "plan" ? stage.batch : null;
  const count = batch ? picked(batch).length : 0;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>
            {stage.kind === "done"
              ? stage.undone
                ? "Servers put back"
                : "Updates finished"
              : "Review updates"}
          </DialogTitle>
          <DialogDescription>
            {batch
              ? "Only the ticked servers run, and only the changes listed here."
              : "The servers with an update waiting"}
          </DialogDescription>
        </DialogHeader>
        {stage.kind === "loading" && (
          <p className="flex items-center gap-2 text-meta text-muted-foreground">
            <Loader2 className="size-4 animate-spin" /> Asking for the newest versions
          </p>
        )}
        {stage.kind === "working" && (
          <p className="flex items-center gap-2 text-meta text-muted-foreground">
            <Loader2 className="size-4 animate-spin" /> {stage.what}
          </p>
        )}
        {stage.kind === "error" && (
          <p role="alert" className="text-meta text-destructive">
            {stage.message}
          </p>
        )}
        {stage.kind === "plan" && (
          <div className="flex flex-col gap-2">
            {stage.stale && (
              <p role="status" className="text-meta text-warning">
                The plan changed while this was open. Review the new plan.
              </p>
            )}
            <ul aria-label="Servers to update" className="max-h-96 overflow-auto">
              {stage.batch.plans.map((plan) => (
                <PlanItem
                  key={plan.id}
                  plan={plan}
                  ticked={ticked[plan.server] === true}
                  onTick={(on) =>
                    setTicked((current) => ({ ...current, [plan.server]: on }))
                  }
                />
              ))}
            </ul>
            <p className="text-meta text-muted-foreground">
              Each server restarts after its own update, unless a call is running. Undo
              puts all of them back at once.
            </p>
          </div>
        )}
        {stage.kind === "done" && (
          <div role="status" className="flex flex-col gap-2">
            <p className="text-meta font-semibold">
              {stage.undone ? "Put back" : "Updated"}{" "}
              {plural(stage.done.applied, "server")}
              {stage.done.failed > 0 && `, ${stage.done.failed} did not go through`}
            </p>
            <ul aria-label="Results" className="max-h-96 overflow-auto">
              {stage.done.results.map((item) => (
                <ResultItem
                  key={item.id || item.server}
                  item={item}
                  onRestartNow={(server) =>
                    void restartNow(server, stage.done, stage.names)
                  }
                />
              ))}
            </ul>
          </div>
        )}
        <DialogFooter>
          {stage.kind === "done" ? (
            <>
              {stage.done.applied > 0 && !stage.undone && (
                <Button
                  variant="outline"
                  onClick={() => void undo(stage.done, stage.names)}
                >
                  <Undo2 /> Undo all
                </Button>
              )}
              <Button onClick={() => onOpenChange(false)}>Close</Button>
            </>
          ) : (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              {batch && (
                <Button disabled={count === 0} onClick={() => void apply(batch)}>
                  Update {plural(count, "server")}
                </Button>
              )}
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
