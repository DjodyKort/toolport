import { type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Callout } from "@/components/Callout";
import { CopyButton } from "./CopyButton";
import {
  humanKey,
  planOf,
  resultOf,
  toShown,
  type PlanOp,
  type PlanStep,
  type PlanV1,
  type Shown,
  withoutResult,
} from "./plan";

const MARK: Record<PlanOp, { glyph: string; word: string; tone: string }> = {
  create: { glyph: "+", word: "Create", tone: "text-success" },
  merge: { glyph: "~", word: "Merge", tone: "text-warning" },
  update: { glyph: "~", word: "Update", tone: "text-warning" },
  delete: { glyph: "−", word: "Delete", tone: "text-destructive" },
  exec: { glyph: "▸", word: "Run", tone: "text-muted-foreground" },
  note: { glyph: "i", word: "Note", tone: "text-muted-foreground" },
};

function Step({ step }: { step: PlanStep }) {
  const mark = MARK[step.op];
  return (
    <li className="grid grid-cols-[1.25rem_minmax(0,1fr)] gap-2 px-3 py-1 text-sm">
      <span className={cn("text-center font-mono", mark.tone)} aria-hidden="true">
        {mark.glyph}
      </span>
      <div className="min-w-0">
        <span className="sr-only">{mark.word}: </span>
        {step.detail}
        {step.path && (
          <code className="ml-1.5 font-mono text-xs break-all text-muted-foreground">
            {step.path}
          </code>
        )}
        {step.keys && step.keys.length > 0 && (
          <span className="ml-1.5 inline-flex flex-wrap gap-1 align-middle">
            {step.keys.map((key) => (
              <code key={key} className="rounded bg-muted px-1.5 font-mono text-xs">
                {key}
              </code>
            ))}
          </span>
        )}
        {step.diff && (
          <details className="mt-1">
            <summary className="cursor-pointer text-xs text-muted-foreground">
              Show the change
            </summary>
            <div className="mt-1 grid gap-2 sm:grid-cols-2">
              <pre
                aria-label="Before"
                className="max-h-40 overflow-auto rounded-md bg-destructive/10 p-2 font-mono text-xs whitespace-pre-wrap"
              >
                {step.diff.before}
              </pre>
              <pre
                aria-label="After"
                className="max-h-40 overflow-auto rounded-md bg-success/10 p-2 font-mono text-xs whitespace-pre-wrap"
              >
                {step.diff.after}
              </pre>
            </div>
          </details>
        )}
      </div>
    </li>
  );
}

function UndoHint({ undo }: { undo: string }) {
  if (!undo) return null;
  return (
    <div className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
      <span>To undo:</span>
      <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs break-all">
        {undo}
      </code>
      <CopyButton text={undo} size="xs" />
    </div>
  );
}

function PlanView({ plan }: { plan: PlanV1 }) {
  const tokens = plan.effects?.tokens;
  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm">{plan.summary}</p>
      {plan.steps.length > 0 && (
        <ol aria-label="Changes" className="rounded-lg border bg-background py-1.5">
          {plan.steps.map((step, i) => (
            <Step key={i} step={step} />
          ))}
        </ol>
      )}
      {tokens && (
        <p className="text-sm text-muted-foreground">
          Tokens in a session: {tokens.before.toLocaleString("en")} to{" "}
          {tokens.after.toLocaleString("en")} ({tokens.basis})
        </p>
      )}
      {plan.warnings.length > 0 && (
        <Callout variant="warning" role="status">
          <ul className="list-disc pl-4">
            {plan.warnings.map((warning, i) => (
              <li key={i}>{warning}</li>
            ))}
          </ul>
        </Callout>
      )}
      <UndoHint undo={plan.undo} />
    </div>
  );
}

function ShownView({ shown }: { shown: Shown }): ReactNode {
  switch (shown.kind) {
    case "scalar":
      return <span className="break-words">{shown.text}</span>;
    case "list":
      return shown.items.length === 0 ? (
        <span className="text-muted-foreground">none</span>
      ) : (
        <span className="flex flex-wrap gap-1">
          {shown.items.map((item, i) => (
            <code key={i} className="rounded bg-muted px-1.5 font-mono text-xs break-all">
              {item}
            </code>
          ))}
        </span>
      );
    case "json":
      return (
        <pre className="max-h-64 overflow-auto rounded-md bg-muted p-2 font-mono text-xs whitespace-pre-wrap">
          {shown.text}
        </pre>
      );
    case "record":
      return (
        <dl className="grid grid-cols-[minmax(7rem,max-content)_minmax(0,1fr)] gap-x-3 gap-y-1.5 text-sm">
          {shown.entries.map(([key, child]) => (
            <div key={key} className="contents">
              <dt className="text-muted-foreground">{humanKey(key)}</dt>
              <dd className="min-w-0">
                <ShownView shown={child} />
              </dd>
            </div>
          ))}
        </dl>
      );
  }
}

/** Any command data as a readable list; a shape it cannot list is pretty JSON. */
export function DataView({ data, className }: { data: unknown; className?: string }) {
  const result = resultOf(data);
  const rest = result ? withoutResult(data) : data;
  const showRest = !result || Object.keys(rest as object).length > 0;
  return (
    <div className={cn("flex flex-col gap-3", className)}>
      {showRest && <ShownView shown={toShown(rest)} />}
      {result && (
        <div className="flex flex-col gap-2 text-sm">
          <ShownView
            shown={toShown({ changed: result.changed, backups: result.backups })}
          />
          <UndoHint undo={result.undo} />
        </div>
      )}
    </div>
  );
}

/** The dry run of a write as a plan: what changes, the warnings and the undo command. A
 * `PlanV1` renders as a list of steps with their diffs; the shapes of older commands render
 * as a readable list, and anything else as pretty JSON. */
export function PlanPreview({ data, className }: { data: unknown; className?: string }) {
  const plan = planOf(data);
  return (
    <section aria-label="Preview" className={cn("flex flex-col gap-2", className)}>
      {plan ? <PlanView plan={plan} /> : <DataView data={data} />}
    </section>
  );
}
