import { useId, useRef, useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { CommandLine } from "../allcommands/RunFlow";
import { commandLine } from "../allcommands/model";
import type { CommandRow } from "../bridge/data";
import {
  JobProgress,
  PlanPreview,
  SecretField,
  outcomeOf,
  useCtlJob,
  type PlanV1,
  type SecretFieldHandle,
} from "../ui";
import { policyOf } from "./model";

/** A write whose passphrase goes to the child's stdin and nowhere else (D-059, D-081): the
 * plan, the command line without the secret, the write-only field (twice when it is a new
 * passphrase), and the typed phrase when the registry says the tier is destructive. These
 * commands have no dry run, so the confirmation is the preview. */
export function SecretWriteDialog({
  title,
  description,
  command,
  argv,
  plan,
  rows,
  secretLabel = "Passphrase",
  repeat = false,
  form,
  canRun = true,
  confirmLabel,
  doneLabel = "Done",
  renderResult,
  onClose,
  onDone,
}: {
  title: string;
  description?: ReactNode;
  /** The registry id the tier is read from. */
  command: string;
  /** The argv without the secret; the passphrase flag reads stdin. */
  argv: string[];
  plan: PlanV1;
  rows: CommandRow[] | null;
  secretLabel?: string;
  /** Ask for the passphrase a second time. */
  repeat?: boolean;
  /** The other fields of the command, above the passphrase. */
  form?: ReactNode;
  /** Whether those fields are filled in well enough to run. */
  canRun?: boolean;
  confirmLabel: string;
  doneLabel?: string;
  renderResult?: (data: unknown) => ReactNode;
  onClose: () => void;
  onDone?: (data: unknown) => void;
}) {
  const job = useCtlJob();
  const first = useRef<SecretFieldHandle>(null);
  const second = useRef<SecretFieldHandle>(null);
  const [filled, setFilled] = useState([false, false]);
  const [typed, setTyped] = useState("");
  const [mismatch, setMismatch] = useState(false);
  const phraseId = useId();
  const policy = policyOf(rows, command);
  const destructive = policy?.tier === "destructive";
  const running = job.state.phase === "running";
  const started = job.state.phase !== "idle";
  const outcome = outcomeOf(job.state);
  const ready =
    !!policy &&
    !policy.terminal &&
    canRun &&
    filled[0] &&
    (!repeat || filled[1]) &&
    (!destructive || typed === command);

  async function run() {
    const value = first.current?.take() ?? "";
    const again = repeat ? (second.current?.take() ?? "") : value;
    if (value === "") return;
    if (value !== again) {
      setMismatch(true);
      return;
    }
    setMismatch(false);
    const result = await job.start(argv, { stdinSecret: value });
    if (result?.envelope?.ok) onDone?.(result.envelope.data);
  }

  return (
    <Dialog open onOpenChange={(open) => !open && !running && onClose()}>
      <DialogContent
        className="sm:max-w-xl"
        showCloseButton={!running}
        onEscapeKeyDown={(event) => running && event.preventDefault()}
        onInteractOutside={(event) => event.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>{title}?</DialogTitle>
          <DialogDescription>
            {description ?? "Review the change, enter the passphrase and confirm."}
          </DialogDescription>
        </DialogHeader>
        {!started && (
          <div className="flex flex-col gap-3">
            {!policy && (
              <Callout variant="warning" role="status">
                {rows
                  ? `Toolport does not know how safe \`${command}\` is, so it does not run it from here.`
                  : "The command list has not loaded yet. Try again in a moment."}
              </Callout>
            )}
            <PlanPreview data={{ plan }} />
            <CommandLine line={commandLine(argv)} />
            <form
              className="flex flex-col gap-3"
              onSubmit={(event) => {
                event.preventDefault();
                if (ready && !destructive) void run();
              }}
            >
              {form}
              <SecretField
                label={secretLabel}
                handle={first}
                hint="Sent on stdin to the command. It is not shown, logged or kept in the app."
                onFilledChange={(value) => {
                  setFilled((prev) => [value, prev[1]]);
                  setMismatch(false);
                }}
              />
              {repeat && (
                <SecretField
                  label={`Repeat ${secretLabel.toLowerCase()}`}
                  handle={second}
                  hint="Type it again so a typo is not locked in."
                  onFilledChange={(value) => {
                    setFilled((prev) => [prev[0], value]);
                    setMismatch(false);
                  }}
                />
              )}
              {mismatch && (
                <Callout variant="danger" role="alert">
                  The two entries differ. Both fields were cleared; type them again.
                </Callout>
              )}
              {destructive && (
                <label htmlFor={phraseId} className="flex flex-col gap-1.5 text-sm">
                  <span>
                    Type <b className="font-semibold text-foreground">{command}</b> to
                    confirm
                  </span>
                  <Input
                    id={phraseId}
                    value={typed}
                    onChange={(event) => setTyped(event.target.value)}
                    autoComplete="off"
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                  />
                </label>
              )}
            </form>
          </div>
        )}
        {started && (
          <JobProgress
            state={job.state}
            onCancel={() => void job.cancel()}
            title="Applying"
            doneLabel={doneLabel}
            renderResult={
              renderResult ??
              ((data) =>
                data && typeof data === "object" ? (
                  <PlanPreview data={data} />
                ) : (
                  <p>Done.</p>
                ))
            }
          />
        )}
        <DialogFooter>
          {!started && (
            <>
              <Button variant="ghost" onClick={onClose}>
                Cancel
              </Button>
              <Button
                variant={destructive ? "destructive" : "default"}
                disabled={!ready}
                onClick={() => void run()}
              >
                {confirmLabel}
              </Button>
            </>
          )}
          {started && (
            <Button
              variant={outcome?.kind === "ok" ? "default" : "ghost"}
              onClick={onClose}
              disabled={running}
            >
              {outcome?.kind === "ok" ? "Done" : "Close"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
