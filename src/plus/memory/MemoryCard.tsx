import { useState } from "react";
import { Brain } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import { Switch } from "@/components/ui/switch";
import { useLoad } from "../servers/useLoad";
import { leaveOut } from "../context/commands";
import { Group, RowShell } from "../context/StackRows";
import { plural } from "../context/model";
import {
  memoryFiles,
  memoryStatus,
  restoreSessions,
  sessionsOf,
  setMemory,
  undoMemory,
  undoSessions,
} from "./commands";
import type { MemoryStatusData, SwitchPosition } from "./data";
import { dayOf, setterOf, sizeOf } from "./format";
import { ClearMemoryDialog, ClearSessionsDialog } from "./MemoryDialogs";
import { withUndo } from "./withUndo";

const TABS = [
  { id: "memory", label: "Auto memory" },
  { id: "instructions", label: "Instruction files" },
  { id: "sessions", label: "Sessions" },
] as const;
type TabId = (typeof TABS)[number]["id"];

const POSITIONS: Array<{ id: SwitchPosition; label: string }> = [
  { id: "default", label: "Default" },
  { id: "on", label: "On" },
  { id: "off", label: "Off" },
];

function Positions({
  label,
  value,
  disabled,
  onPick,
}: {
  label: string;
  value: SwitchPosition;
  disabled: boolean;
  onPick: (position: SwitchPosition) => void;
}) {
  return (
    <div role="group" aria-label={label} className="flex gap-1">
      {POSITIONS.map((position) => (
        <Button
          key={position.id}
          size="sm"
          variant="outline"
          disabled={disabled}
          aria-pressed={value === position.id}
          className="aria-pressed:bg-accent"
          onClick={() => onPick(position.id)}
        >
          {position.label}
        </Button>
      ))}
    </div>
  );
}

export interface MemoryCardProps {
  folder: string;
  /** Called after a change that other cards of the page show too. */
  onChanged?: () => void;
}

/** Context > This folder: auto memory, instruction files and sessions of the repository the
 * folder belongs to, three tabs of one card. */
