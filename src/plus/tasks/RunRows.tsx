import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { errorText } from "../settings/useCtl";
import { runLog } from "./commands";
import type { MissedRecord, RunTrigger, RunView } from "./data";
import {
  catchUpSentence,
  duration,
  failedStep,
  isLive,
  ms,
  runResult,
  slotTimes,
  stamp,
  stepOf,
  triggerLabel,
  type Tone,
} from "./model";
import { Chip, Mono, Quiet } from "./ui";

const TRIGGER_TONE: Record<RunTrigger, Tone> = {
  manual: "off",
  cli: "off",
  selfMcp: "info",
  schedule: "off",
  "catch-up": "warning",
  onAuthFailure: "warning",
};

function resultTone(run: RunView): Tone {
  if (run.status === "failed") return "danger";
  if (run.status === "waiting") return "warning";
  if (run.status === "ok") return "success";
  return "off";
}

function sentence(run: RunView, now: number): string {
  if (run.status === "failed") {
    const step = failedStep(run);
    const later = step && step.index < run.steps.length;
    return [run.error ?? "", later ? `Step ${step.index + 1} did not run.` : ""]
      .filter(Boolean)
      .join(". ")
      .replace(/\.\./g, ".");
  }
  if (run.status === "waiting") {
    const step = stepOf(run);
    const title = run.steps[(step ?? 1) - 1]?.title ?? "";
    return `Stopped at step ${step} of ${run.steps.length}: ${title.toLowerCase()}.`;
  }
  if (run.trigger === "catch-up") return catchUpSentence(run, now);
  if (run.trigger === "cli") return "You ran it from the terminal";
  if (run.trigger === "selfMcp") return "You ran it when asked";
  return `${run.steps.length} step${run.steps.length === 1 ? "" : "s"}`;
}

export interface RunRowProps {
  run: RunView;
  now: number;
  /** Shown in History, where rows of several tasks are mixed. */
  taskId?: string;
  onRunAgain?: () => void;
  onOpen?: () => void;
}

/** One run: when, who started it, how it ended, and a Log that opens the redacted steps in place. */
export function RunRow({ run, now, taskId, onRunAgain, onOpen }: RunRowProps) {
  const [open, setOpen] = useState(false);
  const started = ms(run.startedAt) ?? now;
  const live = isLive(run.status);
  return (
    <div className="grid gap-2 px-4 py-3" data-run={run.id}>
      <div className="flex flex-wrap items-start gap-x-3 gap-y-1">
        <span className="w-28 shrink-0 text-meta text-muted-foreground tabular-nums">
          {stamp(started, now)}
        </span>
        <span className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1">
          <Chip tone={TRIGGER_TONE[run.trigger]}>{triggerLabel(run.trigger)}</Chip>
          {taskId && <Mono>{taskId}</Mono>}
          <Chip tone={resultTone(run)}>{runResult(run)}</Chip>
        </span>
        <span className="flex shrink-0 items-center gap-2">
          {run.durationMs !== null && (
            <span className="text-meta text-muted-foreground tabular-nums">
              {duration(run.durationMs)}
            </span>
          )}
          {live && onOpen ? (
            <Button size="xs" variant="outline" onClick={onOpen}>
              Open
            </Button>
          ) : (
            <Button
              size="xs"
              variant="outline"
              aria-expanded={open}
              onClick={() => setOpen((value) => !value)}
            >
              Log
            </Button>
          )}
        </span>
      </div>
      <Quiet className="pl-0 sm:pl-[7.75rem]">{sentence(run, now)}</Quiet>
      {open && (
        <RunLog id={run.id} onRunAgain={onRunAgain} close={() => setOpen(false)} />
      )}
    </div>
  );
}

