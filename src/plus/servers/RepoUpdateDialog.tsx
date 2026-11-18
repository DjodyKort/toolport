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
import { errorText } from "../settings/useCtl";
import { plural } from "../settings/model";
import {
  repoApply,
  repoPlan,
  repoUndo,
  restartServer,
  type RepoBuildChoice,
} from "./commands";
import type { RepoDoneData, RepoPlan } from "./data";

const COMMIT_LIMIT = 8;
const FILE_LIMIT = 6;

const short = (id: string | null) => (id ?? "").slice(0, 8);

type Stage =
  | { kind: "loading" }
  | { kind: "plan"; plan: RepoPlan; stale?: boolean }
  | { kind: "working"; what: string }
  | { kind: "done"; result: RepoDoneData; undone: boolean }
  | { kind: "error"; message: string };

function BuildSection({
  plan,
  run,
  command,
  onRun,
  onCommand,
}: {
  plan: RepoPlan;
  run: boolean;
  command: string;
  onRun: (on: boolean) => void;
  onCommand: (text: string) => void;
}) {
  return (
    <div className="flex flex-col gap-2 text-meta">
      <label className="flex items-start gap-2">
        <input
          type="checkbox"
          className="mt-1"
          checked={run}
          onChange={(e) => onRun(e.target.checked)}
        />
        <span>
          Run a build command in the checkout after the update
          <span className="block text-muted-foreground">
            It stops after {plan.build.timeoutSeconds / 60} minutes. Without it only the
            files change.
            {plan.build.remembered &&
              " You agreed to this command for this server before."}
          </span>
        </span>
      </label>
      {run && (
        <input
          aria-label="Build command"
          className="rounded-md border border-input bg-background px-2 py-1 font-mono text-meta"
          value={command}
          placeholder="./build.sh"
          onChange={(e) => onCommand(e.target.value)}
        />
      )}
      {run && plan.build.remembered && command !== plan.build.command && (
        <p className="text-warning">
          A different command is a new yes: it runs once you press Update.
        </p>
      )}
    </div>
  );
}

