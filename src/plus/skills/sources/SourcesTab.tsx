import { useMemo, useState } from "react";
import { FolderPlus, Layers, RefreshCw, WifiOff } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { EmptyState } from "@/components/ui/empty-state";
import type { SourceRow, SourcesLsData, SourcesRootLsData } from "../../bridge/data";
import { AsyncView } from "../../ui";
import { useRead, useRegistryRows, useWrite, type WriteControl } from "../hooks";
import { plural } from "../model";
import { Section, SourceBadge, Stat } from "../parts";
import { WriteDialogs } from "../WriteDialogs";
import { AddRootDialog, RootsCard } from "./RootsCard";
import { useOnline } from "./online";
import { SourceDetail } from "./SourceDetail";
import {
  basisText,
  countsText,
  itemTotal,
  needsLook,
  plain,
  STATE_LABEL,
  STATE_TONE,
  timeText,
  totalTokens,
  visibleTotals,
} from "./model";

const PLANNED_ID = "planned:github";

function SourceListRow({
  row,
  selected,
  onSelect,
}: {
  row: SourceRow;
  selected: boolean;
  onSelect: () => void;
}) {
  const behind = row.status.state === "behind" ? row.freshness?.behind : undefined;
  return (
    <li>
      <button
        type="button"
        aria-current={selected ? "true" : undefined}
        onClick={onSelect}
        className="flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left text-sm outline-none hover:bg-muted focus-visible:ring-1 focus-visible:ring-ring aria-[current=true]:border-primary aria-[current=true]:bg-muted"
      >
        <span className="flex min-w-0 flex-1 flex-col">
          <b className="truncate">{row.origin.name}</b>
          <small className="truncate text-muted-foreground">
            {countsText(row.counts)}
          </small>
        </span>
        <Badge variant={STATE_TONE[row.status.state]}>
          {STATE_LABEL[row.status.state]}
          {behind ? ` (${behind})` : ""}
        </Badge>
        <SourceBadge origin={row.origin} />
        {!row.writable && <Badge variant="outline">read-only</Badge>}
      </button>
    </li>
  );
}

function PlannedRow({ selected, onSelect }: { selected: boolean; onSelect: () => void }) {
  return (
    <li>
      <button
        type="button"
        aria-current={selected ? "true" : undefined}
        onClick={onSelect}
        className="flex w-full items-center gap-2 rounded-lg border border-dashed px-3 py-2 text-left text-sm outline-none hover:bg-muted focus-visible:ring-1 focus-visible:ring-ring aria-[current=true]:border-primary aria-[current=true]:bg-muted"
      >
        <span className="flex min-w-0 flex-1 flex-col">
          <b className="truncate">GitHub skills account</b>
          <small className="truncate text-muted-foreground">not connected</small>
        </span>
        <Badge variant="info">planned</Badge>
        <Badge variant="outline">remote-library</Badge>
      </button>
    </li>
  );
}

function PlannedDetail() {
  return (
    <section
      aria-label="Source GitHub skills account"
      className="flex flex-col gap-3 rounded-lg border bg-card p-4"
    >
      <h4 className="flex flex-wrap items-center gap-2 text-base font-semibold">
        GitHub skills account <Badge variant="info">planned</Badge>
      </h4>
      <p className="text-sm text-muted-foreground">
        Sign in with GitHub so a skills library follows you across machines. Today the
        library only uses your git credentials; this makes the connection visible and
        manageable. It becomes one of the source kinds in a later wave.
      </p>
      <div>
        <Button size="sm" variant="outline" disabled title="needs MIG-SRC-3">
          Sign in with GitHub
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        Sign-in needs MIG-SRC-3: nothing is read from or sent to GitHub here.
      </p>
    </section>
  );
}

function Strip({ rows }: { rows: SourceRow[] }) {
  const items = rows.reduce((sum, row) => sum + itemTotal(row.counts), 0);
  const seen = visibleTotals(rows);
  const look = needsLook(rows);
  const cost = totalTokens(rows);
  return (
    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      <Stat
        label="Sources"
        value={rows.length}
        note={`${plural(items, "item")} in all`}
      />
      <Stat
        label="Cost if all loaded"
        value={cost.value.toLocaleString("en")}
        note={`tokens, ${basisText(cost.basis)}`}
      />
      <Stat
        label="Visible to Claude"
        warn={seen.seen < seen.total}
        value={`${seen.seen} of ${seen.total}`}
        note={
          seen.seen < seen.total
            ? `${plural(seen.total - seen.seen, "skill")} slash-only; see the reasons on the Skills tab`
            : "every skill found"
        }
      />
      <Stat
        label="Needs a look"
        warn={look.length > 0}
        value={look.length}
        note={look.join(" · ") || "nothing"}
      />
    </div>
  );
}

