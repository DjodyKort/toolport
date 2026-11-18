import { useRef, type KeyboardEvent } from "react";
import { cn } from "@/lib/utils";
import { Skeleton } from "@/components/ui/skeleton";
import type { Badge, RangeId, SummaryData } from "./data";
import { freshShare, percent, RANGES, tiles } from "./model";
import { KindBadge } from "./KindBadge";

const TILE =
  "flex min-w-0 flex-col gap-1 rounded-xl border bg-card p-4 text-left focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";
const GRID = "grid grid-cols-2 gap-3 @min-[680px]:grid-cols-4";

export interface RangeTilesProps {
  summary: SummaryData;
  range: RangeId;
  showCost: boolean;
  /** The cache tile's kind: Measured with OTel events, Counted from transcripts alone. */
  cacheBadge: Badge;
  onRange: (range: RangeId) => void;
}

/** The three ranges are one radio group (arrow keys move, 1 2 3 pick); the fourth tile is
 * the cache share and is not a choice. */
export function RangeTiles({
  summary,
  range,
  showCost,
  cacheBadge,
  onRange,
}: RangeTilesProps) {
  const group = useRef<HTMLDivElement>(null);
  const list = tiles(summary, showCost);
  const label = RANGES.find((r) => r.id === range)?.label.toLowerCase() ?? range;

  function move(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const last = list.length - 1;
    const next =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? (index + 1) % list.length
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? (index + last) % list.length
          : null;
    if (next === null) return;
    event.preventDefault();
    onRange(list[next].id);
    group.current?.querySelectorAll<HTMLButtonElement>('[role="radio"]')[next]?.focus();
  }

  return (
    <div className={GRID} data-slot="range-tiles">
      <div role="radiogroup" aria-label="Range" ref={group} className="contents">
        {list.map((tile, index) => {
          const selected = tile.id === range;
          return (
            <button
              key={tile.id}
              type="button"
              role="radio"
              aria-checked={selected}
              tabIndex={selected ? 0 : -1}
              onClick={() => onRange(tile.id)}
              onKeyDown={(event) => move(event, index)}
              className={cn(
                TILE,
                "hover:bg-muted/50",
                selected && "border-primary shadow-[inset_0_0_0_1px_var(--primary)]",
              )}
            >
              <span className="text-meta font-medium text-muted-foreground">
                {tile.label}
              </span>
              <span className="flex flex-wrap items-center gap-2 text-2xl font-semibold tabular-nums @max-[680px]:text-[22px]">
                {tile.headline}
                {tile.badge === "Estimate" && <KindBadge kind="Estimate" />}
              </span>
              {tile.detail && (
                <span className="text-meta text-muted-foreground tabular-nums">
                  {tile.detail}
                </span>
              )}
              {tile.notes.map((note) => (
                <span key={note} className="text-meta text-muted-foreground">
                  {note}
                </span>
              ))}
            </button>
          );
        })}
      </div>
      <div className={TILE} data-slot="cache-tile">
        <span className="flex flex-wrap items-center gap-2 text-meta font-medium text-muted-foreground">
          Cache read share <KindBadge kind={cacheBadge} />
        </span>
        <span className="text-2xl font-semibold tabular-nums">
          {percent(summary.cacheShare.value)}
        </span>
        <span className="text-meta text-muted-foreground">
          of input-side tokens came from the prompt cache
        </span>
        <span className="text-meta text-muted-foreground">
          Fresh tokens: {freshShare(summary.cacheShare.value)} ({label})
        </span>
      </div>
    </div>
  );
}

/** The tiles while the first report is being computed. */
export function RangeTilesSkeleton() {
  return (
    <div className={GRID} aria-hidden="true" data-slot="range-tiles-skeleton">
      {RANGES.map((r) => (
        <div key={r.id} className={TILE}>
          <span className="text-meta font-medium text-muted-foreground">{r.label}</span>
          <Skeleton className="h-6 w-24" />
          <Skeleton className="h-3 w-32" />
        </div>
      ))}
      <div className={TILE}>
        <span className="text-meta font-medium text-muted-foreground">
          Cache read share
        </span>
        <Skeleton className="h-6 w-16" />
        <Skeleton className="h-3 w-32" />
      </div>
    </div>
  );
}
