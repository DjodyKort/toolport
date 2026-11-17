import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { FolderStatusData } from "./data";
import { tokens, type HeadlineState } from "./model";

const WHEN = { dateStyle: "medium", timeStyle: "short" } as const;

function when(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleString([], WHEN);
}

const BADGE: Record<
  HeadlineState["kind"],
  { variant: "warning" | "success" | "info"; text: string }
> = {
  estimate: { variant: "warning", text: "Estimate" },
  measured: { variant: "success", text: "Measured" },
  stale: { variant: "warning", text: "Measured, stale" },
  projected: { variant: "info", text: "Projected" },
};

const NOTE: Record<HeadlineState["kind"], string> = {
  estimate:
    "Counted from the instruction files with an offline tokenizer. Good for ordering, not for savings. Skills cost less than they look: Claude Code reads only their descriptions up front.",
  measured:
    "The input size of one real request in this folder, with the instruction files verified against Claude Code's own log.",
  stale:
    "Measured with another Claude Code version or other settings. The number is still served; measure again for a current one.",
  projected:
    "The last measured total minus the estimated saving of the changes made since. It applies from the next session.",
};

export interface HeadlineProps {
  state: HeadlineState;
  status: FolderStatusData;
  estimate: number;
  onMeasure: () => void;
}

/** One number and its basis, then the bar: the itemised part, and the hatched part nothing
 * itemises once a measurement exists. */
export default function Headline({ state, status, estimate, onMeasure }: HeadlineProps) {
  const badge = BADGE[state.kind];
  const measured = state.kind === "estimate" ? null : state.measured;
  const rest = state.kind === "estimate" ? 0 : state.notItemised;
  const itemised = measured ? Math.max(0, state.total - rest) : estimate;
  const share = state.total > 0 ? Math.round((itemised / state.total) * 100) : 100;
  return (
    <section
      aria-label="Loaded at the start of every session here"
      className="flex flex-col gap-3 rounded-xl border bg-card p-4"
    >
      <h2 className="text-meta font-semibold uppercase tracking-wide text-muted-foreground">
        Loaded at the start of every session here
      </h2>
      <div className="flex flex-wrap items-center gap-3">
        <p className="text-3xl font-semibold tabular-nums" data-testid="headline-total">
          {state.kind === "estimate" ? "≈ " : ""}
          {tokens(state.total)} <span className="text-lg font-normal">tokens</span>
        </p>
        <Badge variant={badge.variant}>{badge.text}</Badge>
        {state.kind === "projected" && (
          <Badge variant="outline">Applies from the next session</Badge>
        )}
        {measured && (
          <span className="text-meta text-muted-foreground">
            {when(measured.measuredAt)} · Claude Code {measured.claudeCodeVersion} ·{" "}
            {measured.model} · {measured.windowPercent}% of the window
          </span>
        )}
      </div>
      <p className="text-meta text-muted-foreground">{NOTE[state.kind]}</p>
      {measured && (
        <div className="flex flex-col gap-1">
          <div
            role="img"
            aria-label={`${tokens(itemised)} tokens itemised, ${tokens(rest)} not itemised`}
            className="flex h-3 overflow-hidden rounded-full bg-muted"
          >
            <div className="bg-primary" style={{ width: `${share}%` }} />
            <div
              className="flex-1"
              style={{
                backgroundImage:
                  "repeating-linear-gradient(45deg, var(--muted-foreground) 0 2px, transparent 2px 6px)",
                opacity: 0.45,
              }}
            />
          </div>
          <p className="text-meta text-muted-foreground">
            Itemised ≈ {tokens(itemised)} (estimate) · Not itemised {tokens(rest)}: Claude
            Code's own prompt and tools, tool definitions, the skill list, plugin parts
            and the error of the estimate.
          </p>
        </div>
      )}
      <div className="flex flex-wrap items-center gap-3">
        <Button onClick={onMeasure}>Measure for real…</Button>
        <p className="text-meta text-muted-foreground">
          One short Claude Code request in this folder, about a minute.
          {state.kind === "estimate"
            ? ` Uses roughly ${tokens(estimate)} input tokens of your plan.`
            : " A cached answer costs nothing."}
        </p>
      </div>
      {status.calibration && state.kind === "estimate" && (
        <p className="text-meta text-muted-foreground">
          Calibrated by the last measurement: ≈{" "}
          {tokens(status.calibration.calibratedTotal)} (factor {status.calibration.factor}
          ).
        </p>
      )}
    </section>
  );
}
