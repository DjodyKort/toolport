import { Table2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";
import type { Badge } from "./data";
import { chartTable, legend, tokens, type Bar, type ModelFamily } from "./model";
import { Panel } from "./Panel";

const CHART_HEIGHT = 150;
const LABELLED_DAYS = 7;
const LABEL_ROOM = 22;

const FAMILY_CLASS: Record<ModelFamily, string> = {
  Opus: "bg-primary",
  Sonnet: "bg-warning",
  Haiku: "bg-success",
  Other: "bg-muted-foreground",
};

const HATCH = "repeating-linear-gradient(45deg, var(--card) 0 2px, transparent 2px 6px)";

const isEstimate = (bar: Bar) => bar.usd > 0 && bar.hatched >= 0.999;

function dayNames(names: string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

function valueOf(bar: Bar, showCost: boolean): string {
  if (!showCost) return tokens(bar.tokens);
  return bar.label;
}

function ariaOf(bar: Bar, showCost: boolean): string {
  const value = showCost ? `$${bar.usd.toFixed(2)}` : `${tokens(bar.tokens)} tokens`;
  const basis = isEstimate(bar)
    ? ", estimated from transcripts"
    : bar.hatched > 0
      ? ", partly estimated from transcripts"
      : "";
  return `${bar.weekday} ${bar.monthDay}: ${value}${basis}`;
}

export interface CostByDayProps {
  bars: Bar[];
  showCost: boolean;
  table: boolean;
  badge: Badge;
  rangeLabel: string;
  onTable: (on: boolean) => void;
}

/** Cost by day, stacked by model; days that come from transcripts are hatched. The same data
 * is a table behind the Table button (T). */
export function CostByDay({
  bars,
  showCost,
  table,
  badge,
  rangeLabel,
  onTable,
}: CostByDayProps) {
  const labelled = bars.length <= LABELLED_DAYS;
  const estimated = bars.filter(isEstimate);
  const measured = bars.some((bar) => bar.usd > 0 && bar.hatched < 0.999);
  const families = legend(bars);
  const title = showCost ? "Cost by day" : "Tokens by day";
  const rows = chartTable(bars);

  return (
    <Panel
      title={title}
      badge={badge}
      actions={
        <Button
          variant="outline"
          size="xs"
          aria-pressed={table}
          onClick={() => onTable(!table)}
        >
          <Table2 data-icon="inline-start" />
          Table
          <kbd className="rounded border px-1 text-[10px] font-medium">T</kbd>
        </Button>
      }
      footer={
        <>
          {estimated.length > 0 && estimated.length < bars.length && (
            <>
              <b className="text-foreground">
                {dayNames(estimated.map((bar) => bar.weekday))}{" "}
                {estimated.length === 1 ? "is" : "are"} hatched:
              </b>{" "}
              the Toolport app was closed, so no OTel events arrived.{" "}
            </>
          )}
          {estimated.length > 0 && estimated.length === bars.length && (
            <>
              <b className="text-foreground">Every day is hatched:</b> no OTel events
              arrived.{" "}
            </>
          )}
          {estimated.length > 0 &&
            "Tokens come from the transcripts that Claude Code kept; cost is our estimate from list prices. "}
          {measured &&
            "Cost for measured days is the figure Claude Code reports per request, which is its own estimate and not your invoice."}
          {estimated.length === 0 && !measured && "Nothing was spent in this range."}
        </>
      }
    >
      <div className="flex flex-col gap-3 p-4">
        {table ? (
          <div className="overflow-x-auto">
            <table className="w-full text-meta" data-slot="cost-table">
              <caption className="sr-only">
                {title}, {rangeLabel}
              </caption>
              <thead>
                <tr className="text-left text-muted-foreground">
                  <th scope="col" className="py-1 pr-3 font-medium">
                    Day
                  </th>
                  {showCost && (
                    <th scope="col" className="py-1 pr-3 text-right font-medium">
                      Cost
                    </th>
                  )}
                  <th scope="col" className="py-1 pr-3 text-right font-medium">
                    Tokens
                  </th>
                  <th scope="col" className="py-1 font-medium">
                    Basis
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <tr key={row.day} className="border-t">
                    <th scope="row" className="py-1 pr-3 text-left font-normal">
                      {row.day}
                    </th>
                    {showCost && (
                      <td className="py-1 pr-3 text-right tabular-nums">{row.cost}</td>
                    )}
                    <td className="py-1 pr-3 text-right tabular-nums">{row.tokens}</td>
                    <td className="py-1 text-muted-foreground">{row.basis}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <>
            <div
              className={cn(
                "flex items-end",
                bars.length > LABELLED_DAYS ? "gap-0.5" : "gap-2",
              )}
              style={{ height: CHART_HEIGHT + LABEL_ROOM }}
              data-slot="cost-chart"
            >
              {bars.map((bar) => (
                <div
                  key={bar.day}
                  role="img"
                  aria-label={ariaOf(bar, showCost)}
                  data-estimate={isEstimate(bar) || undefined}
                  className="flex min-w-0 flex-1 flex-col items-center justify-end gap-1"
                >
                  {labelled && (
                    <span className="text-[11px] whitespace-nowrap text-muted-foreground tabular-nums @max-[520px]:text-[11px]">
                      {valueOf(bar, showCost)}
                    </span>
                  )}
                  <div
                    className="relative flex w-full max-w-10 flex-col-reverse overflow-hidden rounded-t-sm"
                    style={{
                      height: Math.max(bar.height * CHART_HEIGHT, bar.usd > 0 ? 2 : 0),
                    }}
                  >
                    {bar.segments.map((segment) => (
                      <i
                        key={segment.family}
                        className={FAMILY_CLASS[segment.family]}
                        style={{ flex: `${Math.max(segment.usd, 0.0001)} 0 0` }}
                      />
                    ))}
                    {bar.hatched > 0 && (
                      <span
                        aria-hidden="true"
                        className="absolute inset-x-0 bottom-0"
                        style={{
                          height: `${bar.hatched * 100}%`,
                          backgroundImage: HATCH,
                        }}
                      />
                    )}
                  </div>
                </div>
              ))}
            </div>
            <div
              className={cn("flex", bars.length > LABELLED_DAYS ? "gap-0.5" : "gap-2")}
              aria-hidden="true"
            >
              {bars.map((bar, index) => (
                <div
                  key={bar.day}
                  className="min-w-0 flex-1 text-center text-[11px] leading-tight text-muted-foreground"
                >
                  {bars.length <= LABELLED_DAYS || index % 5 === 0 ? (
                    <>
                      <b className="block font-medium text-foreground">{bar.weekday}</b>
                      {bar.monthDay}
                      {bar.soFar && <span className="block">so far</span>}
                    </>
                  ) : null}
                </div>
              ))}
            </div>
          </>
        )}
        <ul
          className="flex flex-wrap gap-x-4 gap-y-1 text-meta text-muted-foreground"
          aria-label="Legend"
        >
          {families.map((family) => (
            <li key={family} className="flex items-center gap-1.5">
              <i className={cn("size-2.5 rounded-sm", FAMILY_CLASS[family])} />
              {family}
            </li>
          ))}
          <li className="flex items-center gap-1.5">
            <i
              className="size-2.5 rounded-sm border"
              style={{
                backgroundImage:
                  "repeating-linear-gradient(45deg, var(--muted-foreground) 0 2px, transparent 2px 5px)",
              }}
            />
            Hatched = from transcripts, cost estimated from list prices
          </li>
        </ul>
      </div>
    </Panel>
  );
}

export function CostByDaySkeleton() {
  return (
    <Panel title="Cost by day">
      <div
        className="flex h-44 items-end gap-2 p-4"
        aria-hidden="true"
        data-slot="cost-chart-skeleton"
      >
        {[40, 70, 30, 55, 90, 60, 35].map((height, index) => (
          <Skeleton key={index} className="flex-1" style={{ height: `${height}%` }} />
        ))}
      </div>
    </Panel>
  );
}
