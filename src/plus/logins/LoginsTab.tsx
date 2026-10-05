import { lazy, Suspense, useCallback, useState } from "react";
import { KeyRound, RefreshCw, Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { EmptyState } from "@/components/ui/empty-state";
import { Switch } from "@/components/ui/switch";
import { useCtlJob } from "../ui";
import { useRefreshTasks } from "../tasks/refreshTasks";
import { Gate, StateBadge, WhenText } from "./atoms";
import {
  countableLogins,
  expiresText,
  loginArgv,
  needsSignIn,
  summarize,
  type LoginRow,
} from "./model";
import { ProbePanel } from "./ProbePanel";
import { SignInDialog } from "./SignInDialog";
import { SetSecretDialog } from "./SetSecretDialog";
import { Strip } from "./Strip";
import { useProbe } from "./useProbe";
import { POLL_MS, useRoster } from "./useRoster";

const RunTaskDialog = lazy(() =>
  import("../tasks/RunDialog").then((m) => ({ default: m.RunTaskDialog })),
);

export interface TabProps {
  /** Opens the All commands page on a command group. */
  onOpenCommands?: (group?: string) => void;
  /** How often the login state is read again; 0 reads it only on demand. */
  pollMs?: number;
}

function Detail({ row }: { row: LoginRow }) {
  const auth = row.auth;
  if (!auth) return null;
  const bits: string[] = [];
  if (row.state !== "ok" && row.state !== "unknown" && auth.reason)
    bits.push(auth.reason);
  const expires = row.state === "expiring" ? expiresText(auth) : null;
  if (expires) bits.push(expires);
  const hint = auth.fix && auth.fix.action === "fix_config" ? auth.fix.label : null;
  if (bits.length === 0 && !hint) return null;
  return (
    <div className="text-xs text-muted-foreground">
      {bits.join(" · ")}
      {hint && <span className="block">{hint}</span>}
    </div>
  );
}

function Actions({
  row,
  busy,
  probing,
  onSignIn,
  onSetSecret,
  onProbe,
  onRefreshTask,
}: {
  row: LoginRow;
  busy: boolean;
  probing: boolean;
  onSignIn: () => void;
  onSetSecret: () => void;
  onProbe: () => void;
  onRefreshTask?: () => void;
}) {
  const needs = needsSignIn(row.state);
  return (
    <div className="flex flex-wrap justify-end gap-1.5">
      {onRefreshTask && (
        <Button
          size="sm"
          variant="outline"
          aria-label={`Refresh task for ${row.name}`}
          onClick={onRefreshTask}
        >
          <Play /> Refresh task…
        </Button>
      )}
      {row.canSetSecret && (
        <Button
          size="sm"
          variant={row.kind === "token" && row.state !== "ok" ? "default" : "outline"}
          disabled={busy}
          aria-label={`Set secret for ${row.name}`}
          onClick={onSetSecret}
        >
          Set secret
        </Button>
      )}
      {row.canSignIn && (
        <Button
          size="sm"
          variant={needs ? "default" : "outline"}
          disabled={busy}
          aria-label={needs ? `Sign in to ${row.name}` : `Sign in again to ${row.name}`}
          onClick={onSignIn}
        >
          {needs ? "Sign in" : "Sign in again"}
        </Button>
      )}
      <Button
        size="sm"
        variant="outline"
        disabled={busy}
        aria-label={`Probe ${row.name}`}
        onClick={onProbe}
      >
        {probing ? "Probing…" : "Probe"}
      </Button>
    </div>
  );
}

/** Logins: every server that has a login, the broken ones first, with a state, a reason, the
 * time of the last probe and the one action that fixes it: Sign in for an OAuth server, Set
 * secret for an API-token server. Probe re-checks one or all of them. */
export function LoginsTab({ onOpenCommands, pollMs = POLL_MS }: TabProps) {
  const data = useRoster(pollMs);
  const { reload } = data;
  const [force, setForce] = useState(true);
  const probe = useProbe(reload);
  const signIn = useCtlJob();
  const [signing, setSigning] = useState<{ row: LoginRow; noOpen: boolean } | null>(null);
  const [secretFor, setSecretFor] = useState<LoginRow | null>(null);
  const refreshTaskOf = useRefreshTasks();
  const [refreshing, setRefreshing] = useState<{ id: string; title: string } | null>(
    null,
  );

  const { start: startLogin, reset: resetLogin } = signIn;
  const startSignIn = useCallback(
    (row: LoginRow, noOpen: boolean) => {
      setSigning({ row, noOpen });
      void startLogin(loginArgv(row.id, noOpen)).then((result) => {
        if (result?.envelope?.ok) reload();
      });
    },
    [startLogin, reload],
  );
  const closeSignIn = () => {
    resetLogin();
    setSigning(null);
  };

  const busy = probe.job.state.phase === "running" || signIn.state.phase === "running";
  const status = data.status.data;
  const gatewayMissing = status !== null && !status.gateway.present;

  return (
    <div className="flex flex-col gap-4">
      <Gate
        queries={[data.status, data.servers]}
        pending={data.roster === null}
        title="Couldn't read the logins"
        context="status, server ls"
        onOpenCommands={onOpenCommands}
      >
        {() => {
          const roster = data.roster!;
          const rows = roster.rows;
          return (
            <>
              <Strip summary={summarize(countableLogins(rows))} />
              <div className="flex flex-wrap items-center gap-3">
                <Button
                  disabled={busy || rows.length === 0}
                  onClick={() => void probe.run({ force })}
                >
                  <RefreshCw /> Probe all
                </Button>
                <label className="flex items-center gap-2 text-sm">
                  <Switch size="sm" checked={force} onCheckedChange={setForce} />
                  Re-check even if a result is cached
                </label>
                <Button
                  variant="ghost"
                  size="sm"
                  className="ml-auto"
                  onClick={reload}
                  aria-label="Refresh the list"
                >
                  <RefreshCw /> Refresh
                </Button>
              </div>
              <ProbePanel probe={probe} />
              {gatewayMissing && (
                <Callout
                  variant="warning"
                  role="status"
                  className="flex flex-wrap items-center gap-2"
                >
                  <span>
                    The gateway is not installed next to the app, so servers cannot start.
                    Probes still run.
                  </span>
                  {onOpenCommands && (
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => onOpenCommands("doctor")}
                    >
                      Open doctor
                    </Button>
                  )}
                </Callout>
              )}
              {data.failedInfos > 0 && (
                <Callout
                  variant="warning"
                  role="status"
                  className="flex flex-wrap items-center gap-2"
                >
                  <span>
                    {data.failedInfos} {data.failedInfos === 1 ? "server" : "servers"}{" "}
                    could not be read, so their secrets are not listed.
                  </span>
                  <Button size="sm" variant="outline" onClick={reload}>
                    Retry
                  </Button>
                </Callout>
              )}
              {rows.length === 0 ? (
                <EmptyState
                  icon={<KeyRound />}
                  title="No logins to manage"
                  description="None of your servers signs in or holds a secret. Add a remote server or a server with an API key and it shows up here."
                />
              ) : (
                <div className="overflow-x-auto rounded-xl border">
                  <table aria-label="Logins" className="w-full border-collapse text-sm">
                    <thead>
                      <tr className="text-left text-2xs tracking-[0.06em] text-muted-foreground uppercase">
                        <th scope="col" className="px-3 py-2 font-medium">
                          Server
                        </th>
                        <th scope="col" className="px-3 py-2 font-medium">
                          Type
                        </th>
                        <th scope="col" className="px-3 py-2 font-medium">
                          State
                        </th>
                        <th scope="col" className="px-3 py-2 font-medium">
                          Last probe
                        </th>
                        <th scope="col" className="px-3 py-2 text-right font-medium">
                          <span className="sr-only">Actions</span>
                        </th>
                      </tr>
                    </thead>
                    <tbody>
                      {rows.map((row) => (
                        <tr key={row.id} className="border-t">
                          <th scope="row" className="px-3 py-2.5 text-left font-normal">
                            <b className="font-semibold">{row.name}</b>
                            <Detail row={row} />
                          </th>
                          <td className="px-3 py-2.5">{row.typeLabel}</td>
                          <td className="px-3 py-2.5">
                            <StateBadge state={row.state} />
                          </td>
                          <td className="px-3 py-2.5 text-muted-foreground">
                            <WhenText seconds={row.auth?.lastProbe} />
                          </td>
                          <td className="px-3 py-2.5">
                            <Actions
                              row={row}
                              busy={busy}
                              probing={probe.target === row.id && busy}
                              onSignIn={() => startSignIn(row, false)}
                              onSetSecret={() => setSecretFor(row)}
                              onProbe={() => void probe.run({ server: row.id, force })}
                              onRefreshTask={
                                refreshTaskOf(row.id)
                                  ? () => {
                                      const task = refreshTaskOf(row.id)!;
                                      setRefreshing({ id: task.id, title: task.title });
                                    }
                                  : undefined
                              }
                            />
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
              {roster.noLogin > 0 && (
                <p className="text-xs text-muted-foreground">
                  {roster.noLogin} other{" "}
                  {roster.noLogin === 1 ? "server needs" : "servers need"} no login. A
                  login that starts to need attention raises a notification once.
                </p>
              )}
            </>
          );
        }}
      </Gate>
      {signing && (
        <SignInDialog
          row={signing.row}
          job={signIn}
          noOpen={signing.noOpen}
          onNoBrowser={() => {
            resetLogin();
            startSignIn(signing.row, true);
          }}
          onClose={closeSignIn}
          onSetSecret={
            signing.row.canSetSecret
              ? () => {
                  const row = signing.row;
                  closeSignIn();
                  setSecretFor(row);
                }
              : undefined
          }
        />
      )}
      {refreshing && (
        <Suspense fallback={null}>
          <RunTaskDialog
            taskId={refreshing.id}
            title={refreshing.title}
            onChanged={reload}
            onClose={() => setRefreshing(null)}
          />
        </Suspense>
      )}
      {secretFor && (
        <SetSecretDialog
          server={{ id: secretFor.id, name: secretFor.name }}
          keys={secretFor.secretKeys}
          existing={() => null}
          onClose={() => setSecretFor(null)}
          onStored={reload}
          onProbe={() => {
            const row = secretFor;
            setSecretFor(null);
            void probe.run({ server: row.id, force: true });
          }}
        />
      )}
    </div>
  );
}
