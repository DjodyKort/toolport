import { useEffect, useState } from "react";
import {
  Check,
  CircleAlert,
  Loader2,
  Package,
  RotateCw,
  TriangleAlert,
  Undo2,
  X,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Badge } from "@/components/ui/badge";
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
import { clock, shortVersion } from "./format";

type Stage =
  | { kind: "loading" }
  | { kind: "plan"; batch: PackageBatchPlan; checkedAt: number; stale?: boolean }
  | { kind: "working"; what: string }
  | { kind: "done"; done: PackageBatchDone; names: string[]; undone: boolean; at: number }
  | { kind: "error"; message: string };

/** What a server's restart after an update answered: `busy` is a call still running. */
type Restarted = { status: "restarted" | "busy" | "failed"; detail: string };

const KIND: Record<PackageUpdatePlan["kind"], string> = {
  npm: "npm",
  pypi: "PyPI",
  docker: "Docker image",
  github: "GitHub release",
};

const WAITING_SENTENCE =
  "One at a time. You can close this window: the update keeps running.";

/** A release without a checksum stays unticked until the person allows it. */
const needsAllow = (plan: PackageUpdatePlan) =>
  plan.release !== null && plan.release.checksum === null;

const runnable = (plan: PackageUpdatePlan) => plan.refusal === null;

const megabytes = (size: number) => `${Math.max(1, Math.round(size / 1_000_000))} MB`;

function versionLine(plan: PackageUpdatePlan) {
  const range = `${plan.digest ? "Digest " : ""}${shortVersion(plan.from)} to ${shortVersion(plan.to)}`;
  if (plan.release) {
    return plan.release.checksum ? (
      <>{range} · replaces the binary in place</>
    ) : (
      <>
        {range} · <span className="text-warning">no checksum published</span>
      </>
    );
  }
  return <>{plan.downgrade ? `${range} (an older version)` : range}</>;
}

function PlanItem({
  plan,
  ticked,
  allowed,
  onTick,
  onAllow,
}: {
  plan: PackageUpdatePlan;
  ticked: boolean;
  allowed: boolean;
  onTick: (on: boolean) => void;
  onAllow: (on: boolean) => void;
}) {
  const refusal = plan.refusal;
  const release = plan.release;
  const locked = refusal !== null || (needsAllow(plan) && !allowed);
  return (
    <li className="flex flex-col gap-2 rounded-lg border p-3">
      <label className="flex items-start gap-3">
        <input
          type="checkbox"
          className="mt-2"
          aria-label={`Update ${plan.server}`}
          checked={ticked}
          disabled={locked}
          onChange={(e) => onTick(e.target.checked)}
        />
        <span
          aria-hidden="true"
          className="grid size-8 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground [&_svg]:size-4"
        >
          <Package />
        </span>
        <span className="min-w-0 flex-1 text-meta">
          <span className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-semibold break-words">{plan.server}</span>
            <Badge variant="off" className="uppercase">
              {KIND[plan.kind]}
            </Badge>
          </span>
          <span className="block break-words text-muted-foreground">
            {refusal ? plan.package : versionLine(plan)}
          </span>
        </span>
      </label>
      {refusal && (
        <p
          className={`flex items-start gap-2 text-meta ${refusal.code === "up_to_date" ? "text-muted-foreground" : "text-warning"}`}
        >
          <TriangleAlert className="mt-0.5 size-3.5 shrink-0" />
          <span>
            {refusal.code === "up_to_date"
              ? "Nothing to update: "
              : "Toolport cannot update this server: "}
            {refusal.message}.
          </span>
        </p>
      )}
      {!refusal && (
        <div className="flex flex-col gap-2 text-meta text-muted-foreground">
          {plan.digest ? (
            <p>
              Pins the new digest in the server&apos;s command. The image is pulled on the
              next start, so the first call after a restart is slower.
            </p>
          ) : (
            <ul className="flex flex-col gap-0.5">
              {plan.changes.map((change) => (
                <li key={change.field} className="break-all">
                  <code>{change.field}</code>: <code>{change.from}</code> to{" "}
                  <code>{change.to}</code>
                </li>
              ))}
            </ul>
          )}
          {release && (
            <p className="break-words">
              <code>{release.asset.name}</code> ({megabytes(release.asset.size)}) replaces{" "}
              <code className="break-all">{release.file}</code>
            </p>
          )}
          {release?.checksum && (
            <p>
              Checked against {release.checksum.source} (sha256) before it replaces the
              binary. The old binary is kept for Undo.
            </p>
          )}
          {release && needsAllow(plan) && (
            <>
              <p
                role="note"
                className="flex items-start gap-2 rounded-md border border-warning/40 bg-warning/12 p-2 text-foreground"
              >
                <CircleAlert className="mt-0.5 size-3.5 shrink-0 text-warning" />
                <span>
                  The publisher offers no checksum, so the download cannot be verified. It
                  stays unticked until you allow it.
                </span>
              </p>
              <label className="flex items-start gap-2 text-foreground">
                <input
                  type="checkbox"
                  className="mt-1"
                  checked={allowed}
                  onChange={(e) => onAllow(e.target.checked)}
                />
                <span>
                  Install without verification
                  <span className="block text-muted-foreground">
                    Only if you trust the source of this release.
                  </span>
                </span>
              </label>
            </>
          )}
          {release && (
            <p>
              {release.verifyCommand
                ? "The new binary has to pass the source's check command, or the previous one is put back."
                : "The new binary has to be an executable file, or the previous one is put back."}
            </p>
          )}
          {!plan.verified && (
            <p>The registry was not asked whether {shortVersion(plan.to)} exists.</p>
          )}
        </div>
      )}
    </li>
  );
}

