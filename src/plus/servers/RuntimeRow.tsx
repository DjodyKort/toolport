import { useEffect, useState } from "react";
import { Check, Clock, Info, Loader2, Power, RotateCw, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { shortenUrls } from "@/lib/errors";
import type { ProbeResult, ServerEntry } from "@/lib/types";
import { plural } from "../settings/model";
import { clock, MAX_WAIT_SECONDS } from "./format";
import type { Loaded } from "./useLoad";
import { OperationRow } from "./OperationRow";
import type { RestartGateway, ServerInfoData } from "./data";
import {
  cancel,
  restart,
  restartNow,
  summary,
  useRestartState,
  waitThenRestart,
} from "./restartStore";

const SHELL_ENV = {
  on: "your whole shell environment, plus its own entries",
  off: "only a fixed set (PATH, HOME, locale, certificates), plus its own entries",
} as const;

function useElapsed(since: number | null): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (since === null) return;
    const timer = setInterval(() => setNow(Date.now()), 1_000);
    return () => clearInterval(timer);
  }, [since]);
  return since === null ? 0 : Math.max(0, Math.floor((now - since) / 1_000));
}

function healthBadge(enabled: boolean, health?: ProbeResult) {
  if (!enabled) return <Badge variant="off">Off</Badge>;
  if (!health) return <Badge variant="off">Checking</Badge>;
  if (health.ok) {
    return (
      <Badge variant="success">
        Ready{health.toolCount > 0 ? ` · ${plural(health.toolCount, "tool")}` : ""}
      </Badge>
    );
  }
  if (health.authRequired) return <Badge variant="warning">Needs sign-in</Badge>;
  return <Badge variant="destructive">Failed to start</Badge>;
}

export function GatewayAnswers({
  gateways,
  waiting,
}: {
  gateways: RestartGateway[];
  waiting?: boolean;
}) {
  return (
    <ul className="flex flex-col gap-1.5 text-meta" aria-label="Gateway answers">
      {gateways.map((gateway) => (
        <li key={gateway.pid} className="flex items-start gap-2">
          {gateway.restarted ? (
            <Check aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-success" />
          ) : gateway.inFlight !== undefined ? (
            <Clock
              aria-hidden="true"
              className="mt-0.5 size-4 shrink-0 text-muted-foreground"
            />
          ) : (
            <X aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-destructive" />
          )}
          <span className="min-w-0 break-words">
            <span className="font-medium">
              {gateway.root ? gateway.root : `Gateway ${gateway.pid}`}
            </span>{" "}
            <span className="text-muted-foreground">
              {gateway.root ? `(gateway ${gateway.pid}) ` : ""}
              {gateway.restarted ? "restarted" : shortenUrls(gateway.detail)}
            </span>
          </span>
        </li>
      ))}
      {waiting && (
        <li className="flex items-center gap-2 text-muted-foreground">
          <Loader2 aria-hidden="true" className="size-4 animate-spin" />
          Waiting for the answers
        </li>
      )}
    </ul>
  );
}

