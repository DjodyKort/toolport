import { useState } from "react";
import { ArrowLeft, RefreshCw, Undo2 } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { toastError } from "@/lib/toast";
import { CtlError } from "../bridge/ctl";
import { errorText } from "../settings/useCtl";
import HookReview, { type HookView } from "./HookReview";
import { plural } from "./model";
import { selectApply, selectPlan, selectSet, selectUndo } from "./selectCommands";
import type {
  PlanRowData,
  SelectHistory,
  SelectItem,
  SelectPlanData,
} from "./selectData";
import { commandLines, lineSummary, rowLabel, shortRef } from "./selectModel";
import { Banner, CliFooter, HistoryBanner, PolicyBanners } from "./SelectParts";

function Group({
  title,
  pill,
  tone,
  children,
}: {
  title: string;
  pill?: string;
  tone?: "success" | "warning" | "partial" | "destructive";
  children: React.ReactNode;
}) {
  return (
    <section aria-label={title} className="flex flex-col gap-1.5">
      <h3 className="flex flex-wrap items-center gap-2 text-sm font-semibold">
        {title}
        {pill && <Badge variant={tone ?? "off"}>{pill}</Badge>}
      </h3>
      <ul className="flex flex-col gap-1.5">{children}</ul>
    </section>
  );
}

function Line({
  row,
  note,
  children,
}: {
  row: PlanRowData;
  note: React.ReactNode;
  children?: React.ReactNode;
}) {
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border bg-card px-3 py-2">
      <div className="min-w-0 flex-1 basis-48">
        <p className="text-sm font-medium break-words">{row.id}</p>
        <p className="text-meta break-words text-muted-foreground">
          {row.kind} · {note}
        </p>
      </div>
      {children && <div className="flex flex-wrap gap-2">{children}</div>}
    </li>
  );
}