function ResultItem({
  item,
  undone,
  restarted,
  onRestartNow,
}: {
  item: PackageBatchResult;
  undone: boolean;
  restarted: Restarted | undefined;
  onRestartNow: (server: string) => void;
}) {
  const result = item.result;
  const restart = restarted ?? result?.restart ?? null;
  const busy = restart?.status === "busy";
  return (
    <li className="flex flex-col gap-0.5 text-meta">
      <span className="flex items-start gap-2">
        {item.ok ? (
          <Check className="mt-0.5 size-4 shrink-0 text-success" />
        ) : (
          <X className="mt-0.5 size-4 shrink-0 text-destructive" />
        )}
        <span className="min-w-0 break-words">
          <span className="font-semibold">{item.server}</span>
          {" · "}
          {result?.applied ? (
            `${undone ? "put back to" : "updated to"} ${shortVersion(result.to)}`
          ) : result ? (
            `already on ${shortVersion(result.from)}`
          ) : (
            <span className="text-destructive">{item.error?.message}</span>
          )}
        </span>
      </span>
      {restart && (
        <span className="pl-6 text-muted-foreground">
          {busy ? (
            <>
              A call is still running, so it was not restarted.{" "}
              <Button
                size="sm"
                variant="outline"
                onClick={() => onRestartNow(item.server)}
              >
                <RotateCw /> Restart now
              </Button>
            </>
          ) : (
            restart.detail
          )}
        </span>
      )}
      {result?.warnings.map((warning) => (
        <span key={warning} className="pl-6 text-warning">
          {warning}
        </span>
      ))}
    </li>
  );
}

