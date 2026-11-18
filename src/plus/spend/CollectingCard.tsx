import { useState } from "react";
import { Download, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { StatusData } from "./data";
import { collectingCard, profileName } from "./model";
import { Panel } from "./Panel";

const NOTE = "text-meta text-muted-foreground";

function Meter({ fraction, label }: { fraction: number; label: string }) {
  return (
    <div
      role="img"
      aria-label={label}
      className="h-2 w-full max-w-65 overflow-hidden rounded-full bg-muted"
    >
      <i
        className="block h-full rounded-full bg-primary"
        style={{ width: `${Math.round(fraction * 1000) / 10}%` }}
      />
    </div>
  );
}

const STATE_BADGE = {
  collecting: { label: "On", variant: "success" },
  waiting: { label: "Waiting", variant: "warning" },
  "not-listening": { label: "Not listening", variant: "destructive" },
  error: { label: "Not listening", variant: "destructive" },
  off: { label: "Off", variant: "off" },
} as const;

export interface CollectingCardProps {
  status: StatusData;
  now: number;
  /** Days that come from transcripts because no events arrived. */
  closedDays: string[];
  onExport: () => void;
  onPrune: () => void;
  onUndo: () => void;
}

/** Is collecting on, and what does it store. */
export function CollectingCardView({
  status,
  now,
  closedDays,
  onExport,
  onPrune,
  onUndo,
}: CollectingCardProps) {
  const [shown, setShown] = useState(false);
  const card = collectingCard(status, now, closedDays);
  const badge = STATE_BADGE[status.state];
  const hasData = status.store.events > 0 || status.store.dailyLines > 0;
  return (
    <Panel
      id="collecting"
      title="Collecting"
      actions={
        <>
          <Badge variant={badge.variant} data-slot="collecting-state">
            {badge.label}
          </Badge>
          {hasData && (
            <>
              <Button variant="outline" size="sm" onClick={onExport}>
                <Download data-icon="inline-start" />
                Export CSV…
              </Button>
              <Button variant="outline" size="sm" onClick={onPrune}>
                <Trash2 data-icon="inline-start" />
                Prune now…
              </Button>
            </>
          )}
        </>
      }
    >
      <div className="flex flex-col gap-3 p-4">
        <dl className="grid grid-cols-1 gap-x-4 gap-y-1 text-sm @min-[560px]:grid-cols-[96px_minmax(0,1fr)]">
          <dt className={cn(NOTE, "mt-2 @min-[560px]:mt-0")}>Receiver</dt>
          <dd>
            {card.receiver}
            {status.receiver.listening && ` · ${card.drops}`}
          </dd>
          <dt className={cn(NOTE, "mt-2 @min-[560px]:mt-0")}>Source</dt>
          <dd>{card.source}</dd>
          <dt className={cn(NOTE, "mt-2 @min-[560px]:mt-0")}>Stored</dt>
          <dd className="flex flex-wrap items-center gap-2">
            {hasData && (
              <Meter
                fraction={card.storedFraction}
                label={`${card.stored} (${Math.round(card.storedFraction * 100)}% of the cap)`}
              />
            )}
            <span className="tabular-nums">{card.stored}</span>
          </dd>
          {card.keptFraction !== null && (
            <>
              <dt className={cn(NOTE, "mt-2 @min-[560px]:mt-0")}>Kept for</dt>
              <dd className="flex flex-col gap-1">
                <span className="flex flex-wrap items-center gap-2">
                  <Meter fraction={card.keptFraction} label={card.keptFor} />
                  <span className="tabular-nums">{card.keptFor}</span>
                </span>
                {card.rate && <span className={NOTE}>{card.rate}</span>}
              </dd>
            </>
          )}
          <dt className={cn(NOTE, "mt-2 @min-[560px]:mt-0")}>Daily totals</dt>
          <dd>
            {card.dailyTotals}: one line per day, project and model, so the charts keep
            their history.
          </dd>
          <dt className={cn(NOTE, "mt-2 @min-[560px]:mt-0")}>Claude Code</dt>
          <dd className="flex flex-col gap-1">
            <span>
              {card.claudeCode}
              {status.setup.length > 0 && (
                <>
                  {" · "}
                  <button
                    type="button"
                    className="text-primary underline-offset-4 hover:underline"
                    aria-expanded={shown}
                    onClick={() => setShown((value) => !value)}
                  >
                    {shown ? "Hide" : "Show"}
                  </button>
                  {" · "}
                  <button
                    type="button"
                    className="text-primary underline-offset-4 hover:underline"
                    onClick={onUndo}
                  >
                    Undo setup…
                  </button>
                </>
              )}
            </span>
            {shown && (
              <ul className="font-mono text-xs" data-slot="setup-profiles">
                {status.setup.map((record) => (
                  <li key={record.profile}>{profileName(record.profile)}</li>
                ))}
              </ul>
            )}
          </dd>
        </dl>
        <div className="rounded-lg bg-muted/50 p-3 text-meta text-muted-foreground">
          <b className="text-foreground">Stored per event:</b> time, session id, model,
          token counts by kind, cost, tool name, MCP server name, working folder.{" "}
          <b className="text-foreground">Never stored:</b> prompts, tool arguments or
          results, file contents. Long text values are cut and passed through the secret
          redaction.
        </div>
      </div>
    </Panel>
  );
}
