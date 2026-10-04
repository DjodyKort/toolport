import { useMemo, useState, type FormEvent } from "react";
import { BarChart3, Loader2, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import {
  ErrorState,
  ScreenSkeleton,
  errorText,
  outcomeOf,
  useCtlJob,
  useCtlQuery,
} from "../ui";
import { Section, Stat } from "./atoms";
import { OtelCard } from "./OtelCard";
import {
  CacheCard,
  FailuresCard,
  ModelsCard,
  ProjectsCard,
  ServersCard,
  SessionsCard,
  SourcesCard,
} from "./Tables";
import { TokensChart } from "./TokensChart";
import { useRegistryRows, useWrite } from "./useWrite";
import { WriteDialogs } from "./WriteDialogs";
import {
  DEFAULT_PERIOD,
  PERIODS,
  STATUS_ARGV,
  compact,
  exact,
  isEmptyIndex,
  latestTs,
  parseStatus,
  parseUsage,
  percent,
  receiverLabel,
  share,
  todayOf,
  tokensOf,
  usageArgv,
  windowOf,
  type OtelStatus,
  type Period,
  type UsageView,
} from "./model";

function OtelStat({ query }: { query: { status: string; data: OtelStatus | null } }) {
  if (query.data === null)
    return (
      <Stat
        label="OTel receiver"
        value={query.status === "error" ? "Unknown" : "Checking"}
        note={query.status === "error" ? "status unavailable" : undefined}
      />
    );
  const label = receiverLabel(query.data.receiver.state);
  return (
    <Stat
      label="OTel receiver"
      value={
        <span className="inline-flex items-center gap-2">
          <Badge variant={label.tone}>{label.label}</Badge>
        </span>
      }
      note={query.data.enabled ? query.data.endpoint : "not enabled"}
    />
  );
}

function Strip({
  view,
  period,
  today,
  status,
}: {
  view: UsageView;
  period: Period;
  today: string;
  status: { status: string; data: OtelStatus | null };
}) {
  const window = windowOf(view.days, today, period);
  const counts = window.reduce(
    (total, row) => ({
      messages: total.messages + row.messages,
      tokens: total.tokens + tokensOf(row),
      cacheRead: total.cacheRead + row.cacheRead,
    }),
    { messages: 0, tokens: 0, cacheRead: 0 },
  );
  const calls = view.servers.reduce((total, server) => total + server.calls, 0);
  return (
    <div
      role="group"
      aria-label="Usage summary"
      className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4"
    >
      <Stat
        label={`Tokens, last ${period} days`}
        value={compact(counts.tokens)}
        title={exact(counts.tokens)}
        note={`${exact(counts.messages)} messages`}
      />
      <Stat
        label="Cache read"
        value={percent(share(counts.cacheRead, counts.tokens))}
        note={`of tokens in the last ${period} days`}
      />
      <Stat
        label="MCP calls"
        value={exact(calls)}
        note={`${exact(view.servers.length)} servers, all indexed sessions`}
      />
      <OtelStat query={status} />
    </div>
  );
}

function PeriodPicker({
  period,
  onChange,
}: {
  period: Period;
  onChange: (next: Period) => void;
}) {
  return (
    <div role="group" aria-label="Period" className="flex gap-1">
      {PERIODS.map((days) => (
        <Button
          key={days}
          size="xs"
          variant={days === period ? "secondary" : "outline"}
          aria-pressed={days === period}
          onClick={() => onChange(days)}
        >
          {days} days
        </Button>
      ))}
    </div>
  );
}

/** The Usage tab of the Tokens screen: tokens and MCP calls from the Claude Code transcripts,
 * and the OTel receiver. It opens on the stored index (`usage --no-refresh`); Refresh re-indexes
 * (`usage`). `today` and `now` are the clock a test sets. */
export function UsageTab({
  onOpenCommands,
  today,
  now,
}: {
  onOpenCommands?: (group?: string) => void;
  today?: string;
  now?: () => Date;
}) {
  const clock = useMemo(() => now ?? (() => new Date()), [now]);
  const day = today ?? todayOf(clock());
  const [rootText, setRootText] = useState("");
  const [root, setRoot] = useState("");
  const [period, setPeriod] = useState<Period>(DEFAULT_PERIOD);
  const [refreshed, setRefreshed] = useState<{
    root: string;
    data: unknown;
    at: string;
  } | null>(null);
  const stored = useCtlQuery<unknown>(usageArgv({ refresh: false, root }));
  const statusQuery = useCtlQuery<unknown>(STATUS_ARGV);
  const rows = useRegistryRows();
  const job = useCtlJob();
  const write = useWrite(rows, () => {
    setRefreshed(null);
    stored.reload();
    statusQuery.reload();
  });

  const raw = refreshed && refreshed.root === root ? refreshed.data : stored.data;
  const view = useMemo(() => (raw === null ? null : parseUsage(raw)), [raw]);
  const status = useMemo(
    () => (statusQuery.data === null ? null : parseStatus(statusQuery.data)),
    [statusQuery.data],
  );
  const running = job.state.phase === "running";
  const outcome = outcomeOf(job.state);

  async function refresh() {
    const result = await job.start(usageArgv({ refresh: true, root }));
    const envelope = result?.envelope;
    if (envelope?.ok)
      setRefreshed({ root, data: envelope.data, at: clock().toISOString() });
  }

  function applyRoot(event: FormEvent) {
    event.preventDefault();
    setRefreshed(null);
    setRoot(rootText.trim());
  }

  const unreachable = errorText(stored.error).code === "bridge";
  const body = (() => {
    if (view === null) {
      if (stored.status === "error")
        return (
          <ErrorState
            error={stored.error}
            title={
              unreachable
                ? "Toolport could not run toolportctl"
                : "Couldn't read the usage index"
            }
            context="usage --no-refresh"
            onRetry={stored.reload}
          />
        );
      return <ScreenSkeleton rows={4} label="Loading usage" />;
    }
    if (isEmptyIndex(view))
      return (
        <EmptyState
          icon={<BarChart3 />}
          title="Nothing indexed yet"
          description="The index stays empty until it runs once. Refresh reads your Claude Code transcripts and builds it; after that this tab opens on the stored figures."
          action={
            <Button size="sm" disabled={running} onClick={() => void refresh()}>
              <RefreshCw /> Refresh
            </Button>
          }
          className="py-10"
        />
      );
    const newest = latestTs(view);
    const rowsInWindow = windowOf(view.days, day, period);
    return (
      <>
        {stored.status === "error" && (
          <ErrorState
            error={stored.error}
            title="Couldn't re-read the usage index, showing the last figures"
            context="usage --no-refresh"
            onRetry={stored.reload}
          />
        )}
        <Strip
          view={view}
          period={period}
          today={day}
          status={{ status: statusQuery.status, data: status }}
        />
        <Section
          title="Tokens per day"
          note={`The last ${period} days up to ${day}, UTC.`}
          action={<PeriodPicker period={period} onChange={setPeriod} />}
        >
          <TokensChart rows={rowsInWindow} period={period} />
        </Section>
        <div className="grid gap-4 lg:grid-cols-2">
          <ProjectsCard projects={view.projects} />
          <ServersCard servers={view.servers} />
        </div>
        <SessionsCard sessions={view.sessions} />
        <div className="grid gap-4 lg:grid-cols-2">
          <CacheCard totals={view.totals} />
          <ModelsCard models={view.models} />
        </div>
        <FailuresCard failures={view.failures} />
        <SourcesCard
          view={view}
          newest={newest}
          indexedAt={refreshed && refreshed.root === root ? refreshed.at : null}
        />
      </>
    );
  })();

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap items-end gap-3">
          <form onSubmit={applyRoot} className="flex flex-wrap items-end gap-2">
            <label className="flex flex-col gap-1 text-xs text-muted-foreground">
              Transcript folder (--root)
              <Input
                value={rootText}
                placeholder="~/.claude/projects"
                className="w-72 max-w-full"
                onChange={(event) => setRootText(event.target.value)}
              />
            </label>
            <Button
              type="submit"
              size="sm"
              variant="outline"
              disabled={rootText.trim() === root}
            >
              Use this folder
            </Button>
          </form>
          <Button
            size="sm"
            className={cn("ml-auto")}
            disabled={running}
            onClick={() => void refresh()}
          >
            {running ? <Loader2 className="animate-spin" /> : <RefreshCw />} Refresh
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          Opening this tab reads the stored index (<code>usage --no-refresh</code>), which
          is quick. Refresh re-indexes: it reads every new line in your transcripts (
          <code>usage</code>) and can take a while on a large history. Leave the folder
          empty for the default.
        </p>
        {running && (
          <div role="status" className="flex flex-wrap items-center gap-2 text-sm">
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
            {job.state.cancelling ? "Cancelling…" : "Re-indexing your transcripts…"}
            <Button size="xs" variant="outline" onClick={() => void job.cancel()}>
              Cancel
            </Button>
          </div>
        )}
        {outcome?.kind === "error" && (
          <ErrorState
            error={new Error(outcome.message)}
            title="Refresh failed"
            context="usage"
            onRetry={() => void refresh()}
          />
        )}
      </div>
      {body}
      <OtelCard
        status={statusQuery}
        usage={view}
        write={write}
        onOpenCommands={onOpenCommands}
      />
      <WriteDialogs write={write} />
    </div>
  );
}