/** The plan of the package and release updates of one or several servers: every server's change
 * listed, the ones to run ticked, one confirm, then each server's result and one Undo for all.
 * Nothing restarts by itself; the result asks, and the restart waits for a running call. */
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
  const [allowed, setAllowed] = useState<Record<string, boolean>>({});
  const [restarts, setRestarts] = useState<Record<string, Restarted>>({});

  function show(batch: PackageBatchPlan, stale = false) {
    setTicked(
      Object.fromEntries(
        batch.plans.map((plan) => [plan.server, runnable(plan) && !needsAllow(plan)]),
      ),
    );
    setAllowed({});
    setStage({ kind: "plan", batch, checkedAt: Date.now(), stale });
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

  function finished(done: PackageBatchDone, chosen: string[], undone: boolean) {
    setRestarts({});
    setStage({ kind: "done", done, names: chosen, undone, at: Date.now() });
    onChanged?.();
  }

  async function apply(batch: PackageBatchPlan) {
    const chosen = picked(batch);
    const chosenNames = chosen.map((plan) => plan.server);
    setStage({
      kind: "working",
      what:
        chosen.length === 1
          ? `Updating ${chosenNames[0]}`
          : `Updating ${plural(chosen.length, "server")}`,
    });
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
      finished(done, chosenNames, false);
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
      finished(await batchUndo(applied.length > 0 ? applied : chosen), chosen, true);
    } catch (error) {
      setStage({ kind: "error", message: errorText(error) });
    }
  }

  async function restart(targets: string[], force: boolean) {
    const answers = await Promise.all(
      targets.map(async (server): Promise<[string, Restarted]> => {
        try {
          const data = await restartServer(server, { ifIdle: !force });
          if (data.status === "busy")
            return [server, { status: "busy", detail: "A call is still running." }];
          return [
            server,
            {
              status: data.status === "failed" ? "failed" : "restarted",
              detail:
                data.status === "not_running"
                  ? "No gateway holds it right now; it starts fresh on its next call."
                  : data.status === "failed"
                    ? "A gateway could not restart it."
                    : "Restarted",
            },
          ];
        } catch (error) {
          return [server, { status: "failed", detail: errorText(error) }];
        }
      }),
    );
    setRestarts((current) => ({ ...current, ...Object.fromEntries(answers) }));
  }

  const batch = stage.kind === "plan" ? stage.batch : null;
  const count = batch ? picked(batch).length : 0;
  const single = batch?.plans.length === 1 ? batch.plans[0].server : null;
  const outcome =
    stage.kind === "done"
      ? {
          pending: stage.done.results
            .filter((r) => r.result?.applied)
            .map((r) => r.server)
            .filter((server) => {
              const status = (
                restarts[server] ??
                stage.done.results.find((r) => r.server === server)?.result?.restart
              )?.status;
              return status !== "restarted";
            }),
        }
      : null;
  const title =
    stage.kind === "done"
      ? stage.names.length === 1
        ? `${stage.names[0]} ${stage.undone ? "put back" : "updated"}`
        : `${stage.done.applied} ${stage.undone ? "put back" : "updated"}${stage.done.failed > 0 ? `, ${stage.done.failed} failed` : ""}`
      : stage.kind === "working"
        ? stage.what
        : "Review updates";
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>
            {stage.kind === "plan"
              ? `Checked ${clock(stage.checkedAt)}. Only the ticked items run, and only the steps you see here.`
              : stage.kind === "working"
                ? WAITING_SENTENCE
                : stage.kind === "done"
                  ? clock(stage.at)
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
            <Loader2 className="size-4 animate-spin" /> Applying the reviewed plan
          </p>
        )}
        {stage.kind === "error" && (
          <p role="alert" className="text-meta text-destructive">
            {stage.message}
          </p>
        )}
        {stage.kind === "plan" && (
          <div className="flex flex-col gap-3">
            {stage.stale && (
              <p role="status" className="text-meta text-warning">
                The plan changed while this was open. Review the new plan.
              </p>
            )}
            <ul
              aria-label="Servers to update"
              className="flex max-h-[60vh] flex-col gap-3 overflow-auto"
            >
              {stage.batch.plans.map((plan) => (
                <PlanItem
                  key={plan.id}
                  plan={plan}
                  ticked={ticked[plan.server] === true}
                  allowed={allowed[plan.server] === true}
                  onTick={(on) =>
                    setTicked((current) => ({ ...current, [plan.server]: on }))
                  }
                  onAllow={(on) => {
                    setAllowed((current) => ({ ...current, [plan.server]: on }));
                    setTicked((current) => ({ ...current, [plan.server]: on }));
                  }}
                />
              ))}
            </ul>
            <p className="text-meta text-muted-foreground">
              Does not restart anything. You will be asked after.
              {stage.batch.plans.length > 1 && " Undo puts all of them back at once."}
            </p>
          </div>
        )}
        {stage.kind === "done" && outcome && (
          <div role="status" className="flex flex-col gap-3">
            <ul
              aria-label="Results"
              className="flex max-h-[60vh] flex-col gap-2 overflow-auto"
            >
              {stage.done.results.map((item) => (
                <ResultItem
                  key={item.id || item.server}
                  item={item}
                  undone={stage.undone}
                  restarted={restarts[item.server]}
                  onRestartNow={(server) => void restart([server], true)}
                />
              ))}
            </ul>
            {outcome.pending.length > 0 && (
              <p className="text-meta text-muted-foreground">
                {outcome.pending.join(" and ")} still run the{" "}
                {stage.undone ? "newer" : "old"} version until restarted.
              </p>
            )}
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
                  <Undo2 /> {stage.names.length === 1 ? "Undo" : "Undo all"}
                </Button>
              )}
              <Button
                variant={outcome && outcome.pending.length > 0 ? "outline" : "default"}
                onClick={() => onOpenChange(false)}
              >
                Close
              </Button>
              {outcome && outcome.pending.length > 0 && (
                <Button onClick={() => void restart(outcome.pending, false)}>
                  Restart {outcome.pending.length}
                </Button>
              )}
            </>
          ) : (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Cancel
              </Button>
              {batch && (
                <Button disabled={count === 0} onClick={() => void apply(batch)}>
                  Update {single && count === 1 ? single : plural(count, "server")}
                </Button>
              )}
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
