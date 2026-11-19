import { useCallback, useEffect, useState } from "react";
import { CircleAlert, Coins } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { SkeletonRows } from "@/components/ui/skeleton";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { openView } from "../shell/navigate";
import { now } from "../tasks/clock";
import { exportCsv } from "./commands";
import { CacheCard } from "./CacheCard";
import { CollectingCardView } from "./CollectingCard";
import { CostByDay, CostByDaySkeleton } from "./CostByDay";
import type { Badge, RangeId, SummaryData } from "./data";
import { McpCardView } from "./McpCard";
import {
  bars,
  DEFAULT_RANGE,
  isFiltered,
  NO_FILTER,
  RANGES,
  taskSplitAvailable,
  toggleFilter,
  type BreakdownKind,
  type Filter,
  type FilterKind,
} from "./model";
import { Breakdowns } from "./Breakdowns";
import { RangeTiles, RangeTilesSkeleton } from "./RangeTiles";
import { SessionList } from "./SessionList";
import { FilterBar, SpendHeader } from "./SpendHeader";
import { Callout, SpendStrip } from "./SpendStrip";
import { useSpend } from "./useSpend";

const KINDS: BreakdownKind[] = ["project", "model", "task"];
const TYPING =
  "input, textarea, select, [contenteditable='true'], [role='dialog'], [role='menu']";

export interface SpendPageProps {
  /** Opens the export dialog. Without it the CSV is read and its row count is announced. */
  onExport?: (range: RangeId, filter: Filter) => void;
  /** Changes when something was written elsewhere, so the page reads again. */
  version?: number;
  /** Opens the plan to turn collecting on. */
  onSetup?: () => void;
  /** Opens a plan that moves the receiver and the export endpoint to another port. */
  onUsePort?: (port: number, current: number) => void;
  onPrune?: () => void;
  onUndo?: () => void;
}

async function readCsv(range: RangeId) {
  try {
    const data = await exportCsv(range);
    toast.success(`CSV ready: ${data.rows} rows for ${range}`);
  } catch (failure) {
    toastError("Export failed", { description: errorText(failure) });
  }
}

function sourceBadges(summary: SummaryData, showCost: boolean) {
  const otel = summary.sources.otelSessions > 0;
  const counted: Badge = otel ? "Measured" : "Counted";
  const chart: Badge = otel ? "Measured" : showCost ? "Estimate" : "Counted";
  return { counted, chart };
}

