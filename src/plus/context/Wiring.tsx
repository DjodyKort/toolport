import { useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Check, RefreshCw, Rocket } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import type { Loaded } from "../servers/useLoad";
import { disableWiring, initWiring, runPresync } from "./commands";
import type { WiringCheck, WiringStatusData } from "./data";
import { intervalText, secondsText, shortPath, whenText } from "./model";
import PlanDialog from "./PlanDialog";
import { Group, RowShell } from "./StackRows";

type Open = "init" | "fix" | "disable";

const LEVEL: Record<
  WiringCheck["level"],
  { label: string; tone: "success" | "warning" | "destructive" }
> = {
  ok: { label: "OK", tone: "success" },
  look: { label: "Look", tone: "warning" },
  fail: { label: "Fail", tone: "destructive" },
};

function offerUndo(done: string, undo: () => Promise<unknown>, reload: () => void) {
  toast.success(done, {
    duration: 10_000,
    action: {
      label: "Undo",
      onClick: () => {
        undo().then(reload, (error: unknown) =>
          toastError("Could not undo", { description: errorText(error) }),
        );
      },
    },
  });
}

function PresyncSummary({ status }: { status: WiringStatusData }) {
  const last = status.presync;
  if (!last) return <>The presync has not run yet; it runs before the next launch.</>;
  return (
    <>
      Presync ran {whenText(last.lastAttemptAt)} ({secondsText(last.durationMs)}).
      {last.nextAfter && ` Next run after ${whenText(last.nextAfter)}.`}
    </>
  );
}

function PresyncLine({ status }: { status: WiringStatusData }) {
  const last = status.presync;
  if (!last) return <>The presync has not run yet; it runs before the next launch.</>;
  return (
    <>
      Last run {whenText(last.lastAttemptAt)}
      {last.nextAfter && ` · next after ${whenText(last.nextAfter)}`} ·{" "}
      {secondsText(last.durationMs)},{" "}
      {last.warnings.length === 0 ? "no warnings" : last.warnings[0]}
    </>
  );
}

/** The health card of Rules & launch and the presync row under it. The tab leads with one
 * finding and its fix; the wiring runs unseen once it is set up. */
