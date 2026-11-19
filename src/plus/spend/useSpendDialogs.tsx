import { useState, type ReactNode } from "react";
import { toast } from "sonner";
import type { RangeId } from "./data";
import { count } from "./model";
import { ExportDialog } from "./ExportDialog";
import { PortDialog } from "./PortDialog";
import { PruneDialog } from "./PruneDialog";
import { SetupDialog } from "./SetupDialog";
import { UndoDialog } from "./UndoDialog";
import { WeeklyMeasureDialog } from "./WeeklyMeasureDialog";

type Open =
  | { kind: "setup" }
  | { kind: "undo" }
  | { kind: "weekly" }
  | { kind: "prune"; withTotals: boolean }
  | { kind: "export"; range: RangeId; filtered: boolean }
  | { kind: "port"; port: number; current: number };

export interface SpendDialogs {
  element: ReactNode;
  setup: () => void;
  undo: () => void;
  weeklyMeasure: () => void;
  prune: () => void;
  /** Prune with "Also delete the daily totals" already ticked. */
  deleteTotals: () => void;
  exportCsv: (range: RangeId, filtered?: boolean) => void;
  usePort: (port: number, current: number) => void;
}

/** The five Spend dialogs and the toast each one ends in. `onChanged` runs after anything was
 * written, so the page behind reads its status again. */
export function useSpendDialogs(onChanged: () => void): SpendDialogs {
  const [open, setOpen] = useState<Open | null>(null);
  const close = () => setOpen(null);
  const undo = () => setOpen({ kind: "undo" });

  let element: ReactNode = null;
  if (open?.kind === "setup")
    element = (
      <SetupDialog
        onClose={close}
        onApplied={(result) => {
          const written = result.profiles.reduce(
            (sum, p) => sum + (p.applied ? p.writes : 0),
            0,
          );
          if (result.weeklyMeasure.error)
            toast.warning("Collecting is on, but the weekly measure was not added", {
              description: result.weeklyMeasure.error,
              action: { label: "Undo setup…", onClick: undo },
            });
          else if (result.complete)
            toast.success("Collecting is on", {
              description: `${written} settings written. Restart Claude Code sessions that are already open; new sessions export, and the first events usually arrive within a minute.`,
              action: { label: "Undo setup…", onClick: undo },
            });
          else
            toast.warning("Collecting is on, but a key was kept", {
              description:
                "Claude Code still sends to the address you kept, so Toolport receives nothing and Spend stays on transcripts.",
              action: { label: "Undo setup…", onClick: undo },
            });
          close();
          onChanged();
        }}
      />
    );
  else if (open?.kind === "undo")
    element = (
      <UndoDialog
        onClose={close}
        onUndone={(result) => {
          toast.success("Collecting is off", {
            description: result.deleted
              ? `Deleted ${count(result.deleted.events)} events and the daily totals.`
              : "Claude Code's settings are restored and the collected data is kept.",
          });
          onChanged();
        }}
      />
    );
  else if (open?.kind === "weekly")
    element = (
      <WeeklyMeasureDialog
        onClose={close}
        onTurnedOn={(result) => {
          toast.success("The weekly measure is on", {
            description: `${result.weeklyMeasure.task} measures ${result.weeklyMeasure.folder ?? "the folder"} and runs on its own schedule.`,
          });
          close();
          onChanged();
        }}
      />
    );
  else if (open?.kind === "prune")
    element = (
      <PruneDialog
        withTotals={open.withTotals}
        onClose={close}
        onPruned={(result) => {
          toast.success(
            result.totalsDeleted
              ? "Pruned the events and deleted the daily totals"
              : `Pruned ${count(result.removedEvents)} events`,
          );
          close();
          onChanged();
        }}
      />
    );
  else if (open?.kind === "export")
    element = (
      <ExportDialog
        range={open.range}
        filtered={open.filtered}
        onClose={close}
        onExported={(result) => {
          toast.success(
            `Exported ${count(result.rows)} ${result.rows === 1 ? "row" : "rows"}`,
            {
              description: result.out,
              action: {
                label: "Copy path",
                onClick: () => void navigator.clipboard.writeText(result.out),
              },
            },
          );
          close();
        }}
      />
    );
  else if (open?.kind === "port")
    element = (
      <PortDialog
        suggested={open.port}
        current={open.current}
        onClose={close}
        onMoved={(port) => {
          toast.success(`The receiver listens on 127.0.0.1:${port}`, {
            description:
              "Claude Code sessions that are already open keep the old port until restarted.",
          });
          close();
          onChanged();
        }}
      />
    );

  return {
    element,
    setup: () => setOpen({ kind: "setup" }),
    undo,
    weeklyMeasure: () => setOpen({ kind: "weekly" }),
    prune: () => setOpen({ kind: "prune", withTotals: false }),
    deleteTotals: () => setOpen({ kind: "prune", withTotals: true }),
    exportCsv: (range, filtered = false) => setOpen({ kind: "export", range, filtered }),
    usePort: (port, current) => setOpen({ kind: "port", port, current }),
  };
}
