import type { Badge, SummaryData } from "./data";
import { cacheLines, count, percent, RANGES, type CacheLine } from "./model";
import { Panel } from "./Panel";

const DOT: Record<CacheLine["id"], string> = {
  cacheRead: "bg-primary",
  cacheCreation: "bg-warning",
  input: "bg-success",
  output: "bg-foreground",
};

/** Cache against fresh tokens. Claude Code reports cost per request, not per token kind, so
 * no row carries a cost. */
export function CacheCard({ summary, badge }: { summary: SummaryData; badge: Badge }) {
  const lines = cacheLines(summary.totals.tokens);
  const label = RANGES.find((r) => r.id === summary.range)?.label.toLowerCase();
  const { totals } = summary;
  return (
    <Panel
      title="Cache vs fresh tokens"
      badge={badge}
      footer="Claude Code reports cost per request, not per token kind, so no cost is shown per row."
    >
      <div className="flex flex-col gap-3 p-4">
        <div>
          <div className="text-[26px] leading-tight font-semibold tabular-nums">
            {percent(summary.cacheShare.value)}
          </div>
          <div className="text-meta text-muted-foreground">
            of input-side tokens were cache reads ({label})
          </div>
        </div>
        <div
          role="img"
          aria-label={lines.map((l) => `${l.label} ${l.share}`).join(", ")}
          className="flex h-2.5 overflow-hidden rounded-full bg-muted"
        >
          {lines.map((line) => (
            <i
              key={line.id}
              className={DOT[line.id]}
              style={{ width: `${(line.tokens / (totals.tokens.total || 1)) * 100}%` }}
            />
          ))}
        </div>
        <dl className="grid grid-cols-[minmax(0,1fr)_auto_auto] gap-x-4 gap-y-1 text-meta">
          {lines.map((line) => (
            <div key={line.id} className="contents">
              <dt className="flex items-center gap-2">
                <i className={`size-2 rounded-full ${DOT[line.id]}`} />
                {line.label}
              </dt>
              <dd className="text-right tabular-nums">{line.text}</dd>
              <dd className="text-right text-muted-foreground tabular-nums">
                {line.share}
              </dd>
            </div>
          ))}
        </dl>
        <dl className="grid grid-cols-[96px_minmax(0,1fr)] gap-y-1 border-t pt-3 text-meta">
          <dt className="text-muted-foreground">Requests</dt>
          <dd className="tabular-nums">
            {count(totals.requests)} in {count(totals.sessions)}{" "}
            {totals.sessions === 1 ? "session" : "sessions"}
          </dd>
        </dl>
      </div>
    </Panel>
  );
}