function PlanView({
  plan,
  stale,
  run,
  command,
  onRun,
  onCommand,
}: {
  plan: RepoPlan;
  stale?: boolean;
  run: boolean;
  command: string;
  onRun: (on: boolean) => void;
  onCommand: (text: string) => void;
}) {
  if (plan.refusal) {
    return (
      <p role="alert" className="flex items-start gap-2 text-meta">
        <TriangleAlert className="mt-0.5 size-4 shrink-0 text-warning" />
        <span>
          {plan.refusal.code === "up_to_date"
            ? "Nothing to update: "
            : "Toolport will not update this checkout: "}
          {plan.refusal.message}.
        </span>
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-3">
      {stale && (
        <p role="status" className="text-meta text-warning">
          The checkout changed while this was open. Review the new plan.
        </p>
      )}
      <section aria-label="Commits" className="flex flex-col gap-1 text-meta">
        <span className="font-semibold">
          {plural(plan.commitsTotal, "commit")} on {plan.upstream}
        </span>
        <ul className="flex flex-col gap-0.5">
          {plan.commits.slice(0, COMMIT_LIMIT).map((commit) => (
            <li key={commit.id} className="flex gap-2">
              <code className="text-muted-foreground">{commit.id}</code>
              <span className="break-words">{commit.subject}</span>
            </li>
          ))}
        </ul>
        {plan.commitsTotal > COMMIT_LIMIT && (
          <span className="text-muted-foreground">
            and {plan.commitsTotal - COMMIT_LIMIT} more
          </span>
        )}
      </section>
      <section aria-label="Files" className="flex flex-col gap-1 text-meta">
        <span className="font-semibold">{plural(plan.filesTotal, "file")} change</span>
        <ul className="flex flex-col gap-0.5">
          {plan.files.slice(0, FILE_LIMIT).map((file) => (
            <li key={file.path} className="flex gap-2">
              <code className="w-4 text-muted-foreground">{file.status}</code>
              <span className="break-all">{file.path}</span>
            </li>
          ))}
        </ul>
        {plan.filesTotal > FILE_LIMIT && (
          <span className="text-muted-foreground">
            and {plan.filesTotal - FILE_LIMIT} more
          </span>
        )}
      </section>
      <BuildSection
        plan={plan}
        run={run}
        command={command}
        onRun={onRun}
        onCommand={onCommand}
      />
      {plan.sharedWith.length > 0 && (
        <p className="text-meta text-muted-foreground">
          {plan.sharedWith.join(", ")} run from the same checkout and get the new files
          too.
        </p>
      )}
      <p className="text-meta text-muted-foreground">
        {plan.inFlight && plan.inFlight > 0
          ? `${plural(plan.inFlight, "call")} running now. The restart waits until they end; you can restart at once from the result.`
          : "Then the server restarts. Nothing is running that a restart would cut."}{" "}
        The branch only fast-forwards; nothing else in the checkout is touched.
      </p>
    </div>
  );
}

function ResultView({
  result,
  undone,
  onRestartNow,
}: {
  result: RepoDoneData;
  undone: boolean;
  onRestartNow: () => void;
}) {
  const build = result.build;
  const verb = undone ? "Moved back" : "Updated";
  return (
    <div className="flex flex-col gap-2 text-meta" role="status">
      <p className="flex items-center gap-2 font-semibold">
        <Check className="size-4 text-success" />
        {result.applied
          ? `${verb} ${result.server}: ${short(result.from)} to ${short(result.to)}`
          : `${result.server} is already up to date`}
      </p>
      {build && (
        <div className="flex flex-col gap-1">
          <p className={build.ok ? "" : "text-destructive"}>
            Build {build.ok ? "finished" : "failed"} in{" "}
            {Math.max(1, Math.round(build.elapsedMs / 1000))} s:{" "}
            <code>{build.command}</code>
          </p>
          {!build.ok && build.output && (
            <pre className="max-h-40 overflow-auto rounded-md bg-muted p-2 whitespace-pre-wrap">
              {build.output}
            </pre>
          )}
        </div>
      )}
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

/** The plan of updating the checkout a server runs from, the one confirm, and the result with its
 * Undo. Opens from the Repo row and from the attention row's Review button. */
export default function RepoUpdateDialog({
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
  const [run, setRun] = useState(false);
  const [command, setCommand] = useState("");

  useEffect(() => {
    let alive = true;
    repoPlan(serverName).then(
      ({ plan }) => {
        if (!alive) return;
        setRun(plan.build.command !== null);
        setCommand(plan.build.command ?? "");
        setStage({ kind: "plan", plan });
      },
      (error: unknown) => alive && setStage({ kind: "error", message: errorText(error) }),
    );
    return () => {
      alive = false;
    };
  }, [serverName]);

  const choice = (plan: RepoPlan): RepoBuildChoice => {
    const text = command.trim();
    if (!run || text === "") return { command: null, explicit: true };
    return {
      command: text,
      explicit: text !== plan.build.command || !plan.build.remembered,
    };
  };

  async function apply(plan: RepoPlan) {
    setStage({ kind: "working", what: `Updating ${serverName}` });
    try {
      const picked = choice(plan);
      const fresh = (await repoPlan(serverName, picked)).plan;
      if (fresh.from !== plan.from || fresh.to !== plan.to) {
        setStage({ kind: "plan", plan: fresh, stale: true });
        return;
      }
      const result = await repoApply(serverName, {
        planHash: fresh.planHash,
        build: picked,
      });
      setStage({ kind: "done", result, undone: false });
      onChanged?.();
    } catch (error) {
      setStage({ kind: "error", message: errorText(error) });
    }
  }

  async function undo() {
    setStage({ kind: "working", what: `Moving ${serverName} back` });
    try {
      setStage({ kind: "done", result: await repoUndo(serverName), undone: true });
      onChanged?.();
    } catch (error) {
      setStage({ kind: "error", message: errorText(error) });
    }
  }

  async function restartNow(result: RepoDoneData) {
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
  const canApply =
    plan !== null && plan.refusal === null && (!run || command.trim() !== "");
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {stage.kind === "done"
              ? stage.undone
                ? `${serverName} moved back`
                : `${serverName} updated`
              : `Update ${serverName}`}
          </DialogTitle>
          <DialogDescription>
            {plan && !plan.refusal
              ? `Fast-forward ${plan.branch} ${short(plan.from)} to ${short(plan.to)} in ${plan.root}`
              : "The checkout this server runs from"}
          </DialogDescription>
        </DialogHeader>
        {stage.kind === "loading" && (
          <p className="flex items-center gap-2 text-meta text-muted-foreground">
            <Loader2 className="size-4 animate-spin" /> Reading the checkout
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
            run={run}
            command={command}
            onRun={setRun}
            onCommand={setCommand}
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
