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
import { packageApply, packagePlan, packageUndo, restartServer } from "./commands";
import type { PackageUpdateDone, PackageUpdatePlan } from "./data";
import { shortVersion } from "./format";

const REGISTRY: Record<PackageUpdatePlan["kind"], string> = {
  npm: "npm",
  pypi: "PyPI",
  docker: "Docker Hub",
  github: "GitHub",
};

type Stage =
  | { kind: "loading" }
  | { kind: "plan"; plan: PackageUpdatePlan; stale?: boolean }
  | { kind: "working"; what: string }
  | { kind: "done"; result: PackageUpdateDone; undone: boolean }
  | { kind: "error"; message: string };

function megabytes(size: number): string {
  return `${Math.max(1, Math.round(size / 1_000_000))} MB`;
}

function PlanView({
  plan,
  stale,
  allowUnverified,
  onAllow,
}: {
  plan: PackageUpdatePlan;
  stale?: boolean;
  allowUnverified: boolean;
  onAllow: (on: boolean) => void;
}) {
  if (plan.refusal) {
    return (
      <p role="alert" className="flex items-start gap-2 text-meta">
        <TriangleAlert className="mt-0.5 size-4 shrink-0 text-warning" />
        <span>
          {plan.refusal.code === "up_to_date"
            ? "Nothing to update: "
            : "Toolport cannot update this server: "}
          {plan.refusal.message}.
        </span>
      </p>
    );
  }
  const release = plan.release;
  return (
    <div className="flex flex-col gap-3">
      {stale && (
        <p role="status" className="text-meta text-warning">
          The plan changed while this was open. Review the new plan.
        </p>
      )}
      <section aria-label="Registry change" className="flex flex-col gap-1 text-meta">
        <span className="font-semibold">
          {plan.downgrade ? "Go back" : "Update"} {plan.package} from{" "}
          {shortVersion(plan.from)} to {shortVersion(plan.to)}
        </span>
        <span className="text-muted-foreground">
          The registry entry changes in {plural(plan.changes.length, "place")}; nothing
          else is touched.
        </span>
        <ul className="flex flex-col gap-1">
          {plan.changes.map((change) => (
            <li key={change.field} className="flex flex-col">
              <code className="text-muted-foreground">{change.field}</code>
              <span className="break-all">
                <code>{change.from}</code> to <code>{change.to}</code>
              </span>
            </li>
          ))}
        </ul>
      </section>
      {!plan.verified && (
        <p className="text-meta text-muted-foreground">
          {REGISTRY[plan.kind]} was not asked whether {shortVersion(plan.to)} exists.
        </p>
      )}
      {release && (
        <section aria-label="Release file" className="flex flex-col gap-1 text-meta">
          <span>
            <code>{release.asset.name}</code> ({megabytes(release.asset.size)}) replaces{" "}
            <code className="break-all">{release.file}</code>
          </span>
          {release.checksum ? (
            <span className="text-muted-foreground">
              Checked against {release.checksum.source} (sha256) before it replaces the
              binary. The old binary is kept for Undo.
            </span>
          ) : (
            <label className="flex items-start gap-2">
              <input
                type="checkbox"
                className="mt-1"
                checked={allowUnverified}
                onChange={(e) => onAllow(e.target.checked)}
              />
              <span>
                Install without verification
                <span className="block text-muted-foreground">
                  The publisher offers no checksum, so the download cannot be verified.
                  Only if you trust the source of this release.
                </span>
              </span>
            </label>
          )}
          <span className="text-muted-foreground">
            {release.verifyCommand
              ? "The new binary has to pass the source's check command, or the previous one is put back."
              : "The new binary has to be an executable file, or the previous one is put back."}
          </span>
        </section>
      )}
      <p className="text-meta text-muted-foreground">
        {plan.inFlight && plan.inFlight > 0
          ? `${plural(plan.inFlight, "call")} running now. The restart waits until they end; you can restart at once from the result.`
          : "Then the server restarts. Nothing is running that a restart would cut."}
      </p>
    </div>
  );
}

