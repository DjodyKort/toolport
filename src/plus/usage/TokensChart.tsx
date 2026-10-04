import { useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { axisOf, compact, exact, tokensOf, type DayRow } from "./model";
import { Cell, DataTable, Muted } from "./atoms";

const W = 640;
const H = 230;
const LEFT = 58;
const RIGHT = 8;
const TOP = 10;
const BOTTOM = 46;

/** Tokens per day as bars from zero, with both axes titled, a sentence that says what the
 * picture says and the same numbers as a table. A single series: nothing is told apart by
 * colour. */
export function TokensChart({ rows, period }: { rows: DayRow[]; period: number }) {
  const [asTable, setAsTable] = useState(false);
  const tableId = useId();
  const totals = rows.map(tokensOf);
  const total = totals.reduce((a, b) => a + b, 0);
  if (rows.length === 0 || total === 0) {
    return (
      <Muted>
        No messages in the last {period} days. Pick a longer period, or Refresh to read
        newer transcripts.
      </Muted>
    );
  }
  const peakIndex = totals.indexOf(Math.max(...totals));
  const peak = rows[peakIndex];
  const { top, ticks } = axisOf(totals[peakIndex]);
  const plotW = W - LEFT - RIGHT;
  const plotH = H - TOP - BOTTOM;
  const step = plotW / rows.length;
  const barW = Math.max(2, step * 0.68);
  const every = Math.max(1, Math.ceil(34 / step));
  const y = (value: number) => TOP + plotH - (value / top) * plotH;
  const summary = `Tokens per day, ${rows[0].day} to ${rows[rows.length - 1].day}. Total ${exact(total)} tokens. Busiest day ${peak.day} with ${exact(totals[peakIndex])}.`;
  return (
    <div className="flex flex-col gap-2">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="img"
        aria-label={summary}
        className="h-auto w-full text-muted-foreground"
      >
        {ticks.map((tick) => (
          <g key={tick.value}>
            <line
              x1={LEFT}
              x2={W - RIGHT}
              y1={y(tick.value)}
              y2={y(tick.value)}
              className="stroke-border"
            />
            <text
              x={LEFT - 6}
              y={y(tick.value) + 3}
              textAnchor="end"
              fontSize="10"
              fill="currentColor"
            >
              {tick.label}
            </text>
          </g>
        ))}
        {rows.map((row, i) => {
          const value = totals[i];
          const height = (value / top) * plotH;
          const x = LEFT + i * step + (step - barW) / 2;
          return (
            <g key={row.day}>
              <rect
                x={x}
                y={TOP + plotH - height}
                width={barW}
                height={height}
                rx="2"
                className="fill-primary"
              >
                <title>{`${row.day}: ${exact(value)} tokens, ${exact(row.messages)} messages`}</title>
              </rect>
              {i % every === 0 && (
                <text
                  x={x + barW / 2}
                  y={TOP + plotH + 14}
                  textAnchor="middle"
                  fontSize="10"
                  fill="currentColor"
                >
                  {row.day.slice(5)}
                </text>
              )}
            </g>
          );
        })}
        <text
          x={LEFT + plotW / 2}
          y={H - 6}
          textAnchor="middle"
          fontSize="11"
          fill="currentColor"
        >
          Day (UTC)
        </text>
        <text
          x={12}
          y={TOP + plotH / 2}
          textAnchor="middle"
          fontSize="11"
          fill="currentColor"
          transform={`rotate(-90 12 ${TOP + plotH / 2})`}
        >
          Tokens
        </text>
      </svg>
      <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
        <span>
          Busiest day: {peak.day}, {compact(totals[peakIndex])} tokens. Every bar counts
          input, output, cache write and cache read.
        </span>
        <Button
          size="xs"
          variant="outline"
          aria-expanded={asTable}
          aria-controls={tableId}
          onClick={() => setAsTable((open) => !open)}
        >
          {asTable ? "Hide the numbers" : "Show the numbers"}
        </Button>
      </div>
      {asTable && (
        <div id={tableId} className="max-h-72 overflow-y-auto">
          <DataTable
            caption="Tokens per day"
            columns={[
              { label: "Day" },
              { label: "Messages", numeric: true },
              { label: "Input", numeric: true },
              { label: "Output", numeric: true },
              { label: "Cache write", numeric: true },
              { label: "Cache read", numeric: true },
              { label: "Total", numeric: true },
            ]}
          >
            {rows.map((row, i) => (
              <tr key={row.day}>
                <Cell>{row.day}</Cell>
                <Cell numeric>{exact(row.messages)}</Cell>
                <Cell numeric>{exact(row.input)}</Cell>
                <Cell numeric>{exact(row.output)}</Cell>
                <Cell numeric>{exact(row.cacheCreation)}</Cell>
                <Cell numeric>{exact(row.cacheRead)}</Cell>
                <Cell numeric>{exact(totals[i])}</Cell>
              </tr>
            ))}
          </DataTable>
        </div>
      )}
    </div>
  );
}
