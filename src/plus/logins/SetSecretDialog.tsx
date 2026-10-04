import { useId, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { JobProgress, PlanPreview, SecretField, outcomeOf, useCtlJob } from "../ui";
import { setLine, setPlan, type ServerRef } from "./plans";

/** `secret set`: a write-only field in a dialog. The plan says what is written, Save is the
 * confirmation, and the value goes to the child's stdin and nowhere else. */
export function SetSecretDialog({
  server,
  keys,
  initialKey,
  existing,
  onClose,
  onStored,
  onProbe,
}: {
  server: ServerRef;
  keys: string[];
  initialKey?: string;
  /** Whether the key already has a value (a replace); null when it is not known. */
  existing: (key: string) => boolean | null;
  onClose: () => void;
  onStored: (key: string) => void;
  /** Probe this server now, offered once the value is stored. */
  onProbe?: () => void;
}) {
  const job = useCtlJob();
  const [key, setKey] = useState(
    initialKey && keys.includes(initialKey) ? initialKey : keys[0],
  );
  const keyId = useId();
  const running = job.state.phase === "running";
  const outcome = outcomeOf(job.state);
  const stored = outcome?.kind === "ok";
  const current = existing(key);
  const replacing = current === true;

  async function save(value: string) {
    const result = await job.start(["secret", "set", server.id, key], {
      stdinSecret: value,
    });
    if (result?.envelope?.ok) onStored(key);
  }

  return (
    <Dialog open onOpenChange={(open) => !open && !running && onClose()}>
      <DialogContent
        className="sm:max-w-lg"
        showCloseButton={!running}
        onEscapeKeyDown={(event) => running && event.preventDefault()}
        onInteractOutside={(event) => event.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>
            {replacing ? "Replace" : "Set"} {key}
          </DialogTitle>
          <DialogDescription>
            {server.name}. The value is written to the vault and never shown again.
          </DialogDescription>
        </DialogHeader>
        {!stored && (
          <div className="flex flex-col gap-3">
            {keys.length > 1 && (
              <label
                htmlFor={keyId}
                className="flex flex-col gap-1.5 text-sm font-medium"
              >
                Key
                <select
                  id={keyId}
                  value={key}
                  disabled={running}
                  onChange={(event) => setKey(event.target.value)}
                  className="h-8 rounded-lg border bg-background px-2 text-sm"
                >
                  {keys.map((candidate) => (
                    <option key={candidate} value={candidate}>
                      {candidate}
                    </option>
                  ))}
                </select>
              </label>
            )}
            <PlanPreview data={setPlan(server, key, current)} />
            <p className="text-xs text-muted-foreground">
              Runs <code className="font-mono">{setLine(server, key)}</code> with the
              value on stdin.
            </p>
            <SecretField
              label="New value"
              hint="Paste the new value. It is not shown, logged or kept in the app."
              submitLabel="Save to vault"
              onSubmit={save}
              disabled={running}
            />
          </div>
        )}
        {job.state.phase !== "idle" && (
          <JobProgress
            state={job.state}
            onCancel={() => void job.cancel()}
            title="Saving"
            doneLabel="Saved to the vault"
            renderResult={() => (
              <p>
                {key} is stored for {server.name}.
              </p>
            )}
          />
        )}
        <DialogFooter>
          {stored && onProbe && (
            <Button variant="outline" onClick={onProbe}>
              Probe {server.name} now
            </Button>
          )}
          <Button
            variant={stored ? "default" : "ghost"}
            onClick={onClose}
            disabled={running}
          >
            {stored ? "Done" : "Cancel"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