export default function MemoryCard({ folder, onChanged }: MemoryCardProps) {
  const [tab, setTab] = useState<TabId>("memory");
  const [busy, setBusy] = useState(false);
  const [clearing, setClearing] = useState<"memory" | "sessions" | null>(null);
  const status = useLoad(() => memoryStatus(folder), folder);
  const files = useLoad(() => memoryFiles(folder), folder);
  const sessions = useLoad(() => sessionsOf(folder), folder);

  const reload = () => {
    status.reload();
    files.reload();
    sessions.reload();
    onChanged?.();
  };

  if (status.error && !status.data) {
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<Brain />}
        title="Memory and history could not be read"
        detail={status.error}
      />
    );
  }
  const data = status.data;
  if (!data) return <p className="text-meta text-muted-foreground">Reading memory…</p>;

  const name = data.repository.name;
  async function guarded(task: () => Promise<unknown>) {
    setBusy(true);
    try {
      await task();
    } finally {
      setBusy(false);
    }
  }

  const pick = (position: SwitchPosition, global = false) =>
    guarded(() => {
      const previous = global ? globalPosition(data) : data.state.switch;
      const target = global ? ({ global: true } as const) : { cwd: folder };
      return withUndo(
        global
          ? `The global default for auto memory is ${position}`
          : `Auto memory is ${position} for ${name}`,
        () => setMemory(position, target),
        () => setMemory(previous, target),
        reload,
      );
    });

  const toggleFile = (path: string, name: string, on: boolean) =>
    guarded(() =>
      withUndo(
        on ? `${name} loads here again` : `${name} is left out of this folder`,
        () => leaveOut(on ? "rm" : "add", { file: path }, folder),
        () => leaveOut(on ? "add" : "rm", { file: path }, folder),
        reload,
      ),
    );

  const offerUndo = (done: string, undo: () => Promise<unknown>) =>
    toast.success(done, {
      description: "Undo works for 30 days.",
      duration: 10_000,
      action: { label: "Undo", onClick: () => void undo().then(reload) },
    });

  const trashed = data.trash.filter((entry) => entry.kind === "memory");

  return (
    <section
      aria-label="Memory and history"
      className="flex flex-col gap-3 rounded-xl border bg-card p-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">Memory and history · {name}</h3>
        <div role="tablist" aria-label="Memory and history" className="flex gap-1">
          {TABS.map((item) => (
            <Button
              key={item.id}
              role="tab"
              size="sm"
              variant={tab === item.id ? "secondary" : "ghost"}
              aria-selected={tab === item.id}
              onClick={() => setTab(item.id)}
            >
              {item.label}
            </Button>
          ))}
        </div>
      </div>

      {tab === "memory" && (
        <div role="tabpanel" aria-label="Auto memory" className="flex flex-col gap-3">
          <RowShell
            title={`Auto memory is ${data.state.enabled ? "on" : "off"} for ${name}`}
            subtitle={`Set by ${setterOf(data.state.setBy)}. Worktrees of ${name} share it. ${
              data.state.locked
                ? "Your organization fixes this; it cannot be changed here."
                : ""
            }`}
            trailing={
              <Positions
                label="Auto memory switch"
                value={data.state.switch}
                disabled={busy || data.state.locked}
                onPick={(position) => void pick(position)}
              />
            }
          />
          <RowShell
            title="Global default"
            subtitle="Applies to every repository without a switch of its own."
            trailing={
              <Positions
                label="Global default switch"
                value={globalPosition(data)}
                disabled={busy}
                onPick={(position) => void pick(position, true)}
              />
            }
          />
          <Group
            title="Memory files"
            count={files.data?.files.length ?? data.memory.count}
            action={
              <Button
                size="sm"
                variant="outline"
                disabled={busy || data.memory.count === 0}
                onClick={() => setClearing("memory")}
              >
                Clear memory…
              </Button>
            }
          >
            {(files.data?.files ?? []).map((f) => (
              <RowShell
                key={f.path}
                title={f.name}
                subtitle={f.description ?? (f.index ? "The index Claude Code loads" : "")}
                trailing={
                  <span className="text-meta tabular-nums">
                    {sizeOf(f.bytes)} · {dayOf(f.modified)}
                  </span>
                }
              />
            ))}
            {data.memory.count === 0 && (
              <p className="px-3 py-2 text-meta text-muted-foreground">
                No memory is kept for this repository.
              </p>
            )}
          </Group>
          {trashed.length > 0 && (
            <div className="flex items-center justify-between text-meta">
              <span>Cleared memory waits in Toolport's trash for 30 days.</span>
              <Button
                size="sm"
                variant="outline"
                disabled={busy}
                onClick={() =>
                  void guarded(async () => {
                    await undoMemory(folder);
                    reload();
                  })
                }
              >
                Put memory back
              </Button>
            </div>
          )}
        </div>
      )}

      {tab === "instructions" && (
        <div
          role="tabpanel"
          aria-label="Instruction files"
          className="flex flex-col gap-1"
        >
          <p className="text-meta text-muted-foreground">
            In the order Claude Code loads them. About {data.instructions.totalTokens}{" "}
            tokens (estimate).
          </p>
          <div className="flex flex-col divide-y rounded-xl border bg-card">
            {data.instructions.rows.map((row) => (
              <RowShell
                key={`${row.scope}${row.path}`}
                title={row.name}
                subtitle={
                  row.canExclude
                    ? `${row.scope} · ${row.path ?? ""}`
                    : (row.note ?? "No setting leaves this file out")
                }
                trailing={
                  <>
                    <span className="text-sm tabular-nums">≈ {row.tokens}</span>
                    {row.canExclude && row.path && (
                      <Switch
                        aria-label={`${row.name} (${row.scope}) loads here`}
                        checked={row.state !== "excluded"}
                        disabled={busy}
                        onCheckedChange={(on) =>
                          void toggleFile(row.path ?? "", row.name, on)
                        }
                      />
                    )}
                  </>
                }
              />
            ))}
          </div>
        </div>
      )}

      {tab === "sessions" && (
        <div role="tabpanel" aria-label="Sessions" className="flex flex-col gap-3">
          {sessions.data ? (
            <>
              <RowShell
                title={`${plural(sessions.data.totals.count, "session")} · ${sizeOf(sessions.data.totals.bytes)}`}
                subtitle={`Claude Code deletes sessions after ${sessions.data.retention.days} days; one value for all repositories. There is no setting per repository.`}
                trailing={
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy || sessions.data.totals.count === 0}
                    onClick={() => setClearing("sessions")}
                  >
                    Clear sessions…
                  </Button>
                }
              />
              <Group title="Sessions" count={sessions.data.sessions.length}>
                {sessions.data.sessions.map((s) => (
                  <RowShell
                    key={s.id}
                    title={s.id}
                    subtitle={`${s.ageDays} days old · ${s.expiring ? `expires in ${s.daysLeft} days` : `${s.daysLeft} days left`}`}
                    trailing={<span className="text-meta">{sizeOf(s.bytes)}</span>}
                  />
                ))}
              </Group>
              {sessions.data.trash.length > 0 && (
                <div className="flex items-center justify-between text-meta">
                  <span>Cleared sessions wait in Toolport's trash for 30 days.</span>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy}
                    onClick={() =>
                      void guarded(async () => {
                        await undoSessions(folder);
                        reload();
                      })
                    }
                  >
                    Put sessions back
                  </Button>
                </div>
              )}
              {sessions.data.archive.map((entry) => (
                <div
                  key={entry.id}
                  className="flex items-center justify-between text-meta"
                >
                  <span>Archive {entry.id}</span>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy}
                    onClick={() =>
                      void guarded(async () => {
                        await restoreSessions(entry.id);
                        reload();
                      })
                    }
                  >
                    Restore archive {entry.id}
                  </Button>
                </div>
              ))}
            </>
          ) : (
            <p className="text-meta text-muted-foreground">Reading sessions…</p>
          )}
        </div>
      )}

      {clearing === "memory" && (
        <ClearMemoryDialog
          folder={folder}
          name={name}
          directory={data.directory.path}
          onClose={() => setClearing(null)}
          onDone={(done) => {
            reload();
            if (done.forever) {
              toast.success(`Deleted the auto memory of ${name} for good`);
              return;
            }
            offerUndo(`Cleared the auto memory of ${name}`, () => undoMemory(folder));
          }}
        />
      )}
      {clearing === "sessions" && (
        <ClearSessionsDialog
          folder={folder}
          name={name}
          onClose={() => setClearing(null)}
          onDone={(done) => {
            reload();
            offerUndo(
              `Cleared ${plural(done.sessions.length, "session")} of ${name}`,
              () => undoSessions(folder),
            );
          }}
        />
      )}
    </section>
  );
}

function globalPosition(data: MemoryStatusData): SwitchPosition {
  return data.state.global === null ? "default" : data.state.global ? "on" : "off";
}