export default function SelectPlan({
  source,
  plan,
  items,
  history,
  switchWhole,
  onPlan,
  onBack,
  onApplied,
}: {
  source: string;
  plan: SelectPlanData;
  items: SelectItem[];
  history: SelectHistory;
  switchWhole: boolean;
  onPlan: (plan: SelectPlanData) => void;
  onBack: () => void;
  onApplied: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [stale, setStale] = useState<string | null>(null);
  const [applied, setApplied] = useState<SelectPlanData | null>(null);
  const [review, setReview] = useState<HookView | null>(null);
  const [kept, setKept] = useState<Set<string>>(new Set());
  const d = plan.diffs;

  async function replan() {
    setBusy(true);
    try {
      onPlan(await selectPlan(source, switchWhole));
      setStale(null);
    } catch (error) {
      toastError("No plan was made", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  async function apply() {
    setBusy(true);
    try {
      setApplied(await selectApply(source, plan.planHash, switchWhole));
    } catch (error) {
      if (error instanceof CtlError && error.code === "conflict") {
        setStale(errorText(error));
      } else {
        toastError("Nothing was applied", { description: errorText(error) });
      }
    } finally {
      setBusy(false);
    }
  }

  async function turnOff(row: PlanRowData) {
    setBusy(true);
    try {
      await selectSet(source, { on: [], off: [rowLabel(row)], tick: [] });
      onPlan(await selectPlan(source, switchWhole));
    } catch (error) {
      toastError("Nothing was changed", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  async function undo() {
    setBusy(true);
    try {
      await selectUndo(source);
      toast.success("Restored what Claude Code loads from before this apply", {
        description: "Your choices are kept; review the plan to apply them again.",
      });
      onApplied();
    } catch (error) {
      toastError("Nothing was changed", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  const hookView = (row: PlanRowData): HookView => {
    const text = plan.hooks[rowLabel(row)];
    const known = items.find((i) => i.kind === row.kind && i.id === row.id);
    return {
      id: row.id,
      event: text?.event ?? known?.event ?? row.id,
      matcher: text?.matcher ?? known?.matcher ?? null,
      command: text?.command ?? known?.command ?? "",
      was: text?.was ?? null,
    };
  };

  if (applied) {
    return (
      <section aria-label="Applied" className="flex flex-col gap-3">
        <Banner tone="info">
          Applied to {source}. {applied.plan.steps.at(-1)?.detail ?? ""}
        </Banner>
        <div className="flex flex-wrap gap-2">
          <Button size="sm" onClick={onApplied}>
            Back to the items
          </Button>
          <Button variant="outline" size="sm" disabled={busy} onClick={() => void undo()}>
            <Undo2 /> Undo
          </Button>
        </div>
        <CliFooter lines={[`toolportctl library select undo ${source}`]} />
      </section>
    );
  }

  const movedTo = shortRef(plan.ref.latest);
  const nothing =
    !plan.waiting &&
    d.added.length + d.dropped.length + d.changed.length + d.retick.length === 0;
  return (
    <section aria-label="Update plan" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <Button variant="ghost" size="sm" className="-ml-2" onClick={onBack}>
            <ArrowLeft /> Items
          </Button>
          <h2 className="text-sm font-semibold break-words">
            {source}
            {" · "}
            {plan.ref.pinned && plan.ref.pinned !== plan.ref.latest
              ? `${shortRef(plan.ref.pinned)} → ${movedTo}`
              : movedTo}
          </h2>
          <p className="text-meta text-muted-foreground">
            {plan.plan.summary}. Nothing changes until you apply this plan.
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" onClick={onBack}>
            Not now
          </Button>
          <Button size="sm" disabled={busy || nothing} onClick={() => void apply()}>
            Apply plan
          </Button>
        </div>
      </div>

      <PolicyBanners policy={plan.policy} />
      <HistoryBanner history={history} />
      {plan.fetchError && (
        <Banner tone="warning">
          Could not check for a newer version, last good {shortRef(plan.ref.pinned)}:{" "}
          {plan.fetchError}{" "}
          <Button
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={() => void replan()}
          >
            <RefreshCw /> Retry
          </Button>
        </Banner>
      )}
      {stale && (
        <Banner tone="destructive">
          {stale}{" "}
          <Button
            variant="outline"
            size="sm"
            disabled={busy}
            onClick={() => void replan()}
          >
            Plan again
          </Button>
        </Banner>
      )}
      {plan.plan.warnings
        .filter((warning) => warning !== plan.fetchError)
        .map((warning) => (
          <Banner key={warning} tone="warning">
            {warning}
          </Banner>
        ))}
      {nothing && <Banner tone="info">Nothing waits: this source is up to date.</Banner>}

      {(d.added.length > 0 || d.dropped.length > 0) && (
        <Group title={`Your choice (${d.added.length + d.dropped.length})`}>
          {d.added.map((row) => (
            <Line key={`a-${rowLabel(row)}`} row={row} note="turns on" />
          ))}
          {d.dropped.map((row) => (
            <Line key={`d-${rowLabel(row)}`} row={row} note="turns off" />
          ))}
        </Group>
      )}

      {d.changed.length > 0 && (
        <Group
          title={`Your items that change (${d.changed.length})`}
          pill="applied with the plan"
          tone="success"
        >
          {d.changed.map((row) => (
            <Line
              key={rowLabel(row)}
              row={row}
              note={
                <>
                  {lineSummary(plan.lines[rowLabel(row)])}
                  {row.from && row.to && (
                    <>
                      {" "}
                      · <code className="font-mono text-xs">{shortHash(row.from)}</code>
                      {" → "}
                      <code className="font-mono text-xs">{shortHash(row.to)}</code>
                    </>
                  )}
                </>
              }
            />
          ))}
        </Group>
      )}

      {d.retick.length > 0 && (
        <Group
          title={`Hooks that changed (${d.retick.length})`}
          pill="needs your tick again"
          tone="warning"
        >
          {d.retick.map((row) => {
            const view = hookView(row);
            return (
              <Line
                key={rowLabel(row)}
                row={row}
                note={
                  <>
                    command changed:{" "}
                    <code className="font-mono text-xs">{view.was ?? "?"}</code>
                    {" → "}
                    <code className="font-mono text-xs">{view.command}</code>
                    <br />
                    Stays on the old version until ticked
                  </>
                }
              >
                <Button variant="outline" size="sm" onClick={() => setReview(view)}>
                  Review hook
                </Button>
              </Line>
            );
          })}
        </Group>
      )}

      {d.fresh.length > 0 && (
        <Group
          title={`New in the source (${d.fresh.length})`}
          pill="off by default"
          tone="partial"
        >
          {d.fresh.slice(0, 8).map((row) => (
            <Line
              key={rowLabel(row)}
              row={row}
              note={
                row.kind === "hook"
                  ? "new · needs a tick after this plan is applied"
                  : "new · turn it on after this plan is applied"
              }
            >
              {row.kind === "hook" && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => setReview(hookView(row))}
                >
                  Review hook
                </Button>
              )}
            </Line>
          ))}
          {d.fresh.length > 8 && (
            <li className="text-meta text-muted-foreground">
              + {d.fresh.length - 8} more
            </li>
          )}
        </Group>
      )}

      {d.gone.length > 0 && (
        <Group
          title={`Removed upstream (${d.gone.length})`}
          pill="warning"
          tone="destructive"
        >
          {d.gone.map((row) => (
            <Line
              key={rowLabel(row)}
              row={row}
              note={
                kept.has(rowLabel(row))
                  ? "keeps working from the pinned copy"
                  : "no longer in the source. You use it. It keeps working from the pinned copy until you remove it or accept the loss."
              }
            >
              <Button
                variant="outline"
                size="sm"
                onClick={() => setKept(new Set(kept).add(rowLabel(row)))}
              >
                Keep pinned copy
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={busy}
                onClick={() => void turnOff(row)}
              >
                Remove
              </Button>
            </Line>
          ))}
        </Group>
      )}

      {d.rules.length > 0 && (
        <Banner tone="info">
          {plural(d.rules.length, "rule")} on ({d.rules.map((r) => r.id).join(", ")}):
          rules are not part of the generated plugin. Link them through the Library:
          <ul className="mt-1 list-disc pl-5">
            {plan.plan.steps
              .filter((step) => step.op === "link")
              .map((step) => (
                <li key={step.detail} className="break-words">
                  {step.detail.replace(/^rule:/, "")}
                </li>
              ))}
          </ul>
        </Banner>
      )}

      <details className="rounded-lg border bg-card px-3 py-2 text-meta">
        <summary className="cursor-pointer font-medium">
          What apply writes ({plan.plan.steps.length})
        </summary>
        <ul className="mt-2 flex flex-col gap-1">
          {plan.plan.steps.map((step, index) => (
            <li key={`${step.detail}-${index}`}>{step.detail}</li>
          ))}
        </ul>
      </details>

      <CliFooter
        lines={commandLines(
          source,
          { on: [], off: [], tick: [] },
          plan.planHash,
          switchWhole,
        )}
      />

      {review && (
        <HookReview
          hook={review}
          readOnly
          onCancel={() => setReview(null)}
          onConfirm={() => {}}
        />
      )}
    </section>
  );
}

const shortHash = (hash: string) => hash.replace(/^sha256:/, "").slice(0, 8);
