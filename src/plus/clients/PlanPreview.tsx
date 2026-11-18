import { TriangleAlert } from "lucide-react";
import { changes, type PlanState } from "./plan";

/** What a write will do, shown before it is made: the change to the client's file, the notes, the
 * trade-off and the hash the write is bound to. */
export function PlanPreview({ state }: { state: PlanState }) {
  if (state.loading) {
    return (
      <p className="text-meta text-muted-foreground" role="status">
        Planning the change…
      </p>
    );
  }
  if (state.error) {
    return (
      <p className="text-meta text-destructive" role="alert">
        {state.error}
      </p>
    );
  }
  const { plan } = state;
  if (!plan) return null;
  if (!changes(plan)) {
    return (
      <p className="text-meta text-muted-foreground" role="status">
        Nothing to change: the entry already stands the way Toolport would write it.
      </p>
    );
  }
  const edits = plan.plan.steps.filter((step) => step.diff);
  const notes = plan.plan.steps.filter((step) => !step.diff);
  return (
    <div className="flex flex-col gap-3 text-meta" data-plan-hash={plan.planHash}>
      {edits.map((step) => (
        <div key={step.detail} className="flex flex-col gap-1">
          <span className="font-medium text-foreground">
            Change to <span className="font-mono break-all">{step.path}</span>
          </span>
          <pre className="overflow-x-auto rounded-md bg-muted px-3 py-2 font-mono text-xs break-all whitespace-pre-wrap">
            {step.diff && step.diff.before !== "(absent)" && (
              <span className="block text-muted-foreground">
                {`- ${step.diff.before}`}
              </span>
            )}
            {step.diff && step.diff.after !== "(absent)" && (
              <span className="block">{step.diff.after}</span>
            )}
          </pre>
        </div>
      ))}
      <ul className="flex list-disc flex-col gap-1 pl-5 text-muted-foreground">
        {notes.map((step) => (
          <li key={step.detail}>{step.detail}</li>
        ))}
      </ul>
      {plan.plan.warnings.map((warning) => (
        <p key={warning} className="flex items-start gap-1.5 text-warning">
          <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span>{warning}</span>
        </p>
      ))}
      <p className="font-mono text-xs text-muted-foreground">Plan {plan.planHash}</p>
    </div>
  );
}
