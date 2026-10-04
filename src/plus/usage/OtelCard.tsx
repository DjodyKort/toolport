import { useMemo, useState, type FormEvent, type ReactNode } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Callout } from "@/components/Callout";
import { ErrorState, ScreenSkeleton, errorText, type CtlQuery } from "../ui";
import { Section } from "./atoms";
import {
  DISABLE_ARGV,
  ENABLE_NOTICE,
  enableArgv,
  exact,
  formatTs,
  keyLabel,
  otelPlan,
  parsePort,
  parseStatus,
  receiverLabel,
  settingsLabel,
  type OtelStatus,
  type UsageView,
} from "./model";
import type { WriteControl } from "./useWrite";

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="contents">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 break-words">{children}</dd>
    </div>
  );
}

function Facts({ status, usage }: { status: OtelStatus; usage: UsageView | null }) {
  const receiver = receiverLabel(status.receiver.state);
  const { events } = status;
  return (
    <dl className="grid grid-cols-[minmax(8rem,max-content)_minmax(0,1fr)] gap-x-4 gap-y-2.5 text-sm">
      <Row label="Receiver">
        <span className="inline-flex flex-wrap items-center gap-2">
          <Badge variant={receiver.tone}>{receiver.label}</Badge>
          <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs break-all">
            {status.endpoint}
          </code>
        </span>
      </Row>
      <Row label="Stored events">
        {events.count === 0
          ? "none yet"
          : `${exact(events.count)}${events.latest ? `, newest ${formatTs(events.latest)}` : ""}`}
      </Row>
      <Row label="Not in transcripts">
        {usage === null ? (
          "n/a until the usage figures load"
        ) : (
          <>
            {exact(usage.sources.otelOnly)} of {exact(usage.sources.otelRequests)} API
            requests
            <small className="block text-xs text-muted-foreground">
              Requests the receiver saw that no transcript holds. They are counted in the
              totals once.
            </small>
          </>
        )}
      </Row>
      <Row label="Claude settings">
        <span className="block">{settingsLabel(status.settings.state)}</span>
        <code className="font-mono text-xs break-all text-muted-foreground">
          {status.settings.path}
        </code>
      </Row>
    </dl>
  );
}

function Keys({ status }: { status: OtelStatus }) {
  return (
    <ul
      aria-label="Telemetry keys in the env block of the Claude settings"
      className="divide-y rounded-lg border text-sm"
    >
      {status.settings.keys.map(([key, state]) => {
        const label = keyLabel(state);
        return (
          <li
            key={key}
            className="flex flex-wrap items-center justify-between gap-2 px-3 py-1.5"
          >
            <code className="font-mono text-xs break-all">{key}</code>
            <Badge variant={label.tone}>{label.label}</Badge>
          </li>
        );
      })}
    </ul>
  );
}

function Reported({ usage }: { usage: UsageView }) {
  const { otel } = usage;
  if (otel.events === 0) return null;
  const decisions = otel.toolDecisions
    .map(([name, n]) => `${name} ${exact(n)}`)
    .join(", ");
  return (
    <div className="flex flex-col gap-1 text-sm" aria-label="Reported by Claude Code">
      <b className="text-sm font-medium">Reported by Claude Code</b>
      <p className="text-muted-foreground">
        {exact(otel.apiRequests.count)} API requests, cost ${otel.costUsd.toFixed(2)}
        {decisions ? `, tool decisions: ${decisions}` : ""}
        {otel.connections.total > 0
          ? `, ${exact(otel.connections.total)} MCP connections`
          : ""}
        {otel.connections.failures.length > 0
          ? ` (failed: ${otel.connections.failures.map((f) => `${f.server} x${f.count}`).join(", ")})`
          : ""}
        .
      </p>
    </div>
  );
}

