import { useRef, useState } from "react";
import { Play, Terminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import type { CommandRow } from "../bridge/data";
import type { SecretFieldHandle } from "../ui";
import { NeedBadges, TierBadge } from "./Badges";
import { CommandForm } from "./CommandForm";
import {
  commandLine,
  emptyValues,
  isTerminal,
  phraseFor,
  planRun,
  problems,
  type FormValues,
} from "./model";
import { CommandLine, RunFlowView } from "./RunFlow";
import { useRunFlow, type FlowSpec } from "./useRunFlow";

function sameInput(a: FormValues, b: FormValues): boolean {
  return JSON.stringify([a.operands, a.flags]) === JSON.stringify([b.operands, b.flags]);
}

/** One command: what it does, a form for its input, the exact command line and the run. */
export function CommandPanel({ row }: { row: CommandRow }) {
  const [values, setValues] = useState<FormValues>(emptyValues);
  const secret = useRef<SecretFieldHandle>(null);
  const flow = useRunFlow();

  const plan = planRun(row, values);
  const issues = problems(row, values);
  const terminal = isTerminal(row);
  const line = commandLine(plan.kind === "preview" ? plan.applyArgv : plan.argv);

  function change(next: FormValues) {
    if (!sameInput(values, next) && flow.spec) flow.reset();
    setValues(next);
  }

  function run() {
    if (plan.kind === "terminal") return;
    const spec: FlowSpec = {
      title: row.id,
      line,
      tier: plan.kind === "run" ? "read" : plan.tier,
      phrase: phraseFor(row, values),
      mode: plan.kind,
      confirmFirst: plan.kind === "run" && plan.ask,
      argv: plan.kind === "preview" ? plan.applyArgv : plan.argv,
      previewArgv: plan.kind === "preview" ? plan.previewArgv : undefined,
    };
    flow.begin(spec, (phase) => (phase === "apply" ? secret.current?.take() : undefined));
  }

  const label =
    plan.kind === "preview"
      ? "Preview changes"
      : plan.kind === "run" && !plan.ask
        ? "Run"
        : "Run…";
  const blocked = row.planned ? "Planned: this command is not in the CLI yet." : null;

  return (
    <section aria-label={row.id} className="flex min-w-0 flex-col gap-4">
      <div className="flex flex-col gap-2">
        <h2 className="font-mono text-base font-semibold break-words">{row.id}</h2>
        <p className="text-sm text-muted-foreground">{row.summary}</p>
        <div className="flex flex-wrap items-center gap-1.5">
          <TierBadge tier={row.tier} />
          <NeedBadges row={row} />
        </div>
        {row.baseTier && row.tier && row.baseTier !== row.tier && (
          <p className="text-xs text-muted-foreground">
            Reads by default. It starts changing things when a flag marked "changes
            things" is set
            {row.operandEscalates ? " or an operand is given" : ""}.
          </p>
        )}
      </div>

      <CommandForm
        row={row}
        values={values}
        onChange={change}
        onSecretFilled={(secretFilled) =>
          setValues((prev) =>
            prev.secretFilled === secretFilled ? prev : { ...prev, secretFilled },
          )
        }
        secret={secret}
        disabled={flow.busy}
      />

      <div className="flex flex-col gap-1.5">
        <p className="text-2xs font-semibold tracking-[0.09em] text-muted-foreground uppercase">
          Command line
        </p>
        <CommandLine line={line} />
      </div>

      {terminal ? (
        <Callout variant="info" className="flex items-start gap-2">
          <Terminal className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          <span>
            This command needs a terminal, so it cannot run from here. Copy the command
            line above and run it there.
          </span>
        </Callout>
      ) : (
        <div className="flex flex-col gap-2">
          {issues.length > 0 && !blocked && (
            <p className="text-xs text-muted-foreground">
              Still needed: {issues.join("; ")}.
            </p>
          )}
          {blocked && <p className="text-xs text-muted-foreground">{blocked}</p>}
          <div>
            <Button
              type="button"
              onClick={run}
              disabled={flow.busy || issues.length > 0 || !!blocked}
            >
              <Play /> {label}
            </Button>
          </div>
        </div>
      )}

      <RunFlowView flow={flow} />
    </section>
  );
}
