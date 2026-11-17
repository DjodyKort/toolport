import { useRef, useState } from "react";
import { Copy, ExternalLink, Loader2, RefreshCw, ShieldCheck } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { openExternal } from "@/lib/openUrl";
import { CtlError } from "../bridge/ctl";
import type { AuthRow } from "../bridge/data";
import { errorText } from "../settings/useCtl";
import { checkLogin, signIn, type SignIn } from "./commands";
import { clock, since, span } from "./format";
import { fixIsSignIn, LOGIN_VIEWS, loginFailed, loginReason } from "./loginModel";
import { OperationRow } from "./OperationRow";

type Phase = "idle" | "checking" | "signing";

export interface LoginRowProps {
  serverId: string;
  serverName: string;
  row: AuthRow;
  /** Runs after a check or a sign-in, so the page reads the new state. */
  onChanged: () => void;
}

function sub(row: AuthRow): string {
  const view = LOGIN_VIEWS[row.state];
  const checked =
    row.lastProbe === null ? "not checked yet" : `checked ${clock(row.lastProbe * 1000)}`;
  if (row.state === "ok") return `Signed in · ${checked}`;
  if (row.state === "unknown") return "Not checked yet";
  return `${loginReason(row) ?? view.label} · ${checked}`;
}

function Facts({ row, nextDueAt }: { row: AuthRow; nextDueAt: number | null }) {
  const view = LOGIN_VIEWS[row.state];
  const reason = loginReason(row);
  return (
    <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-meta">
      <dt className="text-muted-foreground">Last check</dt>
      <dd>
        {row.lastProbe === null
          ? "Not checked yet"
          : `${since(row.lastProbe * 1000)} · ${view.label}`}
      </dd>
      {row.state !== "ok" && row.state !== "unknown" && (
        <>
          <dt className="text-muted-foreground">Since</dt>
          <dd>{since(row.since * 1000)}</dd>
        </>
      )}
      {reason && (
        <>
          <dt className="text-muted-foreground">Reason</dt>
          <dd className="break-words">{reason}</dd>
        </>
      )}
      {row.state === "expiring" && row.ttlSecs !== null && (
        <>
          <dt className="text-muted-foreground">Expires</dt>
          <dd>in {span(Math.max(0, row.ttlSecs))}</dd>
        </>
      )}
      {nextDueAt !== null && (
        <>
          <dt className="text-muted-foreground">Next check</dt>
          <dd>{since(nextDueAt * 1000)}</dd>
        </>
      )}
    </dl>
  );
}

function ConsentLink({ url, onCancel }: { url: string | null; onCancel: () => void }) {
  async function copy() {
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
      toast.success("Link copied");
    } catch {
      toast.error("Could not copy the link");
    }
  }
  return (
    <div
      role="group"
      aria-label="Signing in"
      className="flex flex-col gap-2 rounded-lg border border-border bg-muted/40 p-3"
    >
      <p className="flex items-center gap-2 text-sm font-semibold">
        <Loader2 aria-hidden="true" className="size-4 animate-spin" />
        Finish the sign-in in your browser
      </p>
      {url ? (
        <>
          <p className="text-meta text-muted-foreground">
            If the page did not open by itself, use this link. It holds no password.
          </p>
          <code className="rounded-md bg-muted px-2 py-1.5 font-mono text-meta break-all">
            {url}
          </code>
        </>
      ) : (
        <p className="text-meta text-muted-foreground">
          Waiting for the server to print its sign-in link…
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        {url && (
          <>
            <Button variant="outline" size="sm" onClick={() => void openExternal(url)}>
              <ExternalLink /> Open link
            </Button>
            <Button variant="outline" size="sm" onClick={() => void copy()}>
              <Copy /> Copy link
            </Button>
          </>
        )}
        <Button variant="ghost" size="sm" onClick={onCancel}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

/** The login of a stdio or Google server, which upstream cannot see: the state the last check
 * found, a check on demand and the server's own sign-in. */
export function LoginRow({ serverId, serverName, row, onChanged }: LoginRowProps) {
  const view = LOGIN_VIEWS[row.state];
  const [phase, setPhase] = useState<Phase>("idle");
  const [link, setLink] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [nextDueAt, setNextDueAt] = useState<number | null>(null);
  const [userOpen, setUserOpen] = useState<boolean | null>(null);
  const running = useRef<SignIn | null>(null);

  const failed = loginFailed(row);
  const open = userOpen ?? (failed || phase === "signing" || problem !== null);
  const signInFirst = fixIsSignIn(row);

  async function check() {
    setPhase("checking");
    setProblem(null);
    try {
      const data = await checkLogin(serverId);
      const report = data.probes[0];
      if (report) setNextDueAt(report.nextDueAt);
      if (data.failures[0]) {
        setProblem(`The check could not run: ${data.failures[0].error}`);
      }
      onChanged();
    } catch (error) {
      setProblem(errorText(error));
    } finally {
      setPhase("idle");
    }
  }

  function startSignIn() {
    setProblem(null);
    setLink(null);
    const run = signIn(serverId, setLink);
    running.current = run;
    setPhase("signing");
    run.done
      .then((data) => {
        toast.success(data.message);
        onChanged();
      })
      .catch((error: unknown) => {
        if (!(error instanceof CtlError && error.code === "cancelled")) {
          setProblem(errorText(error));
        }
      })
      .finally(() => {
        running.current = null;
        setLink(null);
        setPhase("idle");
      });
  }

  const signInButton = (
    <Button
      variant={signInFirst ? "secondary" : "outline"}
      disabled={phase !== "idle"}
      onClick={startSignIn}
    >
      Sign in again
    </Button>
  );

  return (
    <OperationRow
      icon={<ShieldCheck />}
      title="Login"
      sub={<span aria-live="polite">{problem ?? sub(row)}</span>}
      tone={problem ? "danger" : view.tone}
      badge={<Badge variant={view.variant}>{view.label}</Badge>}
      actions={
        <>
          {signInFirst && signInButton}
          <Button
            variant="outline"
            disabled={phase !== "idle"}
            onClick={() => void check()}
          >
            {phase === "checking" ? <Loader2 className="animate-spin" /> : <RefreshCw />}
            {phase === "checking" ? "Checking" : "Check now"}
          </Button>
        </>
      }
      open={open}
      onToggle={() => setUserOpen(!open)}
    >
      {phase === "signing" && (
        <ConsentLink url={link} onCancel={() => void running.current?.cancel()} />
      )}
      <Facts row={row} nextDueAt={nextDueAt} />
      {row.fix && row.fix.command === null && (
        <p className="text-meta text-muted-foreground">{row.fix.label}</p>
      )}
      {failed && (
        <p className="text-meta text-muted-foreground">
          {serverName} still starts, but calls that need this login fail until it is
          fixed.
        </p>
      )}
      {!signInFirst && <div className="flex flex-wrap gap-2">{signInButton}</div>}
    </OperationRow>
  );
}
