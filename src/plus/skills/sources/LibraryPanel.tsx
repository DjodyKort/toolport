import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowDownToLine, ArrowUpFromLine, RefreshCw, WifiOff } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { runCtl } from "../../bridge/ctl";
import type { LibraryStatusData } from "../../types/library";
import { ErrorState } from "../../ui";
import { failure, useRead, type WriteControl } from "../hooks";
import { plural } from "../model";
import { PathLine } from "../parts";
import {
  authText,
  behindText,
  changesText,
  PULL,
  PUSH,
  reasonOff,
  remoteText,
} from "./library";
import { plain, timeText } from "./model";
import { useOnline } from "./online";
import { Row } from "./Row";

interface Fetched {
  phase: "idle" | "running" | "done";
  data: LibraryStatusData | null;
  error: unknown;
}

/** `library status --fetch` contacts the remote, so it runs only when the button is pressed. */
function useRemoteCheck() {
  const [state, setState] = useState<Fetched>({ phase: "idle", data: null, error: null });
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const check = useCallback(() => {
    setState((before) => ({ ...before, phase: "running", error: null }));
    const settle = (next: Fetched) => alive.current && setState(next);
    runCtl<LibraryStatusData>(["library", "status", "--fetch"]).result.then(
      (result) => {
        const data = result.envelope?.data ?? null;
        settle({
          phase: "done",
          data,
          error: data === null || result.envelope?.ok === false ? failure(result) : null,
        });
      },
      (error) => settle({ phase: "done", data: null, error }),
    );
  }, []);
  return { ...state, check };
}

function Clones({ clones }: { clones: LibraryStatusData["duplicateClones"] }) {
  return (
    <Row label="Other clones">
      <ul aria-label="Other clones" className="flex flex-col gap-2">
        {clones.map((clone) => (
          <li key={clone.path} className="flex flex-col gap-0.5">
            <PathLine path={plain(clone.path)} />
            <span className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
              <Badge variant={clone.sameRemote ? "warning" : "secondary"}>
                {clone.sameRemote ? "same remote" : "another remote"}
              </Badge>
              {plural(clone.skills, "skill")} · {plural(clone.behind, "commit")} behind,{" "}
              {plural(clone.ahead, "commit")} ahead
            </span>
          </li>
        ))}
        <li className="text-xs text-muted-foreground">
          Two clones of one remote drift apart. Toolport never changes or deletes the
          other one; keep the one you work in.
        </li>
      </ul>
    </Row>
  );
}

function Rows({ status }: { status: LibraryStatusData }) {
  return (
    <>
      <Row label="Remote">
        <span className="flex flex-col gap-0.5">
          {status.remote === null ? (
            <span className="text-muted-foreground">No remote</span>
          ) : (
            <code className="font-mono text-xs break-all">{remoteText(status)}</code>
          )}
          <span className="text-xs text-muted-foreground">
            Branch {status.branch ?? "unknown"}
            {status.upstream ? `, follows ${status.upstream}` : ", follows nothing"}
          </span>
        </span>
      </Row>
      {status.remote !== null && (
        <Row label="Against remote">
          <span className="flex flex-col gap-0.5">
            <span>{behindText(status)}</span>
            <span className="text-xs text-muted-foreground">
              Last fetch{" "}
              {status.lastFetch ? (
                <time dateTime={status.lastFetch}>{timeText(status.lastFetch)}</time>
              ) : (
                "never"
              )}
              ; the counts are from then.
            </span>
          </span>
        </Row>
      )}
      <Row label="Changes here">{changesText(status)}</Row>
      {status.remote !== null && <Row label="Sign-in">{authText(status.auth)}</Row>}
      {status.duplicateClones.length > 0 && <Clones clones={status.duplicateClones} />}
    </>
  );
}

