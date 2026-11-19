import { useState } from "react";
import { ChevronRight, Check, AlertTriangle, X } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import type { LoadItem, Verified } from "./data";
import {
  layerOf,
  layerSums,
  markOf,
  shortPath,
  sourceLabel,
  tokens,
  type Lever,
  type RowMark,
} from "./model";

const MARK: Record<
  Exclude<RowMark, null>,
  { label: string; icon: typeof Check; tone: string }
> = {
  loaded: { label: "Claude Code loaded it", icon: Check, tone: "text-success" },
  absent: {
    label: "Claude Code did not load it",
    icon: X,
    tone: "text-muted-foreground",
  },
  mismatch: { label: "Claude Code disagrees", icon: AlertTriangle, tone: "text-warning" },
};

export function Group({
  title,
  count,
  hint,
  action,
  children,
}: {
  title: string;
  count?: number;
  hint?: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section aria-label={title} className="flex flex-col gap-2">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h3 className="text-sm font-semibold">
          {title}
          {count !== undefined && (
            <span className="ml-1.5 text-meta font-normal text-muted-foreground">
              {count}
            </span>
          )}
          {hint && (
            <span className="ml-2 text-meta font-normal text-muted-foreground">
              {hint}
            </span>
          )}
        </h3>
        {action}
      </div>
      <div className="flex flex-col divide-y rounded-xl border bg-card">{children}</div>
    </section>
  );
}

export function RowShell({
  title,
  badge,
  subtitle,
  trailing,
  children,
}: {
  title: React.ReactNode;
  badge?: React.ReactNode;
  subtitle?: React.ReactNode;
  trailing?: React.ReactNode;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1 px-3 py-2.5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <div className="flex min-w-0 flex-[1_1_14rem] flex-col">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="text-sm font-medium break-words">{title}</span>
            {badge}
          </div>
          {subtitle && (
            <span className="text-meta break-words text-muted-foreground">
              {subtitle}
            </span>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-3">{trailing}</div>
      </div>
      {children}
    </div>
  );
}

export function LayerStrip({ items, cwd }: { items: LoadItem[]; cwd: string }) {
  return (
    <p className="flex flex-wrap items-center gap-x-1 gap-y-0.5 text-meta text-muted-foreground">
      {layerSums(items, cwd).map(([layer, sum], index) => (
        <span key={layer} className="inline-flex items-center gap-1">
          {index > 0 && <ChevronRight className="size-3" aria-hidden />}
          {layer}{" "}
          <span className="tabular-nums text-foreground">
            {sum > 0 ? tokens(sum) : "none"}
          </span>
        </span>
      ))}
    </p>
  );
}

export interface InstructionRowProps {
  item: LoadItem;
  cwd: string;
  home: string | null;
  verified: Verified | null | undefined;
  busy: boolean;
  onToggle: (item: LoadItem, on: boolean) => void;
}

/** One instruction file with its switch. On means it loads here; off leaves it out of this folder
 * only, through `claudeMdExcludes` in the folder's local settings. */
export function InstructionRow({
  item,
  cwd,
  home,
  verified,
  busy,
  onToggle,
}: InstructionRowProps) {
  const [open, setOpen] = useState(false);
  const layer = layerOf(item, cwd);
  const mark = markOf(item, verified);
  const lockedBySettings = item.leftOutBy === "settings";
  const reason = lockedBySettings
    ? "Left out by other settings: Toolport did not write it, so it cannot undo it"
    : item.leftOutBy === "toolport"
      ? "Left out here by Toolport"
      : item.reason;
  const MarkIcon = mark ? MARK[mark].icon : null;
  return (
    <RowShell
      title={item.name}
      badge={
        <>
          <Badge variant="outline">{layer}</Badge>
          {item.leftOutBy === "toolport" && (
            <Badge variant="partial">Left out here</Badge>
          )}
          {lockedBySettings && <Badge variant="off">Left out by settings</Badge>}
          {mark && MarkIcon && (
            <span
              className={`inline-flex items-center gap-1 text-meta ${MARK[mark].tone}`}
              title={MARK[mark].label}
            >
              <MarkIcon className="size-3.5" aria-hidden />
              <span className="sr-only">{MARK[mark].label}</span>
            </span>
          )}
        </>
      }
      subtitle={`${sourceLabel(item)} · ${shortPath(item.path ?? "", home)}`}
      trailing={
        <>
          <span
            className="text-sm tabular-nums"
            aria-label={`${item.tokens} tokens, estimate`}
          >
            ≈ {tokens(item.tokens)}
          </span>
          <Switch
            aria-label={`${item.name} (${layer}) loads here`}
            checked={item.loaded}
            disabled={busy || lockedBySettings || !item.path}
            onCheckedChange={(on) => onToggle(item, on)}
          />
        </>
      }
    >
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="w-fit text-meta text-muted-foreground underline-offset-2 hover:underline"
      >
        {open ? "Hide details" : "Details"}
      </button>
      {open && (
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-meta">
          <dt className="text-muted-foreground">Path</dt>
          <dd className="break-all font-mono">{item.path}</dd>
          <dt className="text-muted-foreground">Why</dt>
          <dd>{reason || "Loaded at the start of every session"}</dd>
          <dt className="text-muted-foreground">Leaving it out</dt>
          <dd>
            Writes <code>claudeMdExcludes</code> in{" "}
            <code>.claude/settings.local.json</code> of this folder, git-ignored. It stays
            in place everywhere else.
          </dd>
          {mark && (
            <>
              <dt className="text-muted-foreground">Claude Code says</dt>
              <dd>{MARK[mark].label}</dd>
            </>
          )}
        </dl>
      )}
    </RowShell>
  );
}

export function LeverRow({
  lever,
  rank,
  busy,
  home,
  onLeaveOut,
}: {
  lever: Lever;
  rank: number;
  busy: boolean;
  home: string | null;
  onLeaveOut: (item: LoadItem) => void;
}) {
  return (
    <RowShell
      title={
        <>
          <span className="mr-2 text-muted-foreground">{rank}</span>Leave out{" "}
          {lever.item.name}{" "}
          <span className="font-normal text-muted-foreground">
            ({shortPath(lever.item.path ?? "", home)})
          </span>
        </>
      }
      subtitle={`Loads ${tokens(lever.saving)} tokens (estimate) in every session in this folder. It stays in place everywhere else.`}
      trailing={
        <>
          <span className="text-sm font-medium tabular-nums">
            −{tokens(lever.saving)}
          </span>
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            aria-label={`Leave out ${lever.item.name} (${tokens(lever.saving)} tokens)`}
            onClick={() => onLeaveOut(lever.item)}
          >
            Leave out
          </Button>
        </>
      }
    />
  );
}