function Controls({ status, write }: { status: OtelStatus; write: WriteControl }) {
  const [typed, setTyped] = useState<string | null>(null);
  const text = typed ?? String(status.port);
  const port = parsePort(text);
  const invalid = port === null;
  const busy = write.flow.busy;
  const enable = (given: number) =>
    write.begin({
      command: "obs otel enable",
      title: `Enable the OTel receiver on port ${given}`,
      argv: enableArgv(given),
      confirmLabel: "Enable receiver",
      phrase: "enable",
      notice: ENABLE_NOTICE,
      plan: (data) => otelPlan(data, "enable", "toolportctl obs otel disable"),
    });
  const disable = () =>
    write.begin({
      command: "obs otel disable",
      title: "Disable the OTel receiver",
      argv: DISABLE_ARGV,
      confirmLabel: "Disable receiver",
      phrase: "disable",
      plan: (data) =>
        otelPlan(data, "disable", `toolportctl ${enableArgv(status.port).join(" ")}`),
    });
  function submit(event: FormEvent) {
    event.preventDefault();
    if (port !== null) enable(port);
  }
  if (status.enabled) {
    const repair = status.settings.state !== "configured";
    return (
      <div className="flex flex-col gap-2">
        {repair && (
          <Callout variant="warning" role="status">
            The receiver is on, but not every telemetry key in your Claude settings is
            set. Apply again to restore them.
          </Callout>
        )}
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" variant="outline" disabled={busy} onClick={disable}>
            Disable…
          </Button>
          {repair && (
            <Button
              size="sm"
              disabled={busy}
              onClick={() => enable(status.port)}
              aria-label={`Apply again on port ${status.port}`}
            >
              Apply again…
            </Button>
          )}
        </div>
      </div>
    );
  }
  return (
    <form onSubmit={submit} className="flex flex-wrap items-end gap-2">
      <label className="flex flex-col gap-1 text-xs text-muted-foreground">
        Port
        <Input
          value={text}
          inputMode="numeric"
          aria-invalid={invalid}
          aria-describedby={invalid ? "otel-port-error" : undefined}
          className="w-28"
          onChange={(event) => setTyped(event.target.value)}
        />
      </label>
      <Button type="submit" size="sm" disabled={invalid || busy}>
        Enable…
      </Button>
      {invalid && (
        <small id="otel-port-error" role="alert" className="text-xs text-destructive">
          Use a whole number from 1 to 65535.
        </small>
      )}
    </form>
  );
}

/** The OTel card: what the receiver and the Claude settings look like now, and Enable and
 * Disable, each previewed with the keys it touches before anything is written (D-059). */
export function OtelCard({
  status,
  usage,
  write,
  onOpenCommands,
}: {
  status: CtlQuery<unknown>;
  usage: UsageView | null;
  write: WriteControl;
  onOpenCommands?: (group?: string) => void;
}) {
  const data = useMemo(
    () => (status.data === null ? null : parseStatus(status.data)),
    [status.data],
  );
  const keys = data ? data.settings.keys.length : 5;
  return (
    <Section
      title="OpenTelemetry receiver"
      note={`Counts API requests that are missing from the transcripts. It listens on this computer only and touches ${keys} keys in your Claude settings.`}
      action={
        <div className="flex gap-2">
          <Button
            size="xs"
            variant="outline"
            onClick={status.reload}
            disabled={status.status === "loading"}
          >
            Check status
          </Button>
          {onOpenCommands && (
            <Button size="xs" variant="ghost" onClick={() => onOpenCommands("obs")}>
              All commands
            </Button>
          )}
        </div>
      }
    >
      {data === null ? (
        status.status === "error" ? (
          <ErrorState
            error={status.error}
            title={
              errorText(status.error).code === "bridge"
                ? "Toolport could not run toolportctl"
                : "Couldn't read the receiver status"
            }
            context="obs otel status"
            onRetry={status.reload}
          />
        ) : (
          <ScreenSkeleton rows={3} label="Loading the receiver status" />
        )
      ) : (
        <>
          {status.status === "error" && (
            <ErrorState
              error={status.error}
              title="Couldn't refresh the receiver status"
              context="obs otel status"
              onRetry={status.reload}
            />
          )}
          <Facts status={data} usage={usage} />
          {data.receiver.error && (
            <Callout variant="warning" role="status">
              {data.receiver.error}
            </Callout>
          )}
          {data.settings.error && (
            <Callout variant="warning" role="status">
              {data.settings.error}
            </Callout>
          )}
          {data.settings.warnings.length > 0 && (
            <Callout variant="warning" role="status">
              <ul className="list-disc pl-4">
                {data.settings.warnings.map((warning, i) => (
                  <li key={i}>{warning}</li>
                ))}
              </ul>
            </Callout>
          )}
          <Keys status={data} />
          {usage && <Reported usage={usage} />}
          <Controls status={data} write={write} />
        </>
      )}
    </Section>
  );
}