function Notices({ status }: { status: LibraryStatusData }) {
  return (
    <>
      {status.remote === null && (
        <Callout variant="info" role="status">
          This library has no remote, so there is nothing to pull from or push to. Add one
          in its folder with git and Toolport shows it here.
        </Callout>
      )}
      {status.uncommitted > 0 && (
        <Callout variant="warning" role="status">
          {plural(status.uncommitted, "file")} here{" "}
          {status.uncommitted === 1 ? "is" : "are"} not committed. Pull refuses to run
          until they are committed or stashed; Push commits them as “Update skills
          library” and then pushes.
        </Callout>
      )}
      {status.fetch.requested && status.fetch.ok === false && (
        <Callout variant="warning" role="status">
          The remote could not be reached
          {status.fetch.error ? `: ${plain(status.fetch.error)}` : ""}. The counts above
          are from the last fetch.
        </Callout>
      )}
      {status.fetch.requested && status.fetch.ok === true && !status.auth.ok && (
        <Callout variant="warning" role="status">
          The remote answered but did not accept {authText(status.auth)}. Sign in with git
          in a terminal and check again.
        </Callout>
      )}
    </>
  );
}

/** What the library clone says about itself: its remote, how far apart they are, what is not
 * committed, and the two writes that follow from it. Reading is local; only "Check the remote"
 * reaches the network, and only when pressed. */
export function LibraryPanel({ write }: { write: WriteControl }) {
  const query = useRead<LibraryStatusData>(["library", "status"]);
  const remote = useRemoteCheck();
  const online = useOnline();
  const status = remote.data ?? query.data;
  const off = reasonOff(status, query.status === "loading");
  const busy = write.busy || remote.phase === "running";
  const trigger = useRef<HTMLButtonElement | null>(null);
  const open = useRef(false);
  useEffect(() => {
    if (write.spec) open.current = true;
    else if (open.current) {
      open.current = false;
      trigger.current?.focus();
    }
  }, [write.spec]);
  const begin = (spec: typeof PULL, button: HTMLButtonElement) => {
    trigger.current = button;
    write.begin(spec);
  };

  return (
    <div role="group" aria-label="Library remote" className="flex flex-col gap-3">
      {query.status === "error" && status === null && (
        <ErrorState
          error={query.error}
          title="Couldn't read the library status"
          context="library status"
          onRetry={query.reload}
        />
      )}
      {status === null && query.status === "loading" && (
        <p role="status" className="text-sm text-muted-foreground">
          Reading the library status…
        </p>
      )}
      {status !== null && (
        <dl className="grid grid-cols-[8rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
          <Rows status={status} />
        </dl>
      )}
      {status !== null && <Notices status={status} />}
      {remote.error !== null && (
        <ErrorState
          error={remote.error}
          title="Couldn't check the remote"
          context="library status --fetch"
          onRetry={remote.check}
        />
      )}
      {!online && (
        <Callout variant="info" role="status">
          <span className="flex items-center gap-2">
            <WifiOff className="size-4 shrink-0" aria-hidden="true" />
            You are offline. Check the remote, Pull and Push need the network; the numbers
            above are from the last fetch.
          </span>
        </Callout>
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={busy || status?.remote === null || status === null}
          onClick={remote.check}
          title="Contacts the remote and updates the counts. Changes no file."
        >
          <RefreshCw
            className={remote.phase === "running" ? "animate-spin" : undefined}
          />{" "}
          {remote.phase === "running" ? "Checking the remote…" : "Check the remote"}
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={busy || off !== null}
          title={off ?? "Fetches the remote and shows the commits it would bring"}
          aria-describedby={off ? "library-reason" : undefined}
          onClick={(event) => begin(PULL, event.currentTarget)}
        >
          <ArrowDownToLine /> Pull
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={busy || off !== null}
          title={off ?? "Checks for secrets, then shows the commits it would send"}
          aria-describedby={off ? "library-reason" : undefined}
          onClick={(event) => begin(PUSH, event.currentTarget)}
        >
          <ArrowUpFromLine /> Push…
        </Button>
      </div>
      {off && (
        <p id="library-reason" className="text-xs text-muted-foreground">
          {off}
        </p>
      )}
      <p className="text-xs text-muted-foreground">
        Pull only fast-forwards and never touches files you changed. Push checks for
        secrets first, never forces, and shows the commits before anything is sent.
      </p>
    </div>
  );
}