export default function Wiring({
  status,
  home,
  onSync,
  reloadAll,
}: {
  status: Loaded<WiringStatusData>;
  home: string | null;
  onSync: () => void;
  reloadAll: () => void;
}) {
  const [open, setOpen] = useState<Open | null>(null);
  const [showOutput, setShowOutput] = useState(false);
  const [showLines, setShowLines] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  const [busy, setBusy] = useState(false);
  const data = status.data;

  async function tryAgain() {
    setBusy(true);
    try {
      await runPresync(true);
      toast.success("The presync ran");
    } catch (error) {
      toastError("The presync failed again", { description: errorText(error) });
    } finally {
      setBusy(false);
      reloadAll();
    }
  }

  if (status.error && !data) {
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<AlertTriangle />}
        title="The launch wiring could not be read"
        detail={status.error}
        actions={
          <Button size="sm" variant="outline" onClick={status.reload}>
            Try again
          </Button>
        }
      />
    );
  }
  if (!data) return <p className="text-meta">Reading the launch wiring…</p>;

  const checks = data.checks.filter((c) => c.id !== "presync");
  const finding = checks.find((c) => c.level !== "ok");
  const order = checks.find((c) => c.id === "source-order" && c.level !== "ok");
  const failed = data.state === "failed" && data.presync;

  return (
    <div className="flex flex-col gap-5">
      {data.state === "not_set_up" && !dismissed && (
        <StatusCard
          variant="info"
          icon={<Rocket />}
          title="Set up the launch wiring"
          detail="Toolport can write a small shell file so that every claude launch first refreshes your layers and org file. Nothing changes until you confirm the plan."
          actions={
            <>
              <Button size="sm" onClick={() => setOpen("init")}>
                Preview the setup…
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setDismissed(true)}>
                Not now
              </Button>
            </>
          }
        >
          <p className="mt-2 text-meta text-muted-foreground">
            Nothing is written until the plan is confirmed. Until then, layers can still
            be authored and previewed.
          </p>
        </StatusCard>
      )}
      {data.state === "healthy" && (
        <StatusCard
          variant="success"
          icon={<Check />}
          title="Launch wiring is healthy"
          detail={
            <>
              <PresyncSummary status={data} /> The shell file is written and sourced last.
            </>
          }
          actions={
            <Button size="sm" variant="outline" onClick={onSync}>
              Sync now…
            </Button>
          }
        />
      )}
      {data.state === "look" && finding && (
        <StatusCard
          variant="warning"
          icon={<AlertTriangle />}
          title={finding.title}
          detail={finding.detail}
          actions={
            <>
              {order && (
                <Button size="sm" onClick={() => setOpen("fix")}>
                  Fix source order…
                </Button>
              )}
              {order && (
                <Button
                  size="sm"
                  variant="outline"
                  aria-expanded={showLines}
                  onClick={() => setShowLines(!showLines)}
                >
                  Show lines
                </Button>
              )}
              <Button size="sm" variant="outline" onClick={status.reload}>
                Run again
              </Button>
            </>
          }
        >
          {order && showLines && (
            <p className="mt-2 text-meta text-muted-foreground">
              Our line is line {data.shell.line} of{" "}
              <code className="font-mono">{shortPath(data.shell.file, home)}</code>;{" "}
              {data.shell.sourcedBefore.join(" and ")} is sourced after it.
            </p>
          )}
        </StatusCard>
      )}
      {failed && data.presync && (
        <StatusCard
          role="alert"
          variant="danger"
          icon={<AlertTriangle />}
          title="The last presync failed"
          detail={
            <>
              {data.presync.warnings[0] ?? "It did not finish."} Layers on disk are from
              the last good run
              {data.presync.lastOkAt ? ` (${whenText(data.presync.lastOkAt)})` : ""}. A
              failed presync only warns; Claude still starts.
            </>
          }
          actions={
            <>
              <Button size="sm" disabled={busy} onClick={() => void tryAgain()}>
                Try again
              </Button>
              <Button
                size="sm"
                variant="outline"
                aria-expanded={showOutput}
                onClick={() => setShowOutput(!showOutput)}
              >
                Show output
              </Button>
            </>
          }
        >
          {showOutput && (
            <pre className="mt-2 overflow-x-auto whitespace-pre-wrap rounded-md bg-muted p-2 font-mono text-meta">
              {data.presync.output || "The presync wrote no output."}
            </pre>
          )}
        </StatusCard>
      )}

      {checks.length > 0 && (
        <Group
          title="Checks"
          count={checks.length}
          action={
            <Button size="sm" variant="outline" onClick={status.reload}>
              <RefreshCw /> Run again
            </Button>
          }
        >
          {checks.map((check) => (
            <RowShell
              key={check.id}
              title={check.title}
              badge={
                <Badge variant={LEVEL[check.level].tone}>
                  {LEVEL[check.level].label}
                </Badge>
              }
              subtitle={check.detail || undefined}
            />
          ))}
        </Group>
      )}

      {data.setUp && (
        <Group title="Presync before every launch">
          <RowShell
            title="Refresh before claude starts"
            subtitle={
              <>
                Runs <code className="font-mono">claude()</code> → org tool pull when its
                interval passed → <code className="font-mono">context sync</code>.
                Throttled to once per {intervalText(data.intervalSecs)}; a failure only
                warns.
              </>
            }
            trailing={
              <>
                <Button size="sm" variant="outline" onClick={onSync}>
                  Sync now…
                </Button>
                <Button size="sm" variant="outline" onClick={() => setOpen("disable")}>
                  Turn off…
                </Button>
              </>
            }
          >
            <p className="text-meta text-muted-foreground">
              <PresyncLine status={data} />
            </p>
          </RowShell>
        </Group>
      )}

      {(open === "init" || open === "fix") && (
        <PlanDialog
          title={open === "fix" ? "Fix the source order" : "Set up the launch wiring"}
          description="The plan shows both files. The startup file is copied first and only one marked line of it changes."
          confirmLabel={open === "fix" ? "Move the line" : "Set up"}
          preview={() => initWiring(true)}
          confirm={async () => {
            await initWiring(false);
            offerUndo(
              open === "fix" ? "Moved the line" : "Launch wiring set up",
              () => disableWiring(false),
              reloadAll,
            );
          }}
          onClose={() => setOpen(null)}
          onDone={reloadAll}
        />
      )}
      {open === "disable" && (
        <PlanDialog
          title="Turn off the launch wiring"
          description="Removes the one marked line from the startup file, after a copy of it, and deletes the shell file. Your layers and the org file stay as they are."
          confirmLabel="Turn off"
          destructive
          preview={() => disableWiring(true)}
          confirm={async () => {
            await disableWiring(false);
            offerUndo("Launch wiring turned off", () => initWiring(false), reloadAll);
          }}
          onClose={() => setOpen(null)}
          onDone={reloadAll}
        />
      )}
    </div>
  );
}