function BusyCard({
  id,
  name,
  gateways,
}: {
  id: string;
  name: string;
  gateways: RestartGateway[];
}) {
  const calls = gateways.reduce((sum, g) => sum + (g.inFlight ?? 0), 0);
  return (
    <div
      role="group"
      aria-label="Calls in flight"
      className="flex flex-col gap-2 rounded-lg border border-warning/40 bg-warning/8 p-3"
    >
      <p className="text-sm font-semibold">
        {calls === 1 ? "1 call is still running" : `${calls} calls are still running`}
      </p>
      <GatewayAnswers gateways={gateways.filter((g) => (g.inFlight ?? 0) > 0)} />
      <p className="text-meta text-muted-foreground">
        Restarting cancels {calls === 1 ? "it" : "them"} and the client sees an error.
      </p>
      <div className="flex flex-wrap gap-2">
        <Button variant="destructive" size="sm" onClick={() => void restartNow(id, name)}>
          Restart now
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void waitThenRestart(id, name)}
        >
          Wait, then restart
        </Button>
        <Button variant="ghost" size="sm" onClick={() => cancel(id)}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

function ProcessFacts({ info }: { info: Loaded<ServerInfoData> }) {
  if (!info.data) {
    return (
      <p className="text-meta text-muted-foreground">
        {info.error
          ? `Could not read its settings: ${info.error}`
          : "Reading its settings…"}
      </p>
    );
  }
  const data = info.data;
  const local = data.transport === "stdio";
  const target = local
    ? [data.command, ...data.args].filter(Boolean).join(" ")
    : data.url;
  return (
    <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1 text-meta">
      <dt className="text-muted-foreground">{local ? "Command" : "Address"}</dt>
      <dd className="font-mono break-all">{target}</dd>
      {data.cwd && (
        <>
          <dt className="text-muted-foreground">Folder</dt>
          <dd className="font-mono break-all">{data.cwd}</dd>
        </>
      )}
      <dt className="text-muted-foreground">It sees</dt>
      <dd>
        {local
          ? SHELL_ENV[data.inheritEnv ? "on" : "off"]
          : "Not a local process; Toolport sends no environment"}
        {local && !data.inheritEnv && (
          <span className="block text-muted-foreground">
            Variables from your shell are missing on purpose. Turn on “Use my shell
            environment” in Edit if it needs them.
          </span>
        )}
      </dd>
      {data.env.length > 0 && (
        <>
          <dt className="text-muted-foreground">Own entries</dt>
          <dd className="flex flex-wrap gap-1.5">
            {data.env.map((entry) => (
              <Badge key={entry.key} variant={entry.secret ? "warning" : "off"}>
                <span className="font-mono">{entry.key}</span>
                {entry.secret ? " · secret" : " · plain"}
              </Badge>
            ))}
          </dd>
        </>
      )}
    </dl>
  );
}

export interface RuntimeRowProps {
  server: ServerEntry;
  enabled: boolean;
  health?: ProbeResult;
  info: Loaded<ServerInfoData>;
  /** When the connection settings last changed in this window. */
  changedAt?: number;
}

/** What is running and how to restart it: one click when idle, a confirmation naming the calls in
 * flight otherwise, and the answer of every gateway afterwards. */
export function RuntimeRow({
  server,
  enabled,
  health,
  info,
  changedAt,
}: RuntimeRowProps) {
  const state = useRestartState(server.id);
  const { phase, last, error } = state;
  const [userOpen, setUserOpen] = useState<boolean | null>(null);
  const asking = phase.kind === "asking";
  const elapsed = useElapsed(asking ? phase.since : null);
  const failed = last?.status === "failed";
  const idleAnswer = last?.status === "not_running";
  const open =
    userOpen ?? (phase.kind !== "idle" || failed || idleAnswer || error !== null);
  const stale = changedAt !== undefined && (!last || changedAt > last.at);

  let sub: string;
  if (!enabled) sub = "Switched off · nothing is running";
  else if (asking)
    sub = `Asking every running gateway… ${elapsed} s of up to ${MAX_WAIT_SECONDS} s`;
  else if (phase.kind === "busy") sub = "A call is running; restarting would end it";
  else if (phase.kind === "waiting")
    sub = "Waiting for the running call to finish, then restarting";
  else if (error) sub = `Could not ask the gateways: ${error}`;
  else if (stale)
    sub = `Settings changed ${clock(changedAt)} · a new process starts on the next call`;
  else if (last) sub = `${summary(server.name, last)} · ${clock(last.at)}`;
  else sub = "A restart starts a fresh process in every gateway that runs it";

  const busyGateways =
    phase.kind === "busy" || phase.kind === "waiting" ? phase.gateways : [];
  const badge =
    asking || phase.kind === "waiting" ? (
      <Badge variant="info">Restarting</Badge>
    ) : phase.kind === "busy" ? (
      <Badge variant="warning">Call running</Badge>
    ) : failed ? (
      <Badge variant="warning">
        {last.gateways.filter((g) => !g.restarted).length} failed
      </Badge>
    ) : last?.status === "not_running" ? (
      <Badge variant="off">Idle</Badge>
    ) : (
      healthBadge(enabled, health)
    );

  const action = !enabled ? null : asking ? (
    <Button variant="outline" disabled>
      <Loader2 className="animate-spin" /> Restarting
    </Button>
  ) : phase.kind === "waiting" ? (
    <Button variant="outline" onClick={() => cancel(server.id)}>
      Cancel
    </Button>
  ) : phase.kind === "busy" ? null : (
    <Button
      variant={failed ? "secondary" : "outline"}
      onClick={() => void restart(server.id, server.name)}
    >
      <RotateCw /> {failed ? "Retry" : "Restart"}
    </Button>
  );

  return (
    <OperationRow
      icon={<Power />}
      title="Runtime"
      sub={<span aria-live="polite">{sub}</span>}
      tone={failed || phase.kind === "busy" ? "warning" : error ? "danger" : undefined}
      badge={badge}
      actions={action}
      open={open}
      onToggle={() => setUserOpen(!open)}
    >
      {phase.kind === "busy" && (
        <BusyCard id={server.id} name={server.name} gateways={busyGateways} />
      )}
      {phase.kind === "waiting" && (
        <GatewayAnswers
          gateways={busyGateways.filter((g) => (g.inFlight ?? 0) > 0)}
          waiting
        />
      )}
      {asking && (
        <p className="flex items-center gap-2 text-meta text-muted-foreground">
          <Loader2 aria-hidden="true" className="size-4 animate-spin" />
          Every running gateway answers on its own; the answers appear here together.
        </p>
      )}
      {phase.kind === "idle" && last && last.gateways.length > 0 && (
        <GatewayAnswers gateways={last.gateways} />
      )}
      {phase.kind === "idle" && failed && (
        <p className="text-meta text-muted-foreground">
          Retry asks every gateway again, so one that already restarted restarts once
          more.
        </p>
      )}
      {phase.kind === "idle" && last?.status === "not_running" && (
        <p className="flex items-start gap-2 text-meta text-muted-foreground">
          <Info aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
          No client is connected, or none has used this server in the last 5 minutes. The
          next client that calls it starts the current files, so the restart you wanted
          has already happened.
        </p>
      )}
      <ProcessFacts info={info} />
    </OperationRow>
  );
}