/** The Spend page: what was used and where, the MCP part, and whether collecting is on. */
export default function SpendPage({
  onExport,
  version = 0,
  onSetup,
  onUsePort,
  onPrune,
  onUndo,
}: SpendPageProps = {}) {
  const [range, setRange] = useState<RangeId>(DEFAULT_RANGE);
  const [filter, setFilter] = useState<Filter>(NO_FILTER);
  const [table, setTable] = useState(false);
  const [how, setHow] = useState(false);
  const load = useSpend(range, filter, version);
  const { status, state, shown } = load;

  const doExport = useCallback(
    () => (onExport ? onExport(range, filter) : void readCsv(range)),
    [onExport, range, filter],
  );
  const pick = useCallback(
    (kind: BreakdownKind, key: string) => setFilter((f) => toggleFilter(f, kind, key)),
    [],
  );
  const clear = useCallback(
    (kind: FilterKind) => setFilter((f) => ({ ...f, [kind]: null })),
    [],
  );

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.defaultPrevented || event.metaKey || event.ctrlKey || event.altKey)
        return;
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest(TYPING)) return;
      if (document.querySelector("[role='dialog'], [role='menu']")) return;
      const key = event.key.toLowerCase();
      const step = RANGES.find((r) => r.key === key);
      if (step) setRange(step.id);
      else if (key === "t") setTable((on) => !on);
      else if (key === "e") doExport();
      else if (event.key === "Escape" && isFiltered(filter)) setFilter(NO_FILTER);
      else return;
      event.preventDefault();
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [doExport, filter]);

  if (load.error)
    return (
      <StatusCard
        variant="danger"
        icon={<CircleAlert />}
        title="Spend could not be read"
        detail={load.error}
        actions={<Button onClick={load.reload}>Retry</Button>}
      />
    );
  if (!status || !state) return <SkeletonRows label="Reading the collecting status" />;

  const at = now();
  const showCost = status.showCost;
  const report = shown && state !== "neverSetUp" ? shown : null;
  const chart = report ? bars(report, at, showCost) : [];
  const estimated = chart.filter((bar) => bar.usd > 0 && bar.hatched >= 0.999);
  const closedDays =
    estimated.length < chart.length ? estimated.map((bar) => bar.day) : [];
  const folders = (load.whole ?? shown)?.byProject.map((row) => row.key) ?? [];
  const filtered = isFiltered(filter);
  const kinds = KINDS.filter((kind) => filter[kind] === null);
  const badges = report ? sourceBadges(report, showCost) : null;
  const rangeLabel = RANGES.find((r) => r.id === range)?.label.toLowerCase() ?? range;

  const collecting = (
    <CollectingCardView
      status={status}
      now={at}
      closedDays={closedDays}
      onExport={doExport}
      onPrune={() => onPrune?.()}
      onUndo={() => onUndo?.()}
    />
  );
  const mcp = shown && (
    <McpCardView
      part={shown.mcpPart}
      requests={shown.totals.requests}
      onRetry={load.reload}
      onOpenActivity={() => openView("activity")}
      onOpenMeasurements={() => openView("context")}
    />
  );

  return (
    <div
      className="@container flex flex-col gap-4"
      data-state={state}
      aria-busy={load.refreshing || undefined}
    >
      <SpendHeader
        folders={folders}
        filter={filter}
        onFolder={(project) => setFilter((f) => ({ ...f, project }))}
        onExport={doExport}
        onPrune={() => onPrune?.()}
      />
      <SpendStrip
        state={state}
        status={status}
        summary={report}
        now={at}
        elapsedMs={load.elapsedMs}
        onCancel={load.cancel}
        onRetry={load.reload}
        onSetup={onSetup}
        onUsePort={onUsePort}
      />
      <FilterBar
        filter={filter}
        summary={load.summary}
        whole={load.whole}
        onClear={clear}
      />
      {load.summaryError && (
        <StatusCard
          variant="danger"
          icon={<CircleAlert />}
          title="The report could not be read"
          detail={load.summaryError}
          actions={<Button onClick={load.reload}>Retry</Button>}
        />
      )}

      {state === "neverSetUp" && (
        <>
          <EmptyState
            icon={<Coins />}
            title="Nothing to show yet"
            description={
              <>
                Toolport found no Claude Code transcripts in{" "}
                <code className="font-mono">~/.claude/projects</code> and collecting is
                off. Turn collecting on and the page fills as Claude Code runs: cost,
                tokens, project, model and task. Nothing is written until you have seen a
                plan.
              </>
            }
            action={
              <>
                <Button onClick={onSetup}>Set up collecting…</Button>
                <Button
                  variant="outline"
                  aria-expanded={how}
                  aria-controls="spend-how"
                  onClick={() => setHow((on) => !on)}
                >
                  How it works
                </Button>
              </>
            }
          />
          {how && (
            <div id="spend-how">
              <Callout>
                <b className="text-foreground">
                  Nothing is written until you have seen a plan.
                </b>{" "}
                Toolport starts a receiver inside the app that listens on 127.0.0.1 only.
                Claude Code is told to send its token counts and cost to it by five
                settings in <code className="font-mono">settings.json</code>, written with
                Toolport's settings writer with a backup, and every step can be undone.
                Prompts, tool arguments and results are never exported.
              </Callout>
            </div>
          )}
          {mcp}
          {collecting}
        </>
      )}

      {state !== "neverSetUp" && !report && !load.summaryError && !load.cancelled && (
        <>
          <RangeTilesSkeleton />
          <CostByDaySkeleton />
        </>
      )}
      {state !== "neverSetUp" && !report && load.cancelled && (
        <StatusCard
          variant="info"
          icon={<CircleAlert />}
          title="Reading the transcripts was stopped"
          detail="Nothing was lost: files read so far stay cached, so the next read is shorter."
          actions={<Button onClick={load.reload}>Read again</Button>}
        />
      )}

      {report && badges && (
        <>
          <RangeTiles
            summary={report}
            range={range}
            showCost={showCost}
            cacheBadge={badges.counted === "Measured" ? "Measured" : "Counted"}
            onRange={setRange}
          />
          <div className="grid gap-4 @min-[680px]:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
            <CostByDay
              bars={chart}
              showCost={showCost}
              table={table}
              badge={badges.chart}
              rangeLabel={rangeLabel}
              onTable={setTable}
              prices={report.prices}
            />
            <CacheCard summary={report} badge={badges.counted} />
          </div>
          {filtered && report.sessionList ? (
            <div className="grid gap-4 @min-[780px]:grid-cols-3">
              <SessionList
                summary={report}
                showCost={showCost}
                className="@min-[780px]:col-span-2"
              />
              <Breakdowns
                kinds={kinds}
                summary={report}
                filter={filter}
                badge={badges.counted}
                showCost={showCost}
                taskLocked={!taskSplitAvailable(report)}
                stacked
                onPick={pick}
                onSetup={onSetup}
              />
            </div>
          ) : (
            <Breakdowns
              kinds={kinds}
              summary={report}
              filter={filter}
              badge={badges.counted}
              showCost={showCost}
              taskLocked={!taskSplitAvailable(report)}
              onPick={pick}
              onSetup={onSetup}
            />
          )}
          {mcp}
          {collecting}
          {state === "offWithTranscripts" && (
            <Callout>
              <b className="text-foreground">
                Want exact cost, per-task and per-folder detail?
              </b>{" "}
              Set up collecting. Your existing transcript history stays as the backfill.{" "}
              <button
                type="button"
                className="text-primary underline-offset-4 hover:underline"
                onClick={onSetup}
              >
                Set up collecting…
              </button>
            </Callout>
          )}
        </>
      )}
    </div>
  );
}
