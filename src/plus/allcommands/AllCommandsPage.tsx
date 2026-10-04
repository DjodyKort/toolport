import { useMemo, useState } from "react";
import { Search, TerminalSquare } from "lucide-react";
import { cn } from "@/lib/utils";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { CommandRow, CommandsData } from "../bridge/data";
import { AsyncView, useCtlQuery } from "../ui";
import { TierBadge } from "./Badges";
import { CommandPanel } from "./CommandPanel";
import { commandRows, groupCounts, isTerminal, matchesQuery } from "./model";
import { RunTool } from "./RunTool";

const ALL = "__all__";

/** Every `toolportctl` command, generated from `toolportctl commands --json`: the safety net
 * for anything that has no screen of its own. */
export function AllCommandsPage({ initialGroup }: { initialGroup?: string }) {
  const query = useCtlQuery<CommandsData>(["commands"]);
  return (
    <AsyncView
      query={query}
      errorTitle="Could not load the command list"
      context="commands"
      isEmpty={(data) => commandRows(data).length === 0}
      empty={
        <EmptyState
          icon={<TerminalSquare />}
          title="No commands found"
          description="toolportctl returned an empty command list."
        />
      }
    >
      {(data) => <Browser data={data} initialGroup={initialGroup} />}
    </AsyncView>
  );
}

function Browser({ data, initialGroup }: { data: CommandsData; initialGroup?: string }) {
  const rows = useMemo(() => commandRows(data), [data]);
  const groups = useMemo(() => groupCounts(rows), [rows]);
  const [text, setText] = useState("");
  const [group, setGroup] = useState(
    initialGroup && groups.some((entry) => entry.group === initialGroup)
      ? initialGroup
      : ALL,
  );
  const [selected, setSelected] = useState<string | null>(null);
  const shown = useMemo(
    () =>
      rows.filter(
        (row) => (group === ALL || row.group === group) && matchesQuery(row, text),
      ),
    [rows, group, text],
  );
  const current = rows.find((row) => row.id === selected) ?? null;

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-56 flex-1">
          <Search
            className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            type="search"
            aria-label="Search commands"
            placeholder="Search by name, summary or flag"
            value={text}
            onChange={(event) => setText(event.target.value)}
            className="pl-8"
          />
        </div>
        <Select value={group} onValueChange={setGroup}>
          <SelectTrigger aria-label="Group" className="w-52">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>All groups ({rows.length})</SelectItem>
            {groups.map((entry) => (
              <SelectItem key={entry.group} value={entry.group}>
                {entry.group} ({entry.count})
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p role="status" className="text-xs text-muted-foreground tabular-nums">
          {shown.length} of {rows.length} commands
        </p>
      </div>

      <div className="grid gap-6 lg:grid-cols-[minmax(15rem,22rem)_minmax(0,1fr)]">
        <CommandList rows={shown} selected={selected} onSelect={setSelected} />
        <div className="min-w-0">
          {current ? (
            <CommandPanel key={current.id} row={current} />
          ) : (
            <EmptyState
              className="py-12"
              icon={<TerminalSquare />}
              title="Pick a command"
              description="Choose one on the left to fill in its input and run it, or to copy its command line."
            />
          )}
        </div>
      </div>

      <RunTool data={data} />
    </div>
  );
}

function CommandList({
  rows,
  selected,
  onSelect,
}: {
  rows: CommandRow[];
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  if (rows.length === 0) {
    return (
      <EmptyState
        className="py-12"
        icon={<Search />}
        title="No command matches"
        description="Try fewer words, or pick another group."
      />
    );
  }
  return (
    <ul
      aria-label="Commands"
      className="flex max-h-[32rem] flex-col gap-0.5 overflow-y-auto rounded-xl border p-1"
    >
      {rows.map((row) => (
        <li key={row.id}>
          <button
            type="button"
            onClick={() => onSelect(row.id)}
            aria-current={row.id === selected ? "true" : undefined}
            className={cn(
              "flex w-full items-center justify-between gap-2 rounded-lg px-2.5 py-1.5 text-left text-sm hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none",
              row.id === selected && "bg-muted",
            )}
          >
            <span className="min-w-0">
              <span className="block truncate font-mono text-xs font-medium">
                {row.id}
              </span>
              <span className="block truncate text-xs text-muted-foreground">
                {row.summary}
              </span>
            </span>
            <span className="flex shrink-0 items-center gap-1">
              {isTerminal(row) && (
                <TerminalSquare
                  className="size-3.5 text-muted-foreground"
                  aria-label="Needs a terminal"
                />
              )}
              <TierBadge tier={row.tier} />
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}
