import { useId, type ReactNode } from "react";
import {
  AlertTriangle,
  BookOpen,
  CheckCircle2,
  Download,
  History,
  Pause,
  RefreshCw,
  Undo2,
  Upload,
  XCircle,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { SkeletonRows } from "@/components/ui/skeleton";
import { StatusCard } from "@/components/ui/status-card";
import type { AutoStatus, RemoteStatus, SyncData, VerifyData } from "./data";
import {
  autoLine,
  canPull,
  count,
  doneSummary,
  plural,
  pullBlockedReason,
  rowCount,
  remoteState,
  tickLabel,
  tilde,
  verifyLine,
  when,
  type Headline,
  type TickNeeds,
  needsTick,
} from "./model";
import type { SyncOutcome } from "./useLibrary";

export interface StatusAreaProps {
  outcome: SyncOutcome | null;
  headline: Headline | null;
  remote: RemoteStatus | null;
  auto: AutoStatus | null;
  autoHeld: SyncData | null;
  /** The run that just finished on this page. */
  done: SyncData | null;
  applying: number | null;
  failed: string | null;
  applyCount: number;
  needs: TickNeeds;
  ticked: boolean;
  canUndo: boolean;
  lockfileSyncedAt: string | null;
  /** The result of Check now: every library file against its installed copy. */
  verified: VerifyData | null;
  onTick: (value: boolean) => void;
  onApply: () => void;
  onPull: () => void;
  onPush: () => void;
  onFetch: () => void;
  onUndo: () => void;
  onRetry: () => void;
  onReload: () => void;
  onAuto: (action: "pause" | "resume") => void;
  onInit: () => void;
  onReview: () => void;
}

const CHMOD_FIX = "chmod -R u+w ~/.claude/skills";

function RemoteLine({
  remote,
  home,
  onFetch,
  onPush,
}: {
  remote: RemoteStatus;
  home: string | null;
  onFetch: () => void;
  onPush: () => void;
}) {
  const state = remoteState(remote);
  const reason = pullBlockedReason(state);
  return (
    <div className="flex flex-col gap-1.5 text-meta" data-testid="remote-line">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-muted-foreground">Library</span>
        <code className="font-mono text-xs break-all">{tilde(remote.repo, home)}</code>
        <Badge variant="outline">{remote.branch}</Badge>
        {state.kind === "current" && (
          <Badge variant="success">Up to date with origin</Badge>
        )}
        {"behind" in state && state.behind > 0 && (
          <Badge variant="warning">{state.behind} behind origin</Badge>
        )}
        {"ahead" in state && state.ahead > 0 && (
          <Badge variant="info" className="h-auto py-0.5 text-left whitespace-normal">
            {state.ahead} ahead of origin
          </Badge>
        )}
        {remote.uncommitted > 0 && (
          <Badge variant="info" className="h-auto whitespace-normal py-0.5 text-left">
            {plural(remote.uncommitted, "uncommitted change")}, included in this sync
          </Badge>
        )}
        {state.kind === "no-remote" && (
          <Badge variant="off">Local library, no remote</Badge>
        )}
        {state.kind === "offline" && (
          <Badge variant="warning">Could not reach origin</Badge>
        )}
        <span className="ml-auto flex gap-2">
          {state.kind !== "no-remote" && (
            <Button variant="outline" size="sm" onClick={onFetch}>
              <RefreshCw /> Fetch
            </Button>
          )}
          {state.kind !== "no-remote" && (
            <Button variant="outline" size="sm" onClick={onPush}>
              <Upload /> Push…
            </Button>
          )}
        </span>
      </div>
      {reason && state.kind !== "current" && state.kind !== "ahead" && (
        <p className="text-muted-foreground">{reason}</p>
      )}
    </div>
  );
}

function AutoRow({
  auto,
  onAuto,
}: {
  auto: AutoStatus | null;
  onAuto: (action: "pause" | "resume") => void;
}) {
  if (!auto) return null;
  const line = autoLine(auto);
  return (
    <div className="flex flex-wrap items-center gap-2 text-meta">
      <Badge variant={line.on ? "success" : "warning"}>
        {line.on ? "Auto-sync on" : "Auto-sync paused"}
      </Badge>
      {!line.on && auto.reason === "undo" && (
        <span className="text-muted-foreground">{line.text}</span>
      )}
      <Button
        variant={line.on ? "ghost" : "default"}
        size="sm"
        onClick={() => onAuto(line.on ? "pause" : "resume")}
      >
        {line.on ? (
          <>
            <Pause /> Pause
          </>
        ) : (
          "Resume auto-sync"
        )}
      </Button>
    </div>
  );
}

export default function StatusArea(props: StatusAreaProps) {
  const {
    outcome,
    headline,
    remote,
    auto,
    autoHeld,
    done,
    applying,
    failed,
    applyCount,
    needs,
    ticked,
    canUndo,
    lockfileSyncedAt,
  } = props;
  const tickId = useId();

  if (!outcome) {
    return (
      <div className="flex flex-col gap-3">
        <StatusCard
          variant="info"
          icon={<RefreshCw />}
          title="Checking your library…"
          detail="Reading your library, comparing the files with ~/.claude and linting. Usually under a second."
        />
        <SkeletonRows label="Checking your library" />
      </div>
    );
  }

  if (outcome.kind === "error") {
    if (outcome.code === "not_found") {
      return (
        <StatusCard
          variant="info"
          icon={<BookOpen />}
          title="No skills library yet"
          detail="A library is a folder of skills, rules and agents that Toolport copies into Claude Code for you. Skills Claude Code already has stay where they are and still show up in Skills. To use a repository you already have, clone it with git and Toolport finds it."
          actions={<Button onClick={props.onInit}>Start a local library</Button>}
        />
      );
    }
    if (outcome.code === "conflict") {
      return (
        <StatusCard
          variant="info"
          icon={<RefreshCw />}
          title="Another sync is running"
          detail={`${outcome.message}. The page refreshes when it finishes. Applying twice at once is not possible: one run holds the lock.`}
          actions={
            <>
              <Button disabled>Apply</Button>
              <Button variant="outline" size="sm" onClick={props.onRetry}>
                <RefreshCw /> Check again
              </Button>
            </>
          }
        />
      );
    }
    const denied = /permission denied/i.test(outcome.message);
    return (
      <StatusCard
        variant="danger"
        role="alert"
        icon={<XCircle />}
        title="Sync cannot run"
        detail={
          <>
            <p>{outcome.message}. Nothing was changed.</p>
            {denied && (
              <p className="mt-1">
                Typical cause: the folder is owned by another user.{" "}
                <code className="font-mono text-xs">{CHMOD_FIX}</code>
              </p>
            )}
          </>
        }
        actions={
          <>
            <Button variant="outline" size="sm" onClick={props.onRetry}>
              <RefreshCw /> Retry
            </Button>
            {denied && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => void navigator.clipboard?.writeText(CHMOD_FIX)}
              >
                Copy fix
              </Button>
            )}
          </>
        }
      />
    );
  }

  if (applying !== null) {
    return (
      <StatusCard
        variant="info"
        role="status"
        icon={<RefreshCw className="animate-spin" />}
        title={`Applying ${plural(applying, "change")}…`}
        detail="Takes a snapshot of the files that change, writes whole files, then checks every file against the library. It cannot be stopped once the snapshot is taken."
      />
    );
  }

  if (failed) {
    return (
      <StatusCard
        variant="danger"
        role="alert"
        icon={<XCircle />}
        title="The sync did not finish"
        detail={`${failed} Retry runs the same plan again. Undo covers the steps that did run.`}
        actions={
          <>
            <Button size="sm" onClick={props.onApply}>
              <RefreshCw /> Retry
            </Button>
            {canUndo && (
              <Button variant="outline" size="sm" onClick={props.onUndo}>
                <Undo2 /> Undo
              </Button>
            )}
          </>
        }
      />
    );
  }

  if (done) {
    const summary = doneSummary(done);
    const written = rowCount(done.plan.changes);
    return (
      <div className="flex flex-col gap-3">
        <StatusCard
          variant={summary.verified ? "success" : "warning"}
          role="status"
          icon={summary.verified ? <CheckCircle2 /> : <AlertTriangle />}
          title={
            summary.verified
              ? `Synced ${plural(written, "change")}. Checked after writing: ${summary.line}`
              : `Synced ${plural(written, "change")}. ${plural(done.verify?.mismatched.length ?? 0, "file")} did not match their library copy`
          }
          detail={
            summary.verified ? (
              <>
                Compared each file in ~/.claude with your library (the same check as{" "}
                <code className="font-mono text-xs">toolportctl skills verify</code>). New
                sessions pick the change up; sessions that are open keep what they loaded.
              </>
            ) : (
              <>
                The files were written but differ afterwards, which usually means another
                program changed them in between: {summary.mismatched.join(", ")}.
              </>
            )
          }
          actions={
            summary.verified ? (
              canUndo && (
                <Button variant="outline" size="sm" onClick={props.onUndo}>
                  <Undo2 /> Undo
                </Button>
              )
            ) : (
              <Button variant="outline" size="sm" onClick={props.onApply}>
                <RefreshCw /> Retry those {done.verify?.mismatched.length}
              </Button>
            )
          }
        />
      </div>
    );
  }

  if (!headline) return null;
  const hasPlan = headline.kind !== "in-sync";
  const state = remote ? remoteState(remote) : null;
  const pullFirst = state !== null && canPull(state) && headline.kind === "behind";
  const Icon =
    headline.tone === "success"
      ? CheckCircle2
      : headline.kind === "undone"
        ? History
        : AlertTriangle;
  const variant = headline.tone === "success" ? "success" : headline.tone;

  return (
    <div className="flex flex-col gap-3">
      <StatusCard
        variant={variant}
        icon={<Icon />}
        testId="status-card"
        title={headline.title}
        detail={
          <>
            <p>{headline.detail}</p>
            {headline.kind === "in-sync" && (
              <p className="mt-1">
                {lockfileSyncedAt ? `Last synced ${when(lockfileSyncedAt)}. ` : ""}
                Lint, audit and drift are counted below.
              </p>
            )}
            {headline.kind === "in-sync" && props.verified && (
              <p className="mt-1" role="status">
                {props.verified.clean
                  ? `Checked now: ${verifyLine(props.verified)}.`
                  : `Checked now: ${plural(props.verified.mismatched.length, "file")} differ from the library copy: ${props.verified.mismatched
                      .map((m) => `${m.skill}/${m.path}`)
                      .join(", ")}.`}
              </p>
            )}
          </>
        }
        actions={
          <>
            {pullFirst && (
              <Button size="sm" onClick={props.onPull}>
                <Download /> Pull and review plan
              </Button>
            )}
            {hasPlan && applyCount > 0 && (
              <Button
                size="sm"
                variant={pullFirst ? "outline" : "default"}
                onClick={props.onApply}
              >
                <CheckCircle2 /> Apply {plural(applyCount, "change")}
              </Button>
            )}
            {headline.kind === "in-sync" && (
              <>
                {canUndo && (
                  <Button variant="ghost" size="sm" onClick={props.onUndo}>
                    <Undo2 /> Undo last sync
                  </Button>
                )}
                <Button variant="outline" size="sm" onClick={props.onReload}>
                  <RefreshCw /> Check now
                </Button>
              </>
            )}
          </>
        }
      >
        <div className="flex flex-col gap-2">
          {needsTick(needs) && (
            <label htmlFor={tickId} className="flex items-start gap-2 text-meta">
              <input
                id={tickId}
                type="checkbox"
                className="mt-0.5"
                checked={ticked}
                onChange={(event) => props.onTick(event.target.checked)}
              />
              <span>
                {tickLabel(needs)}. Without the tick the other changes are applied and
                these wait.
              </span>
            </label>
          )}
          {remote && (
            <RemoteLine
              remote={remote}
              home={outcome.kind === "ok" ? outcome.data.outputRoot : null}
              onFetch={props.onFetch}
              onPush={props.onPush}
            />
          )}
          <AutoRow auto={auto} onAuto={props.onAuto} />
        </div>
      </StatusCard>
      {autoHeld && <AutoHeldCard held={autoHeld} onReview={props.onReview} />}
    </div>
  );
}

function AutoHeldCard({ held, onReview }: { held: SyncData; onReview: () => void }) {
  const removals = held.held.filter((h) => h.op === "remove").length;
  const hooks = held.held.filter((h) => h.op === "hook").length;
  const others = held.held.length - removals - hooks;
  const parts: ReactNode[] = [];
  if (removals) parts.push(`remove ${plural(removals, "file")}`);
  if (hooks) parts.push(`add ${plural(hooks, "hook")}`);
  if (others) parts.push(`replace ${plural(others, "file")}`);
  return (
    <StatusCard
      variant="warning"
      icon={<Pause />}
      title="The next auto-sync would stop before writing"
      detail={
        <>
          It would {parts.join(" and ")}, and auto-sync does not do those on its own.
          Everything else it would write is in the plan below. Held items:{" "}
          {count(held.held.length)}.
        </>
      }
      actions={
        <Button variant="outline" size="sm" onClick={onReview}>
          Review plan
        </Button>
      }
    />
  );
}
