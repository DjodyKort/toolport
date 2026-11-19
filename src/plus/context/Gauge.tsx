import { useId, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { useLoad } from "../servers/useLoad";
import { checkpointStatus, setCheckpoint } from "./commands";
import type { CheckpointData } from "./data";
import { tokens, whenText } from "./model";
import PlanDialog from "./PlanDialog";

function percent(part: number, whole: number): number {
  return whole > 0 ? Math.min(100, Math.max(0, (part / whole) * 100)) : 0;
}

function CheckpointForm({
  current,
  onCancel,
  onPreview,
}: {
  current: number | null;
  onCancel: () => void;
  onPreview: (at: number | null) => void;
}) {
  const id = useId();
  const [text, setText] = useState(current === null ? "" : String(current));
  const value = Number(text);
  const valid = Number.isInteger(value) && value >= 1 && value < 1_000_000;
  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Checkpoint</DialogTitle>
          <DialogDescription>
            How many tokens before the compaction point a long session should have its
            handoff written. The gauge marks that point; it does not write anything
            itself.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-1 text-meta">
          <label htmlFor={id} className="font-medium">
            Tokens before compaction
          </label>
          <Input
            id={id}
            inputMode="numeric"
            value={text}
            placeholder="44000"
            aria-invalid={text !== "" && !valid}
            onChange={(event) => setText(event.target.value)}
          />
          {text !== "" && !valid && (
            <span role="alert" className="text-destructive">
              A whole number between 1 and 999,999.
            </span>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          {current !== null && (
            <Button variant="outline" onClick={() => onPreview(null)}>
              Switch off…
            </Button>
          )}
          <Button disabled={!valid} onClick={() => onPreview(value)}>
            Preview the plan
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Auto-compact and checkpoint: how full the newest Claude session is against the window it
 * compacts at. The numbers are the last request of the newest transcript; with none, the card
 * says so instead of showing a figure. */
export default function Gauge() {
  const gauge = useLoad(() => checkpointStatus(), "gauge");
  const [form, setForm] = useState(false);
  const [plan, setPlan] = useState<{ at: number | null } | null>(null);
  const data: CheckpointData | null = gauge.data;

  const used = data?.usedTokens;
  const window = data?.window?.tokens ?? 0;
  const point = data?.checkpoint?.point ?? null;

  return (
    <section
      aria-label="Auto-compact and checkpoint"
      className="flex flex-col gap-2 rounded-xl border bg-card p-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold">Auto-compact and checkpoint</h3>
        <Button size="sm" variant="outline" onClick={() => setForm(true)}>
          Edit
        </Button>
      </div>
      {gauge.error && !data && (
        <p role="alert" className="text-meta text-destructive">
          {gauge.error}
        </p>
      )}
      {data && used === undefined && (
        <p className="text-meta text-muted-foreground">
          {data.note ?? "No Claude session with usage was found yet."} The gauge fills
          once a session has run.
          {data.checkpointAt !== null &&
            ` The checkpoint is set to ${tokens(data.checkpointAt)} tokens before compaction.`}
        </p>
      )}
      {data && used !== undefined && window > 0 && (
        <>
          <div
            role="meter"
            aria-label="Context used"
            aria-valuemin={0}
            aria-valuenow={used}
            aria-valuemax={window}
            className="relative h-3 overflow-hidden rounded-full bg-muted"
          >
            <i
              className="absolute inset-y-0 left-0 bg-primary"
              style={{ width: `${percent(used, window)}%` }}
            />
            {point !== null && (
              <u
                aria-hidden
                className="absolute inset-y-0 w-0.5 bg-foreground no-underline"
                style={{ left: `${percent(point, window)}%` }}
              />
            )}
          </div>
          <p className="text-meta text-muted-foreground">
            {tokens(used)} of {tokens(window)} used in the newest session
            {data.session?.cwd ? ` (${data.session.cwd})` : ""}
            {point !== null
              ? ` · checkpoint at ${tokens(point)}${data.checkpoint?.reached ? ", reached" : ""}`
              : " · no checkpoint set"}{" "}
            · compaction at {tokens(data.compactionAt ?? window)}. Window from the{" "}
            {data.window?.source}.
            {data.session?.stale &&
              ` That session last ran ${whenText(data.session.lastActivityAt)}.`}
          </p>
        </>
      )}
      {form && (
        <CheckpointForm
          current={data?.checkpointAt ?? null}
          onCancel={() => setForm(false)}
          onPreview={(at) => {
            setForm(false);
            setPlan({ at });
          }}
        />
      )}
      {plan && (
        <PlanDialog
          title={plan.at === null ? "Switch the checkpoint off" : "Set the checkpoint"}
          description="Writes one key of context.json; every other key stays."
          confirmLabel={plan.at === null ? "Switch off" : "Set checkpoint"}
          preview={() => setCheckpoint(plan.at, true)}
          confirm={async () => {
            const before = data?.checkpointAt ?? null;
            await setCheckpoint(plan.at, false);
            toast.success(
              plan.at === null ? "Checkpoint switched off" : "Checkpoint set",
              {
                duration: 10_000,
                action: {
                  label: "Undo",
                  onClick: () => {
                    setCheckpoint(before, false).then(gauge.reload, (error: unknown) =>
                      toastError("Could not undo", { description: errorText(error) }),
                    );
                  },
                },
              },
            );
          }}
          onClose={() => setPlan(null)}
          onDone={gauge.reload}
        />
      )}
    </section>
  );
}
