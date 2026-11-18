import { useState } from "react";
import { ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { McpPart } from "./data";
import { mcpCard, percent, SKILLS_NOTE, type FirstRequestSegment } from "./model";
import { KindBadge } from "./KindBadge";
import { Panel } from "./Panel";

const SEGMENT: Record<FirstRequestSegment["id"], string> = {
  instructions: "bg-success",
  tools: "bg-primary",
  rest: "",
};

const HATCH =
  "repeating-linear-gradient(45deg, var(--muted-foreground) 0 2px, transparent 2px 6px)";

const NOTE = "text-meta text-muted-foreground";
const BIG = "text-[26px] leading-tight font-semibold tabular-nums";

export interface McpCardProps {
  part: McpPart;
  requests: number;
  onRetry: () => void;
  onOpenActivity: () => void;
  onOpenMeasurements?: () => void;
}

/** What the tool definitions cost, kept apart from the spend: upstream's saved tokens are
 * never added to or subtracted from it. */
export function McpCardView({
  part,
  requests,
  onRetry,
  onOpenActivity,
  onOpenMeasurements,
}: McpCardProps) {
  const card = mcpCard(part, requests);
  const [details, setDetails] = useState(false);
  const header = {
    title: "MCP part",
    note: "What the tool definitions cost, kept apart from the spend above",
    actions: (
      <Button variant="outline" size="xs" onClick={onOpenActivity}>
        <ExternalLink data-icon="inline-start" />
        Savings in Activity
      </Button>
    ),
  };

  if (card.failed)
    return (
      <Panel {...header}>
        <div className="flex flex-col items-start gap-3 p-4" data-slot="mcp-failed">
          <div className="flex flex-col gap-1">
            <b>Upstream's savings log could not be read.</b>
            <span className="text-meta text-muted-foreground">
              The other cards are not affected; their numbers are unchanged.
            </span>
            {details && (
              <code
                id="mcp-failure"
                className="mt-1 break-all rounded-md bg-muted p-2 font-mono text-meta"
              >
                {card.failed}
              </code>
            )}
          </div>
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" size="sm" onClick={onRetry}>
              Retry
            </Button>
            <Button
              variant="ghost"
              size="sm"
              aria-expanded={details}
              aria-controls="mcp-failure"
              onClick={() => setDetails((on) => !on)}
            >
              {details ? "Hide details" : "Show details"}
            </Button>
          </div>
        </div>
      </Panel>
    );

  const request = card.firstRequest;
  return (
    <Panel
      {...header}
      footer={
        <>
          <b className="text-foreground">Upstream's number stays separate.</b> The
          sidebar's{" "}
          {card.upstreamSaved && (
            <>
              <b className="text-foreground">{card.upstreamSaved} tokens saved</b>{" "}
            </>
          )}
          counts tool-definition exposure of MCP servers only. It is never added to or
          subtracted from the spend on this page.
        </>
      }
    >
      <div className="grid @min-[780px]:grid-cols-3">
        <div className="flex flex-col gap-1 border-b p-4 @min-[780px]:border-r @min-[780px]:border-b-0">
          <div className={cn(NOTE, "flex flex-wrap items-center gap-2")}>
            Tool definitions per session <KindBadge kind="Counted" />
          </div>
          {card.toolDefinitions ? (
            <>
              <div className={BIG}>{card.toolDefinitions.exposed}</div>
              <div className={NOTE}>
                exposed to Claude (median of {card.toolDefinitions.sessions} sessions)
                against <b className="text-foreground">{card.toolDefinitions.full}</b> if
                every tool were listed.
              </div>
              <div className={cn(NOTE, "mt-1")}>
                Toolport's own count with the{" "}
                <code className="font-mono">{card.toolDefinitions.tokenizer}</code>{" "}
                tokenizer, so it differs a little from Claude's. Kept out of the context:{" "}
                {card.toolDefinitions.saved} tokens in this range.
              </div>
            </>
          ) : (
            <div className={NOTE}>
              No tool-definition exposure was recorded in this range.
            </div>
          )}
        </div>
        <div className="flex flex-col gap-1 border-b p-4 @min-[780px]:border-r @min-[780px]:border-b-0">
          <div className={cn(NOTE, "flex flex-wrap items-center gap-2")}>
            Share of your input-side tokens{" "}
            <KindBadge kind={card.share?.badge ?? "Estimate"} />
          </div>
          {card.share ? (
            <>
              <div className={BIG}>{card.share.text}</div>
              <div className={NOTE}>{card.share.detail}</div>
            </>
          ) : (
            <div className={NOTE}>
              There are no input-side tokens to compare with yet.
            </div>
          )}
          <div className={cn(NOTE, "mt-1")}>
            Billing does not split a request into parts, so there is no dollar figure
            here.
          </div>
          <details className={NOTE}>
            <summary className="cursor-pointer text-primary">
              How this is estimated
            </summary>
            <p className="mt-1">
              Tool-definition tokens exposed per request (Toolport's count) times the
              requests of the range, divided by the input-side tokens Claude Code
              reported. Claude Code does not report tool definitions on their own, so this
              is an estimate.
            </p>
          </details>
        </div>
        <div className="flex flex-col gap-1 p-4">
          <div className={cn(NOTE, "flex flex-wrap items-center gap-2")}>
            Compression, counted at the hook <KindBadge kind="Counted" />
          </div>
          {card.compression.cut ? (
            <>
              <div className={BIG}>{card.compression.cut}</div>
              <div className={NOTE}>
                tokens cut before they reached the context ({card.compression.hooks}{" "}
                {card.compression.hooks === 1 ? "hook" : "hooks"},{" "}
                {card.compression.calls.toLocaleString("en-US")} calls
                {card.compression.window ? `, ${card.compression.window}` : ""}).
              </div>
            </>
          ) : (
            <div className={NOTE}>
              No compression savings recorded yet
              {card.compression.calls > 0
                ? ` (${card.compression.calls.toLocaleString("en-US")} hook calls)`
                : ""}
              .
            </div>
          )}
          {card.compression.note && (
            <div className={cn(NOTE, "mt-1")}>
              <b className="text-foreground">Billed effect:</b> {card.compression.note}
              {onOpenMeasurements && (
                <>
                  {" "}
                  <button
                    type="button"
                    className="text-primary underline-offset-4 hover:underline"
                    onClick={onOpenMeasurements}
                  >
                    Open measurements
                  </button>
                </>
              )}
            </div>
          )}
        </div>
      </div>
      <div className="flex flex-col gap-2 border-t p-4" data-slot="first-request">
        {request ? (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <b>First request in a folder, measured</b>
              <span className={NOTE}>
                {request.total} tokens ·{" "}
                <code className="font-mono">{request.folder}</code> · context measure{" "}
                {request.measured}, {request.model}
              </span>
              <KindBadge kind="Measured" />
            </div>
            <div
              role="img"
              aria-label={request.segments
                .map((s) => `${s.label} ${s.tokens.toLocaleString("en-US")} ${s.badge}`)
                .join("; ")}
              className="flex h-3.5 overflow-hidden rounded-full bg-muted"
            >
              {request.segments.map((segment) => (
                <i
                  key={segment.id}
                  className={SEGMENT[segment.id]}
                  style={{
                    width: percent(segment.fraction, 1),
                    ...(segment.id === "rest"
                      ? { backgroundImage: HATCH, opacity: 0.5 }
                      : {}),
                  }}
                />
              ))}
            </div>
            <ul className="flex flex-wrap gap-x-4 gap-y-1 text-meta text-muted-foreground">
              {request.segments.map((segment) => (
                <li key={segment.id} className="flex items-center gap-1.5">
                  <i
                    className={cn("size-2.5 rounded-sm border", SEGMENT[segment.id])}
                    style={
                      segment.id === "rest"
                        ? { backgroundImage: HATCH, opacity: 0.6 }
                        : undefined
                    }
                  />
                  {segment.label} {segment.tokens.toLocaleString("en-US")}{" "}
                  <KindBadge kind={segment.badge} />
                </li>
              ))}
            </ul>
            <p className={NOTE}>{SKILLS_NOTE}</p>
          </>
        ) : (
          <p className={NOTE}>
            No folder has been measured yet. A measurement in Context shows what the first
            request of a session holds.
          </p>
        )}
      </div>
    </Panel>
  );
}
