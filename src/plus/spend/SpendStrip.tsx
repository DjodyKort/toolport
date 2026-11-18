import type { ReactNode } from "react";
import { CircleAlert, CircleCheck, Info, TriangleAlert } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import type { StatusData, SummaryData } from "./data";
import {
  notices,
  progress,
  staleCauses,
  strip,
  type SpendState,
  type Tone,
} from "./model";

const ICON = {
  ok: <CircleCheck />,
  info: <Info />,
  warning: <TriangleAlert />,
  danger: <CircleAlert />,
  off: <Info />,
} as const;

const VARIANT: Record<Tone, "success" | "info" | "warning" | "danger"> = {
  ok: "success",
  info: "info",
  off: "info",
  warning: "warning",
  danger: "danger",
};

export function Callout({
  tone = "info",
  children,
  testId,
}: {
  tone?: "info" | "warning" | "danger";
  children: ReactNode;
  testId?: string;
}) {
  const bar = {
    info: "",
    warning: "shadow-[inset_3px_0_0_var(--warning)]",
    danger: "shadow-[inset_3px_0_0_var(--destructive)]",
  }[tone];
  return (
    <div
      data-testid={testId}
      className={`rounded-xl border bg-card p-3 text-meta text-muted-foreground ${bar}`}
    >
      {children}
    </div>
  );
}

export interface SpendStripProps {
  state: SpendState;
  status: StatusData;
  summary: SummaryData | null;
  now: number;
  elapsedMs: number;
  onCancel: () => void;
  onRetry: () => void;
  onSetup?: () => void;
  onUsePort?: (port: number) => void;
}

function jumpToCollecting() {
  document.getElementById("collecting")?.scrollIntoView?.({ block: "start" });
}

/** The strip under the header and the warnings that belong to the state of collecting. */
export function SpendStrip({
  state,
  status,
  summary,
  now,
  elapsedMs,
  onCancel,
  onRetry,
  onSetup,
  onUsePort,
}: SpendStripProps) {
  const bar = state === "computing" ? progress(status, elapsedMs) : null;
  const line = bar
    ? {
        tone: "info" as const,
        title: bar.title,
        detail: bar.detail,
        action: "cancel" as const,
      }
    : strip(state, status, now);
  const nextPort = status.receiver.address.includes(":")
    ? Number(status.receiver.address.split(":").pop()) + 1
    : 4319;

  const actions =
    line.action === "cancel" ? (
      <Button variant="outline" size="sm" onClick={onCancel}>
        Cancel
      </Button>
    ) : line.action === "setup" ? (
      <Button size="sm" onClick={onSetup}>
        Set up collecting…
      </Button>
    ) : line.action === "port" ? (
      <>
        <Button size="sm" onClick={() => onUsePort?.(nextPort)}>
          Use port {nextPort}…
        </Button>
        <Button variant="outline" size="sm" onClick={onRetry}>
          Retry
        </Button>
      </>
    ) : line.action === "details" ? (
      <>
        {state === "healthy" && <Badge variant="outline">OTel primary</Badge>}
        <Button variant="ghost" size="sm" onClick={jumpToCollecting}>
          Collecting details
        </Button>
      </>
    ) : undefined;

  return (
    <div className="flex flex-col gap-3" data-slot="spend-strip-group">
      <StatusCard
        variant={VARIANT[line.tone]}
        icon={ICON[line.tone]}
        title={line.title}
        detail={line.detail}
        actions={actions}
        testId="spend-strip"
      >
        {bar && bar.fraction !== null && (
          <div
            role="progressbar"
            aria-label="Transcripts read"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(bar.fraction * 100)}
            className="h-1.5 overflow-hidden rounded-full bg-muted"
          >
            <i
              className="block h-full rounded-full bg-primary"
              style={{ width: `${bar.fraction * 100}%` }}
            />
          </div>
        )}
      </StatusCard>
      {state === "receiverCannotListen" && (
        <Callout tone="danger">
          <b className="text-foreground">
            Claude Code is exporting to {status.receiver.address.split(":").pop()}, so its
            data goes to the other program, not to Toolport.
          </b>{" "}
          Using another port shows a plan first: it changes{" "}
          <code className="font-mono">OTEL_EXPORTER_OTLP_ENDPOINT</code> and nothing else.
          Data stored so far stays on this page.
        </Callout>
      )}
      {state === "waitingFirstEvent" && (
        <Callout>
          <b className="text-foreground">
            Restart Claude Code sessions that are already open.
          </b>{" "}
          Claude Code reads its settings when a session starts, so only new sessions
          export. Until the first event arrives the page shows transcripts, marked as
          estimates.
        </Callout>
      )}
      {state === "offWithTranscripts" && (
        <Callout tone="warning">
          <b className="text-foreground">Transcripts only.</b> Tokens are exact (billed
          usage Claude Code wrote down). Cost is our estimate from list prices. History
          goes back as far as Claude Code keeps transcripts. No per-task split and no tool
          or MCP server detail.
        </Callout>
      )}
      {notices(summary, status, now).map((notice) => (
        <StatusCard
          key={notice.id}
          variant={VARIANT[notice.tone]}
          icon={ICON[notice.tone]}
          title={notice.title}
          detail={notice.detail}
          testId={`spend-notice-${notice.id}`}
        >
          {notice.id === "not-exporting" && summary && (
            <div className="flex flex-col gap-2">
              <b className="text-meta">Most likely causes, checked now:</b>
              <ul className="list-disc pl-5 text-meta text-muted-foreground">
                {staleCauses(summary, status).map((cause) => (
                  <li key={cause}>{cause}</li>
                ))}
              </ul>
              <div>
                <Button variant="outline" size="sm" onClick={onSetup}>
                  Open plan…
                </Button>
              </div>
            </div>
          )}
        </StatusCard>
      ))}
    </div>
  );
}
