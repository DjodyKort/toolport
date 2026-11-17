import { useState } from "react";
import { Brain } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import { useLoad } from "../servers/useLoad";
import { plural } from "../context/model";
import { clearSessions, memoryOverview, setMemory, undoSessions } from "./commands";
import type { OverviewRow } from "./data";
import { dayOf, setterOf, sizeOf } from "./format";
import { withUndo } from "./withUndo";

const FILTER_LABEL: Record<string, string> = {
  "memory-off": "Memory off",
  "has-local-file": "Has local file",
  expiring: "Sessions expiring",
  large: "Large",
};

export interface MemoryOverviewProps {
  /** Opens a repository in Context > This folder. */
  onOpen?: (root: string) => void;
}

/** Settings > Toolport+ > Memory & history: one row per repository with its auto memory,
 * instruction files and sessions. */
export default function MemoryOverview({ onOpen }: MemoryOverviewProps) {
  const [filter, setFilter] = useState<string | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [days, setDays] = useState(30);
  const overview = useLoad(() => memoryOverview(filter), filter ?? "all");

  if (overview.error && !overview.data) {
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<Brain />}
        title="Memory and history could not be read"
        detail={overview.error}
      />
    );
  }
  const data = overview.data;
  if (!data) return <p className="text-meta text-muted-foreground">Reading memory…</p>;

  const turnOff = async (row: OverviewRow) => {
    setBusy(true);
    try {
      const previous = row.memory.switch;
      await withUndo(
        `Auto memory is off for ${row.name}`,
        () => setMemory("off", { cwd: row.root }),
        () => setMemory(previous, { cwd: row.root }),
        overview.reload,
      );
    } finally {
      setBusy(false);
    }
  };

  const clearOld = async (row: OverviewRow) => {
    setBusy(true);
    try {
      const plan = await clearSessions(row.root, {
        olderThan: days,
        to: "trash",
        dryRun: true,
      });
      if (plan.sessions.length === 0) {
        toast.message(`No session of ${row.name} is older than ${days} days`);
        return;
      }
      if (!window.confirm(`${plan.plan.summary}? They wait in the trash for 30 days.`)) {
        return;
      }
      await withUndo(
        `Cleared ${plural(plan.sessions.length, "session")} of ${row.name}`,
        () => clearSessions(row.root, { olderThan: days, to: "trash" }),
        () => undoSessions(row.root),
        overview.reload,
        "Undo works for 30 days.",
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <p className="text-meta text-muted-foreground">
        {plural(data.totals.repositories, "repository", "repositories")} ·{" "}
        {plural(data.totals.memoryFiles, "memory file")} (
        {sizeOf(data.totals.memoryBytes)}) · {plural(data.totals.sessions, "session")} (
        {sizeOf(data.totals.sessionBytes)}). Claude Code keeps sessions{" "}
        {data.retention.days} days, one value for all repositories.
      </p>
      <div
        role="group"
        aria-label="Filter repositories"
        className="flex flex-wrap items-center gap-1"
      >
        <Button
          size="sm"
          variant="outline"
          aria-pressed={!filter}
          onClick={() => setFilter(undefined)}
        >
          All
        </Button>
        {data.filters.map((id) => (
          <Button
            key={id}
            size="sm"
            variant="outline"
            aria-pressed={filter === id}
            onClick={() => setFilter(id)}
          >
            {FILTER_LABEL[id] ?? id}
          </Button>
        ))}
        <label className="ml-auto flex items-center gap-2 text-meta">
          Clear sessions older than
          <input
            type="number"
            min={0}
            aria-label="Days for bulk clear"
            className="w-16 rounded-md border bg-background px-2 py-1"
            value={days}
            onChange={(event) => setDays(Math.max(0, Number(event.target.value) || 0))}
          />
          days
        </label>
      </div>
      <table className="w-full text-left text-sm">
        <thead className="text-meta text-muted-foreground">
          <tr>
            <th scope="col">Repository</th>
            <th scope="col">Auto memory</th>
            <th scope="col">Instruction files</th>
            <th scope="col">Sessions</th>
            <th scope="col">Size</th>
            <th scope="col">Last active</th>
            <th scope="col">
              <span className="sr-only">Actions</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {data.rows.map((row) => (
            <tr key={row.key} className="border-t align-top">
              <th scope="row" className="py-2 font-medium">
                {row.name}
              </th>
              <td>
                {row.memory.enabled ? "On" : "Off"}
                <span className="block text-meta text-muted-foreground">
                  {plural(row.memory.files, "file")} · set by {setterOf(row.memory.setBy)}
                </span>
              </td>
              <td>
                {plural(row.instructions.files, "file")} · ≈ {row.instructions.tokens}
                {row.instructions.flags.length > 0 && (
                  <span className="block text-meta text-muted-foreground">
                    {row.instructions.flags.join(", ")}
                  </span>
                )}
              </td>
              <td>
                {row.sessions.count}
                {row.sessions.expiringThisWeek > 0 && (
                  <span className="block text-meta text-warning">
                    {row.sessions.expiringThisWeek} expiring this week
                  </span>
                )}
              </td>
              <td className="tabular-nums">
                {sizeOf(row.memory.bytes + row.sessions.bytes)}
              </td>
              <td>{dayOf(row.lastActive)}</td>
              <td className="flex flex-wrap justify-end gap-1 py-2">
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy || !row.memory.enabled}
                  aria-label={`Turn off auto memory for ${row.name}`}
                  onClick={() => void turnOff(row)}
                >
                  Turn off
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy || row.sessions.count === 0}
                  aria-label={`Clear old sessions of ${row.name}`}
                  onClick={() => void clearOld(row)}
                >
                  Clear sessions
                </Button>
                {onOpen && (
                  <Button
                    size="sm"
                    variant="ghost"
                    aria-label={`Open ${row.name} in Context`}
                    onClick={() => onOpen(row.root)}
                  >
                    Open
                  </Button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {data.other.folders > 0 && (
        <p className="text-meta text-muted-foreground">
          Other folders: {plural(data.other.folders, "folder")} with{" "}
          {plural(data.other.sessions, "session")} ({sizeOf(data.other.sessionBytes)})
          that belong to no git repository.
        </p>
      )}
    </div>
  );
}
