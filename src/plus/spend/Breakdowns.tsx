import { useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { Badge, SummaryData } from "./data";
import {
  breakdown,
  modelFamily,
  TASK_BADGE,
  type BreakdownKind,
  type BreakdownRow,
  type Filter,
  type ModelFamily,
} from "./model";
import { Panel } from "./Panel";

const TITLE: Record<BreakdownKind, string> = {
  project: "By project folder",
  model: "By model",
  task: "By task",
};

const TAB: Record<BreakdownKind, string> = {
  project: "Project",
  model: "Model",
  task: "Task",
};

const FAMILY_BAR: Record<ModelFamily, string> = {
  Opus: "bg-primary",
  Sonnet: "bg-warning",
  Haiku: "bg-success",
  Other: "bg-muted-foreground",
};

const FOOTER: Record<BreakdownKind, ReactNode> = {
  project:
    "Grouped by the folder Claude Code ran in. Worktrees of one repo count together.",
  model: (
    <>
      Requests counted from <code className="font-mono">api_request</code> events.
    </>
  ),
  task: (
    <>
      <b className="text-foreground">Estimate:</b> a session is matched to a task run by
      session id and start time. Scheduled runs come from Tasks; everything else is
      interactive.
    </>
  ),
};

function barClass(kind: BreakdownKind, row: BreakdownRow): string {
  if (kind === "model") return FAMILY_BAR[modelFamily(row.key)];
  return "bg-primary";
}

const isMono = (kind: BreakdownKind, row: BreakdownRow) =>
  !row.rollup &&
  (kind === "project"
    ? row.filterable
    : kind === "task" && row.key !== "Interactive" && row.key !== "Unattributed");

export interface BreakdownCardProps {
  kind: BreakdownKind;
  summary: SummaryData;
  filter: Filter;
  badge: Badge;
  showCost: boolean;
  /** The task split cannot be told without OTel session ids: the card explains and offers setup. */
  locked?: boolean;
  onPick: (kind: BreakdownKind, key: string) => void;
  onSetup?: () => void;
  className?: string;
}

/** One breakdown. A row is a toggle button: Enter or a click narrows the whole page to it, the
 * same again clears it. */
export function BreakdownCard({
  kind,
  summary,
  filter,
  badge,
  showCost,
  locked,
  onPick,
  onSetup,
  className,
}: BreakdownCardProps) {
  const source =
    kind === "project"
      ? summary.byProject
      : kind === "model"
        ? summary.byModel
        : summary.byTask;
  const rows = breakdown(kind, source, { showCost });

  if (locked)
    return (
      <Panel
        title={TITLE[kind]}
        className={className}
        footer="Scheduled runs come from Tasks; everything else is interactive."
      >
        <div className="flex flex-col items-start gap-2 p-4" data-slot="task-locked">
          <b>Per task needs collecting</b>
          <span className="text-meta text-muted-foreground">
            Tasks are matched to spend by Claude Code's session id, which only the OTel
            events carry. Transcripts do not say which task started a session.
          </span>
          <Button variant="outline" size="sm" onClick={onSetup}>
            Set up collecting…
          </Button>
        </div>
      </Panel>
    );

  return (
    <Panel
      title={TITLE[kind]}
      badge={kind === "task" ? TASK_BADGE : badge}
      footer={FOOTER[kind]}
      className={className}
    >
      {rows.length === 0 ? (
        <p className="p-4 text-meta text-muted-foreground">Nothing in this range.</p>
      ) : (
        <ul className="divide-y">
          {rows.map((row) => {
            const active = filter[kind] === row.key;
            return (
              <li key={row.key}>
                <button
                  type="button"
                  aria-pressed={row.filterable ? active : undefined}
                  disabled={!row.filterable}
                  onClick={() => onPick(kind, row.key)}
                  title={row.label}
                  className={cn(
                    "grid w-full grid-cols-[minmax(0,1fr)_auto] items-baseline gap-x-3 gap-y-0.5 px-4 py-2.5 text-left focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring focus-visible:outline-none",
                    row.filterable && "hover:bg-muted/50",
                    active && "bg-muted",
                  )}
                >
                  <span
                    className={cn(
                      "truncate text-sm",
                      isMono(kind, row) && "font-mono text-xs",
                    )}
                  >
                    {row.label}
                  </span>
                  <span className="text-sm font-semibold tabular-nums">{row.cost}</span>
                  <span className="truncate text-meta text-muted-foreground">
                    {row.sub}
                  </span>
                  <span className="text-right text-meta text-muted-foreground tabular-nums">
                    {kind === "project" ? "" : row.share}
                  </span>
                  <span className="col-span-2 mt-1 h-1 overflow-hidden rounded-full bg-muted">
                    <i
                      className={cn("block h-full rounded-full", barClass(kind, row))}
                      style={{
                        width: `${Math.max(row.fraction * 100, row.fraction > 0 ? 2 : 0)}%`,
                      }}
                    />
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </Panel>
  );
}

export interface BreakdownsProps extends Omit<
  BreakdownCardProps,
  "kind" | "badge" | "locked"
> {
  kinds: BreakdownKind[];
  badge: Badge;
  taskLocked: boolean;
  /** One column of cards beside a wider card, instead of up to three side by side. */
  stacked?: boolean;
  className?: string;
}

/** The cards side by side on a wide page; tabs with one card open on a narrow one. Both use the
 * same cards, so a narrow page changes only what is shown. */
export function Breakdowns({
  kinds,
  taskLocked,
  stacked,
  className,
  ...card
}: BreakdownsProps) {
  const [tab, setTab] = useState<BreakdownKind>("project");
  const tabs = useRef<HTMLDivElement>(null);
  const open = kinds.includes(tab) ? tab : kinds[0];

  function move(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const last = kinds.length - 1;
    const next =
      event.key === "ArrowRight"
        ? (index + 1) % kinds.length
        : event.key === "ArrowLeft"
          ? (index + last) % kinds.length
          : null;
    if (next === null) return;
    event.preventDefault();
    setTab(kinds[next]);
    tabs.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  }

  if (kinds.length === 0) return null;
  return (
    <div className={cn("flex flex-col", className)} data-slot="breakdowns">
      <div
        ref={tabs}
        role="tablist"
        aria-label="Break down by"
        className="flex gap-1 border-b @min-[680px]:hidden"
      >
        {kinds.map((kind, index) => (
          <button
            key={kind}
            type="button"
            role="tab"
            id={`spend-tab-${kind}`}
            aria-selected={open === kind}
            aria-controls={`spend-panel-${kind}`}
            tabIndex={open === kind ? 0 : -1}
            onClick={() => setTab(kind)}
            onKeyDown={(event) => move(event, index)}
            className="shrink-0 rounded-t-md px-3 py-1.5 text-sm font-medium text-muted-foreground hover:bg-muted aria-selected:bg-muted aria-selected:text-foreground"
          >
            {TAB[kind]}
          </button>
        ))}
      </div>
      <div
        className={cn(
          "grid gap-4",
          !stacked && kinds.length >= 3
            ? "@min-[680px]:grid-cols-2 @min-[780px]:grid-cols-3"
            : !stacked && kinds.length === 2
              ? "@min-[680px]:grid-cols-2"
              : "grid-cols-1",
        )}
      >
        {kinds.map((kind) => (
          <div
            key={kind}
            id={`spend-panel-${kind}`}
            role="tabpanel"
            aria-labelledby={`spend-tab-${kind}`}
            className={cn("min-w-0", open !== kind && "hidden @min-[680px]:block")}
          >
            <BreakdownCard
              {...card}
              kind={kind}
              locked={kind === "task" && taskLocked}
              className="h-full"
            />
          </div>
        ))}
      </div>
    </div>
  );
}
