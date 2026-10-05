import { ArrowUpRight, EyeOff } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { commandLine } from "../allcommands/model";
import { TerminalCommand } from "../system/atoms";
import type { AttentionItem } from "../types/attention";
import { ageText, viewOfRoute, type Tone } from "./model";

const DOT: Record<Tone, string> = {
  bad: "bg-destructive",
  warn: "bg-warning",
  mut: "bg-muted-foreground/60",
};

/** One thing that wants a decision: what is wrong in plain words, where it comes from, how
 * long it has waited, the screen where it is fixed, the one action Toolport can run for it and
 * the way to hide it. */
export function Row({
  item,
  tone,
  now,
  busy,
  terminal,
  onOpen,
  onAction,
  onDismiss,
}: {
  item: AttentionItem;
  tone: Tone;
  now: number;
  busy: boolean;
  /** The exact command line when the action can only run in a terminal. */
  terminal: string[] | null;
  onOpen: (item: AttentionItem) => void;
  onAction: (item: AttentionItem) => void;
  onDismiss: (item: AttentionItem) => void;
}) {
  const age = ageText(item.since, now);
  const opens = viewOfRoute(item.target.route) !== null;
  const action = item.action;
  return (
    <li
      aria-label={item.title}
      data-level={item.level}
      className="flex flex-col gap-2 rounded-xl border bg-card px-3.5 py-3"
    >
      <div className="flex items-start gap-3">
        <span
          aria-hidden="true"
          className={`mt-1.5 size-2.5 shrink-0 rounded-full ${DOT[tone]}`}
        />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <b className="text-sm font-semibold break-words">{item.title}</b>
          <small className="text-xs break-words text-muted-foreground">
            {item.detail}
          </small>
          <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <Badge variant="secondary">{item.from}</Badge>
            {age && (
              <time dateTime={item.since} title={item.since}>
                {age}
              </time>
            )}
          </div>
        </div>
        <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
          {action && !terminal && (
            <Button
              size="sm"
              variant={item.level === "needs-you" ? "default" : "outline"}
              disabled={busy}
              aria-label={`${action.label}: ${item.title}`}
              onClick={() => onAction(item)}
            >
              {action.label}
            </Button>
          )}
          {opens && (
            <Button
              size="sm"
              variant="outline"
              aria-label={`Open ${item.title}`}
              onClick={() => onOpen(item)}
            >
              <ArrowUpRight /> Open
            </Button>
          )}
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            aria-label={`Dismiss ${item.title}`}
            onClick={() => onDismiss(item)}
          >
            <EyeOff /> Dismiss
          </Button>
        </div>
      </div>
      {terminal && <TerminalCommand line={commandLine(terminal)} note={action?.label} />}
    </li>
  );
}
