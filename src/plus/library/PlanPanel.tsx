import { useState } from "react";
import { ChevronDown, ChevronRight, History } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { useLoad } from "../servers/useLoad";
import { skillFiles } from "./commands";
import type { SyncData } from "./data";
import {
  bytes,
  fileBadge,
  GLYPH,
  groupPlan,
  OP_BADGE,
  plural,
  syncedLine,
  type HeldBack,
  type PlanRow,
} from "./model";

function GroupLabel({
  title,
  count,
  note,
}: {
  title: string;
  count: number;
  note?: string;
}) {
  return (
    <div className="flex items-center justify-between gap-2 border-b bg-muted/50 px-4 py-1.5 text-xs font-semibold tracking-wide text-muted-foreground uppercase">
      <span>
        {title} <span className="ml-1 font-normal normal-case">{count}</span>
      </span>
      {note && <span className="font-normal normal-case">{note}</span>}
    </div>
  );
}

function Files({ skill }: { skill: string }) {
  const files = useLoad(() => skillFiles(skill), skill);
  if (files.error && !files.data)
    return <p className="px-4 py-2 text-meta text-destructive">{files.error}</p>;
  if (!files.data)
    return (
      <p className="px-4 py-2 text-meta text-muted-foreground">Reading the files…</p>
    );
  const unchanged = files.data.files.filter((f) => f.status === "copied").length;
  return (
    <ul className="divide-y border-t bg-muted/20" aria-label={`Files of ${skill}`}>
      {files.data.files
        .filter((f) => f.status !== "copied")
        .map((file) => {
          const badge = fileBadge(file);
          return (
            <li
              key={file.path}
              className="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-1.5 pl-10"
            >
              <code className="min-w-40 flex-1 font-mono text-xs break-all">
                {file.path}
              </code>
              {file.reason && (
                <span className="text-meta text-muted-foreground">{file.reason}</span>
              )}
              <span className="text-meta text-muted-foreground">{bytes(file.size)}</span>
              <Badge variant={badge.variant}>{badge.label}</Badge>
            </li>
          );
        })}
      {unchanged > 0 && (
        <li className="px-4 py-1.5 pl-10 text-meta text-muted-foreground">
          {plural(unchanged, "unchanged file")} not shown
        </li>
      )}
    </ul>
  );
}

function Row({ row }: { row: PlanRow }) {
  const [open, setOpen] = useState(false);
  const Chevron = open ? ChevronDown : ChevronRight;
  return (
    <li className={cn(row.held && "shadow-[inset_3px_0_0_var(--warning)]")}>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-start gap-3 px-4 py-2.5 text-left hover:bg-muted/40"
      >
        <span aria-hidden className="w-3 pt-0.5 font-mono text-sm text-muted-foreground">
          {GLYPH[row.op] ?? "~"}
        </span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="text-sm font-medium">{row.skill}</span>
          <span className="text-meta text-muted-foreground">{row.summary}</span>
        </span>
        <Badge variant={row.held ? "warning" : "outline"}>
          {row.held ? OP_BADGE[row.op] : plural(row.files, "file")}
        </Badge>
        <Chevron aria-hidden className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
      </button>
      {open && <Files skill={row.skill} />}
    </li>
  );
}

function HookRow({ row, fromTap }: { row: PlanRow; fromTap: boolean }) {
  return (
    <li className="flex items-start gap-3 px-4 py-2.5 shadow-[inset_3px_0_0_var(--primary)]">
      <span aria-hidden className="w-3 pt-0.5 font-mono text-sm text-muted-foreground">
        +
      </span>
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="text-sm font-medium">
          {row.skill} · {row.changes[0].path.split("#")[1] ?? "hook"}
        </span>
        <span className="text-meta text-muted-foreground">{row.summary}</span>
      </span>
      <Badge variant={fromTap ? "warning" : "outline"}>
        {fromTap ? "From a tap" : "From your library"}
      </Badge>
    </li>
  );
}

export interface PlanPanelProps {
  sync: SyncData;
  held: HeldBack[];
  tapSkills: ReadonlySet<string>;
  canUndo: boolean;
  onUndo: () => void;
  onOpenSkill: (name: string) => void;
}

/** The plan on the page: what happens to each file, in groups, with the skills a lint error holds
 * back apart. The plan on screen is the confirmation for Apply. */
export default function PlanPanel({
  sync,
  held,
  tapSkills,
  canUndo,
  onUndo,
  onOpenSkill,
}: PlanPanelProps) {
  const groups = groupPlan(sync);
  const rows = new Set(sync.plan.changes.map((c) => c.skill)).size;
  if (rows === 0 && held.length === 0) return null;
  return (
    <section
      aria-label="Sync plan"
      className="overflow-hidden rounded-xl border bg-card"
      data-testid="sync-plan"
    >
      <div className="flex items-center justify-between gap-2 border-b px-4 py-2">
        <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
          {rows > 0 ? "Sync plan" : "Nothing to write"}
          {rows > 0 && <span className="ml-1 font-normal normal-case">{rows}</span>}
        </h2>
        {canUndo && (
          <Button variant="ghost" size="sm" onClick={onUndo}>
            <History /> Undo last sync
          </Button>
        )}
      </div>
      {groups.skills.length > 0 && (
        <>
          <GroupLabel title="Skills" count={groups.skills.length} />
          <ul className="divide-y">
            {groups.skills.map((row) => (
              <Row key={row.skill} row={row} />
            ))}
          </ul>
        </>
      )}
      {groups.rules.length > 0 && (
        <>
          <GroupLabel title="Rules" count={groups.rules.length} />
          <ul className="divide-y">
            {groups.rules.map((row) => (
              <Row key={row.skill} row={row} />
            ))}
          </ul>
        </>
      )}
      {groups.hooks.length > 0 && (
        <>
          <GroupLabel
            title="Also written to settings.json"
            count={groups.hooks.length}
            note="These entries run commands by themselves"
          />
          <ul className="divide-y">
            {groups.hooks.map((row) => (
              <HookRow key={row.skill} row={row} fromTap={tapSkills.has(row.skill)} />
            ))}
          </ul>
        </>
      )}
      {held.length > 0 && (
        <>
          <GroupLabel
            title="Held back"
            count={held.length}
            note="Not written until fixed"
          />
          <ul className="divide-y" aria-label="Held back">
            {held.map((item) => (
              <li
                key={item.skill}
                className="flex items-start gap-3 px-4 py-2.5 shadow-[inset_3px_0_0_var(--destructive)]"
              >
                <span
                  aria-hidden
                  className="w-3 pt-0.5 font-mono text-sm text-destructive"
                >
                  ×
                </span>
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="text-sm font-medium">{item.skill}</span>
                  {item.rows.map((row) => (
                    <span key={row.code} className="text-meta text-muted-foreground">
                      {row.message} Keeps its last good copy in Claude Code.
                    </span>
                  ))}
                </span>
                <Button
                  variant="outline"
                  size="xs"
                  onClick={() => onOpenSkill(item.skill)}
                >
                  Fix…
                </Button>
              </li>
            ))}
          </ul>
        </>
      )}
      <p className="border-t px-4 py-2 text-meta text-muted-foreground">
        {syncedLine(sync)}
      </p>
    </section>
  );
}
