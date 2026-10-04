import { useCallback, useEffect, useState } from "react";
import { CheckCircle2, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { SectionHeader } from "@/components/ui/section-header";
import { CtlError, ctlData } from "../bridge/ctl";
import { doctorData, type DoctorData } from "../bridge/data";
import { check } from "../bridge/shape";
import { commandLine } from "../allcommands/model";
import { CopyButton, ErrorState, ScreenSkeleton, humanKey } from "../ui";
import { Code, Gate } from "./atoms";
import { useWrite } from "./useWrite";
import { WriteDialogs } from "./WriteDialogs";
import { namedFix, policyOf, type DoctorCheck } from "./model";
import { useServers, type TabId } from "./useServers";

const NAMES: Record<string, string> = {
  dataDir: "Data folder",
  registry: "Registry",
  activeProfile: "Active profile",
  secretsBackend: "Secrets",
  gatewayBinary: "Gateway binary",
  directEntries: "Direct entries",
  skills: "Skills",
};

const GOES_TO: Record<string, { tab: TabId; label: string }> = {
  activeProfile: { tab: "profiles", label: "Open Profiles" },
  directEntries: { tab: "clients", label: "Open Clients" },
};

const STATUS: Record<
  string,
  { label: string; variant: "success" | "warning" | "destructive" }
> = {
  ok: { label: "OK", variant: "success" },
  warn: { label: "Warning", variant: "warning" },
  fail: { label: "Failed", variant: "destructive" },
};

/** `doctor` exits non-zero when a check fails but still answers with the checks. */
async function readDoctor(): Promise<DoctorData> {
  try {
    return await ctlData<DoctorData>(["doctor"]);
  } catch (error) {
    if (error instanceof CtlError && check(doctorData, error.data).length === 0) {
      return error.data as DoctorData;
    }
    throw error;
  }
}

interface Doctor {
  state: "loading" | "ready" | "error";
  data: DoctorData | null;
  error: unknown;
}

function useDoctor() {
  const [tick, setTick] = useState(0);
  const [settled, setSettled] = useState<{
    tick: number;
    data: DoctorData | null;
    error: unknown;
  } | null>(null);
  useEffect(() => {
    let alive = true;
    readDoctor().then(
      (data) => alive && setSettled({ tick, data, error: null }),
      (error) =>
        alive && setSettled((prev) => ({ tick, data: prev?.data ?? null, error })),
    );
    return () => {
      alive = false;
    };
  }, [tick]);
  const fresh = settled?.tick === tick;
  const doctor: Doctor = {
    state: !fresh ? "loading" : settled.error ? "error" : "ready",
    data: settled?.data ?? null,
    error: fresh ? settled.error : null,
  };
  return { doctor, rerun: useCallback(() => setTick((n) => n + 1), []) };
}

function Stat({
  label,
  value,
  detail,
  tone,
}: {
  label: string;
  value: string;
  detail?: string;
  tone: "success" | "warning" | "destructive" | "muted";
}) {
  const dot = {
    success: "bg-success",
    warning: "bg-warning",
    destructive: "bg-destructive",
    muted: "bg-muted-foreground/50",
  }[tone];
  return (
    <div className="flex min-w-0 flex-col gap-0.5 rounded-lg border bg-card px-3 py-2">
      <span className="text-xs text-muted-foreground">{label}</span>
      <b className="flex items-center gap-2 text-sm font-semibold">
        <span className={`size-2 shrink-0 rounded-full ${dot}`} aria-hidden="true" />
        {value}
      </b>
      {detail && (
        <small className="truncate text-xs text-muted-foreground">{detail}</small>
      )}
    </div>
  );
}

function FixButton({
  check: row,
  fix,
  onRun,
}: {
  check: DoctorCheck;
  fix: string[];
  onRun: (fix: string[]) => void;
}) {
  const { rows, go } = useServers();
  const id = fix.join(" ");
  const policy = policyOf(rows, id);
  const target = GOES_TO[row.name];
  return (
    <div className="flex flex-wrap items-center gap-2">
      {policy && !policy.terminal ? (
        <Button size="sm" onClick={() => onRun(fix)}>
          Run {id}
        </Button>
      ) : (
        <>
          <Code>{commandLine(fix)}</Code>
          <CopyButton text={commandLine(fix)} label="Copy command" />
        </>
      )}
      {target && (
        <Button size="sm" variant="outline" onClick={() => go(target.tab)}>
          {target.label}
        </Button>
      )}
    </div>
  );
}

export function HealthTab() {
  const { status, statusDoc, registry, go } = useServers();
  const { doctor, rerun } = useDoctor();
  const write = useWrite();
  const unhealthy = (doctor.data?.checks ?? []).filter((row) => row.status !== "ok");
  const logins = statusDoc?.auth.servers.filter(
    (row) => row.state === "needs_reauth" || row.state === "revoked",
  );
  const run = (fix: string[]) =>
    write.begin({
      command: fix.join(" "),
      title: `Run ${fix.join(" ")}`,
      argv: fix,
      after: rerun,
    });

  return (
    <div className="flex flex-col gap-4">
      <Gate queries={[status]} title="Couldn't read the status" context="status">
        {() => {
          const doc = statusDoc;
          if (!doc) return null;
          const needLogin = logins?.length ?? 0;
          return (
            <div
              aria-label="Facts"
              role="group"
              className="grid gap-2 sm:grid-cols-2 lg:grid-cols-5"
            >
              <Stat
                label="Registry"
                value={
                  doc.registry.error
                    ? "Unreadable"
                    : `${doc.serverCount} servers, ${doc.profileCount} profiles`
                }
                detail={doc.registry.error ?? doc.activeProfile}
                tone={doc.registry.error ? "destructive" : "success"}
              />
              <Stat label="Secrets" value={doc.secretsBackend} tone="success" />
              <Stat
                label="Gateway binary"
                value={doc.gateway.present ? "Found" : "Missing"}
                detail={doc.gateway.path}
                tone={doc.gateway.present ? "success" : "destructive"}
              />
              <Stat
                label="Self-management MCP"
                value={registry.data ? `${registry.data.tools.length} tools` : "Reading"}
                tone="muted"
              />
              <Stat
                label="Logins"
                value={needLogin > 0 ? `${needLogin} need a sign-in` : "All signed in"}
                detail={logins
                  ?.slice(0, 4)
                  .map((row) => row.server)
                  .join(", ")}
                tone={needLogin > 0 ? "warning" : "success"}
              />
            </div>
          );
        }}
      </Gate>

      <section aria-label="Fixes" className="flex flex-col gap-2">
        <SectionHeader tone="warning" count={unhealthy.length + (logins?.length ? 1 : 0)}>
          Fixes
        </SectionHeader>
        {logins && logins.length > 0 && (
          <Callout variant="warning" className="flex flex-col gap-2">
            <p>
              <b>
                {logins.length} server{logins.length === 1 ? " needs" : "s need"} a
                sign-in
              </b>
              <br />
              {logins.map((row) => row.server).join(", ")}
            </p>
            <div>
              <Button size="sm" variant="outline" onClick={() => go("logins")}>
                Open Logins
              </Button>
            </div>
          </Callout>
        )}
        {unhealthy.map((row) => {
          const fix = namedFix(row);
          const target = GOES_TO[row.name];
          return (
            <Callout
              key={row.name}
              variant={row.status === "fail" ? "danger" : "warning"}
              className="flex flex-col gap-2"
            >
              <p>
                <b>{NAMES[row.name] ?? humanKey(row.name)}</b>
                <br />
                <span className="break-words">{row.detail}</span>
              </p>
              {fix ? (
                <FixButton check={row} fix={fix} onRun={run} />
              ) : (
                target && (
                  <div>
                    <Button size="sm" variant="outline" onClick={() => go(target.tab)}>
                      {target.label}
                    </Button>
                  </div>
                )
              )}
            </Callout>
          );
        })}
        {doctor.state === "ready" && unhealthy.length === 0 && !logins?.length && (
          <p className="flex items-center gap-2 text-sm text-muted-foreground">
            <CheckCircle2 className="size-4 text-success" aria-hidden="true" />
            Nothing to fix.
          </p>
        )}
      </section>

      <section aria-label="Checks" className="flex flex-col gap-2">
        <SectionHeader
          action={
            <Button
              size="sm"
              variant="outline"
              onClick={rerun}
              disabled={doctor.state === "loading"}
            >
              <RefreshCw /> Run doctor
            </Button>
          }
        >
          Checks
        </SectionHeader>
        {doctor.state === "loading" && !doctor.data && (
          <ScreenSkeleton rows={4} label="Running doctor" />
        )}
        {doctor.state === "error" && (
          <ErrorState
            error={doctor.error}
            title="Doctor could not run"
            context="doctor"
            onRetry={rerun}
          />
        )}
        {doctor.data && (
          <ul
            aria-label="Doctor checks"
            className="flex flex-col divide-y rounded-lg border"
          >
            {doctor.data.checks.map((row) => {
              const badge = STATUS[row.status] ?? STATUS.warn;
              return (
                <li
                  key={row.name}
                  className="grid grid-cols-[minmax(7rem,10rem)_auto_minmax(0,1fr)] items-center gap-3 px-3 py-2 text-sm"
                >
                  <span className="font-medium">
                    {NAMES[row.name] ?? humanKey(row.name)}
                  </span>
                  <Badge variant={badge.variant}>{badge.label}</Badge>
                  <span className="min-w-0 text-xs break-words text-muted-foreground">
                    {row.detail}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
      </section>
      <WriteDialogs write={write} />
    </div>
  );
}