function Body({
  query,
  rescan,
  selected,
  setSelected,
  write,
}: {
  query: ReturnType<typeof useRead<SourcesLsData>>;
  rescan: () => void;
  selected: string | null;
  setSelected: (id: string) => void;
  write: WriteControl;
}) {
  const online = useOnline();
  return (
    <AsyncView
      query={query}
      errorTitle="Couldn't list the sources"
      context="sources ls"
      isEmpty={(data) => data.sources.length === 0 && !data.partial}
      empty={
        <EmptyState
          icon={<Layers />}
          title="No sources found"
          description="Toolport looked in every place it knows and found no skills, commands, agents, rules or CLAUDE.md files. Add the folder that holds your repositories to scan it."
          action={
            <Button variant="outline" onClick={rescan}>
              <RefreshCw /> Rescan sources
            </Button>
          }
        />
      }
    >
      {(data) => {
        const rows = data.sources;
        const row =
          selected === PLANNED_ID
            ? null
            : (rows.find((r) => r.id === selected) ?? rows[0] ?? null);
        const planned = row === null;
        return (
          <>
            <p className="text-xs text-muted-foreground">
              Last scan{" "}
              <time dateTime={data.generatedAt}>{timeText(data.generatedAt)}</time> ·{" "}
              {data.partial
                ? `partial: ${plural(data.skipped.length, "detector")} stopped early`
                : "nothing skipped"}
              . Scans are read-only, bounded, and never walk worktrees.
            </p>
            {!online && (
              <Callout variant="info" role="status">
                <span className="flex items-center gap-2">
                  <WifiOff className="size-4 shrink-0" aria-hidden="true" />
                  You are offline. Scanning reads local files and git state only, so this
                  list is complete; “behind its remote” compares with the last fetch.
                </span>
              </Callout>
            )}
            {data.partial && (
              <Callout variant="warning" role="status">
                <p className="font-medium">
                  The scan stopped early, so some sources may be missing or short.
                </p>
                <ul aria-label="Skipped detectors" className="mt-1 list-disc pl-4">
                  {data.skipped.map((skip) => (
                    <li key={skip.detector}>
                      <code className="font-mono text-xs">{skip.detector}</code>:{" "}
                      {plain(skip.reason)}
                    </li>
                  ))}
                </ul>
                <Button size="sm" variant="outline" className="mt-2" onClick={rescan}>
                  <RefreshCw /> Rescan sources
                </Button>
              </Callout>
            )}
            <Strip rows={rows} />
            <div className="grid gap-4 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
              <ul
                aria-label="Sources"
                className="flex max-h-[32rem] flex-col gap-1 overflow-auto"
              >
                {rows.map((r) => (
                  <SourceListRow
                    key={r.id}
                    row={r}
                    selected={r === row}
                    onSelect={() => setSelected(r.id)}
                  />
                ))}
                {!rows.some((r) => r.detector === "remote-library") && (
                  <PlannedRow
                    selected={planned}
                    onSelect={() => setSelected(PLANNED_ID)}
                  />
                )}
              </ul>
              {row ? (
                <SourceDetail key={row.id} row={row} write={write} />
              ) : (
                <PlannedDetail />
              )}
            </div>
          </>
        );
      }}
    </AsyncView>
  );
}

/** Library > Sources: every place Toolport looks for skills, commands, agents, rules and
 * CLAUDE.md files, who owns each, what it costs and how it is found. Scans are read-only;
 * the only writes here change which folders are scanned, and each is previewed first. */
export function SourcesTab() {
  const rows = useRegistryRows();
  const [epoch, setEpoch] = useState(0);
  const [adding, setAdding] = useState(false);
  const [refresh, setRefresh] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const write = useWrite(rows, () => setEpoch((n) => n + 1));
  return (
    <div className="flex flex-col gap-6">
      <WriteDialogs write={write} />
      {adding && (
        <AddRootDialog
          onClose={() => setAdding(false)}
          onSubmit={(path) => {
            setAdding(false);
            write.begin({
              command: "sources root add",
              title: "Add a folder to scan",
              argv: ["sources", "root", "add", path],
              confirmLabel: "Add folder",
              phrase: path,
            });
          }}
        />
      )}
      <Panel
        key={epoch}
        refresh={refresh}
        setRefresh={setRefresh}
        selected={selected}
        setSelected={setSelected}
        write={write}
        onAdd={() => setAdding(true)}
      />
    </div>
  );
}

function Panel({
  refresh,
  setRefresh,
  selected,
  setSelected,
  write,
  onAdd,
}: {
  refresh: boolean;
  setRefresh: (value: boolean) => void;
  selected: string | null;
  setSelected: (id: string) => void;
  write: WriteControl;
  onAdd: () => void;
}) {
  const argv = useMemo(
    () => ["sources", "ls", ...(refresh ? ["--refresh"] : [])],
    [refresh],
  );
  const sources = useRead<SourcesLsData>(argv);
  const roots = useRead<SourcesRootLsData>(["sources", "root", "ls"]);
  const rescan = () => (refresh ? sources.reload() : setRefresh(true));
  return (
    <>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="max-w-prose text-sm text-muted-foreground">
          The places Toolport looks, who owns each, what each costs and how it is found.
          Read-only sources are shown and never edited.
        </p>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="outline"
            onClick={rescan}
            disabled={sources.status === "loading"}
          >
            <RefreshCw /> Rescan sources
          </Button>
          <Button disabled={write.busy} onClick={onAdd}>
            <FolderPlus /> Add folder to scan…
          </Button>
        </div>
      </div>
      <Section title="Sources" count={sources.data?.sources.length}>
        <Body
          query={sources}
          rescan={rescan}
          selected={selected}
          setSelected={setSelected}
          write={write}
        />
      </Section>
      <RootsCard query={roots} write={write} onAdd={onAdd} />
    </>
  );
}