/** The steps of one run with their output, which the runner already redacted. */
export function RunLog({
  id,
  onRunAgain,
  close,
}: {
  id: string;
  onRunAgain?: () => void;
  close?: () => void;
}) {
  const [log, setLog] = useState<RunView | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    runLog(id).then(
      (data) => alive && setLog(data.run),
      (e: unknown) => alive && setError(errorText(e)),
    );
    return () => {
      alive = false;
    };
  }, [id]);
  if (error) return <Quiet role="alert">The log could not be read: {error}</Quiet>;
  if (!log) return <Quiet>Reading the log…</Quiet>;
  const text = log.steps
    .map((s) => `[${s.status}] ${s.title}${s.output ? `\n${s.output}` : ""}`)
    .join("\n\n");
  return (
    <div
      className="grid gap-3 rounded-lg border bg-muted/40 p-3"
      aria-label={`Log of run ${id}`}
    >
      {log.steps.map((step, i) => {
        const needsYou = step.type === "needs-you";
        const detail =
          step.status === "ok" && step.startedAt && step.endedAt
            ? `Done in ${duration((ms(step.endedAt) ?? 0) - (ms(step.startedAt) ?? 0))}`
            : step.status === "skipped"
              ? `Skipped because step ${failedStep(log)?.index ?? i} failed`
              : step.status === "failed"
                ? `Failed${step.startedAt && step.endedAt ? ` after ${duration((ms(step.endedAt) ?? 0) - (ms(step.startedAt) ?? 0))}` : ""}`
                : step.status;
        return (
          <div key={step.id} className="grid gap-1">
            <p className="flex flex-wrap items-center gap-2 text-sm">
              <span className="text-muted-foreground tabular-nums">{i + 1}</span>
              <Chip tone={needsYou ? "warning" : "off"}>
                {needsYou ? "Needs you" : "Auto"}
              </Chip>
              <span className="font-medium">{step.title}</span>
              <span className="text-meta text-muted-foreground">{detail}</span>
            </p>
            {step.output && (
              <pre className="max-h-64 overflow-auto rounded-md bg-background p-2 text-[12px] break-words whitespace-pre-wrap">
                {step.output}
              </pre>
            )}
          </div>
        );
      })}
      <div className="flex flex-wrap items-center gap-2">
        <Quiet className="flex-1">
          Secret values are replaced with ••• before a line is written.
        </Quiet>
        {onRunAgain && !isLive(log.status) && (
          <Button size="sm" variant="outline" onClick={onRunAgain}>
            Run again
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void navigator.clipboard?.writeText(text)}
        >
          Copy log
        </Button>
        {close && (
          <Button size="sm" variant="ghost" onClick={close}>
            Close
          </Button>
        )}
      </div>
    </div>
  );
}

/** A scheduled slot that started no run, dimmed, with no Log because no run exists. */
export function SkippedRow({
  record,
  now,
  taskId,
  onRunNow,
}: {
  record: MissedRecord;
  now: number;
  taskId?: string;
  onRunNow?: () => void;
}) {
  const due = ms(record.lastDue) ?? now;
  const { first } = slotTimes(record, now);
  const tooOld = record.outcome === "too-old";
  return (
    <div className="grid gap-2 px-4 py-3 opacity-70" data-skipped={record.task}>
      <div className="flex flex-wrap items-start gap-x-3 gap-y-1">
        <span className="w-28 shrink-0 text-meta text-muted-foreground tabular-nums">
          {stamp(due, now)}
        </span>
        <span className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1">
          <Chip>Skipped</Chip>
          {taskId && <Mono>{taskId}</Mono>}
          <span className="text-sm">
            {tooOld ? "Too old to run" : "Busy: slot not run"}
          </span>
        </span>
        {tooOld && onRunNow && (
          <Button size="xs" variant="outline" onClick={onRunNow}>
            Run now
          </Button>
        )}
      </div>
      <Quiet className="pl-0 sm:pl-[7.75rem]">
        {tooOld
          ? `${first} was more than 24 hours late when Toolport started. Recorded, not run.`
          : "A run was still live when this slot came due. Slots are not retried."}
      </Quiet>
    </div>
  );
}