function ResultView({
  result,
  undone,
  onRestartNow,
}: {
  result: PackageUpdateDone;
  undone: boolean;
  onRestartNow: () => void;
}) {
  return (
    <div className="flex flex-col gap-2 text-meta" role="status">
      <p className="flex items-center gap-2 font-semibold">
        <Check className="size-4 text-success" />
        {result.applied
          ? `${undone ? "Put back" : "Updated"} ${result.server}: ${result.package} ${shortVersion(result.from)} to ${shortVersion(result.to)}`
          : `${result.server} is already on ${shortVersion(result.from)}`}
      </p>
      {result.restart && (
        <p>
          {result.restart.status === "busy" ? (
            <>
              A call is still running, so the server was not restarted.{" "}
              <Button size="sm" variant="outline" onClick={onRestartNow}>
                <RotateCw /> Restart now
              </Button>
            </>
          ) : (
            result.restart.detail
          )}
        </p>
      )}
      {result.warnings.map((warning) => (
        <p key={warning} className="text-warning">
          {warning}
        </p>
      ))}
    </div>
  );
}

/** The plan of moving a server to another version of its package or release, the one confirm,
 * and the result with its Undo. Opens from the Package row and from the Updates row's Update. */
export default function PackageUpdateDialog({
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
  const [allowUnverified, setAllowUnverified] = useState(false);

  useEffect(() => {
    let alive = true;
    packagePlan(serverName, { fetch: true }).then(
      ({ plan }) => alive && setStage({ kind: "plan", plan }),
      (error: unknown) => alive && setStage({ kind: "error", message: errorText(error) }),
    );
    return () => {
      alive = false;
    };
  }, [serverName]);

  async function apply(plan: PackageUpdatePlan) {
    setStage({ kind: "working", what: `Updating ${serverName}` });
    try {
      const result = await packageApply(serverName, {
        to: plan.to,
        planHash: plan.planHash,
        allowUnverified,
      });
      setStage({ kind: "done", result, undone: false });
      onChanged?.();
    } catch (error) {
      if (error instanceof CtlError && error.code === "conflict") {
        try {
          const fresh = await packagePlan(serverName, { to: plan.to, fetch: true });
          setStage({ kind: "plan", plan: fresh.plan, stale: true });
          return;
        } catch {
          // The message of the first failure is the more useful one.
        }
      }
      setStage({ kind: "error", message: errorText(error) });
    }
  }

  async function undo() {
    setStage({ kind: "working", what: `Putting ${serverName} back` });
    try {
      setStage({ kind: "done", result: await packageUndo(serverName), undone: true });
      onChanged?.();
    } catch (error) {
      setStage({ kind: "error", message: errorText(error) });
    }
  }

  async function restartNow(result: PackageUpdateDone) {
    try {
      await restartServer(serverName);
      setStage({
        kind: "done",
        result: { ...result, restart: { status: "restarted", detail: "Restarted" } },
        undone: false,
      });
    } catch (error) {
      setStage({ kind: "error", message: errorText(error) });
    }
  }

  const plan = stage.kind === "plan" ? stage.plan : null;
  const needsAllow = plan?.release != null && plan.release.checksum === null;
  const canApply =
    plan !== null && plan.refusal === null && (!needsAllow || allowUnverified);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {stage.kind === "done"
              ? stage.undone
                ? `${serverName} put back`
                : `${serverName} updated`
              : `Update ${serverName}`}
          </DialogTitle>
          <DialogDescription>
            {plan && !plan.refusal
              ? `${plan.package} on ${REGISTRY[plan.kind]}, ${shortVersion(plan.from)} to ${shortVersion(plan.to)}`
              : "The package or release this server runs"}
          </DialogDescription>
        </DialogHeader>
        {stage.kind === "loading" && (
          <p className="flex items-center gap-2 text-meta text-muted-foreground">
            <Loader2 className="size-4 animate-spin" /> Asking for the newest version
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
          <PlanView
            plan={stage.plan}
            stale={stage.stale}
            allowUnverified={allowUnverified}
            onAllow={setAllowUnverified}
          />
        )}
        {stage.kind === "done" && (
          <ResultView
            result={stage.result}
            undone={stage.undone}
            onRestartNow={() => void restartNow(stage.result)}
          />
        )}
        <DialogFooter>
          {stage.kind === "done" ? (
            <>
              {stage.result.applied && !stage.undone && (
                <Button variant="outline" onClick={() => void undo()}>
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
              {plan && (
                <Button disabled={!canApply} onClick={() => void apply(plan)}>
                  Update {serverName}
                </Button>
              )}
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
