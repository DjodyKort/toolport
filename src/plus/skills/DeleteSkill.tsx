import { useEffect, useRef, useState } from "react";
import { Callout } from "@/components/Callout";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { CommandsData } from "../bridge/data";
import { CommandLine } from "../allcommands/RunFlow";
import { commandLine, toolCallArgv } from "../allcommands/model";
import { applyArgs, callTool, toolRow } from "../agents/selfTool";
import { PlanPreview, ScreenSkeleton, TypedConfirmDialog, errorText } from "../ui";

type Step =
  | { kind: "confirm" }
  | { kind: "running" }
  | { kind: "done"; removedPath: string }
  | { kind: "failed"; message: string };

/** Deletes a skill from the library with `skills_delete` after a preview and a typed
 * confirmation of its name. Uninstall (`skills uninstall`) also removes the synced copies from
 * the clients; this removes only the skill folder of the library. */
export function DeleteSkill({
  name,
  path,
  registry,
  onClose,
  onDeleted,
}: {
  name: string;
  path: string;
  registry: CommandsData | null;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const [step, setStep] = useState<Step>({ kind: "confirm" });
  const opener = useRef(document.activeElement as HTMLElement | null);
  const started = useRef(false);
  const row = toolRow(registry, "skills_delete");

  useEffect(() => {
    const target = opener.current;
    return () => {
      setTimeout(() => target?.isConnected && target.focus(), 0);
    };
  }, []);

  function run() {
    if (!row) return;
    started.current = true;
    setStep({ kind: "running" });
    callTool<{ removedPath: string }>(
      "skills_delete",
      JSON.parse(applyArgs(row, { name })) as Record<string, unknown>,
    ).then(
      (result) => {
        setStep({ kind: "done", removedPath: result.removedPath });
        onDeleted();
      },
      (error) => setStep({ kind: "failed", message: errorText(error).message }),
    );
  }

  const plan = {
    summary: `Delete skill ${name} from the library`,
    steps: [
      {
        op: "delete" as const,
        path,
        detail: "Remove the skill folder from your skills repository",
      },
    ],
    effects: {},
    warnings: [
      "Synced copies in the clients stay until the next sync or clean. Uninstall removes them too.",
    ],
    undo: "If your library is a git clone, restore the folder from its last commit.",
  };

  if (step.kind === "confirm" && row) {
    return (
      <TypedConfirmDialog
        open
        onOpenChange={(open) => !open && !started.current && onClose()}
        title={`Delete skill ${name}?`}
        phrase={name}
        confirmLabel="Delete"
        onConfirm={run}
      >
        <div className="flex flex-col gap-3">
          <PlanPreview data={{ plan }} />
          <CommandLine line={`${commandLine(toolCallArgv("skills_delete"))}`} />
        </div>
      </TypedConfirmDialog>
    );
  }

  return (
    <Dialog open onOpenChange={(open) => !open && step.kind !== "running" && onClose()}>
      <DialogContent aria-describedby={undefined} className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Delete skill {name}</DialogTitle>
        </DialogHeader>
        {step.kind === "confirm" && (
          <Callout variant="warning" role="status">
            {registry
              ? "Toolport does not know how safe `skills_delete` is, so it does not run it from here."
              : "The tool list has not loaded yet. Try again in a moment."}
          </Callout>
        )}
        {step.kind === "running" && <ScreenSkeleton />}
        {step.kind === "done" && (
          <Callout variant="success" role="status">
            Deleted {name}. Removed {step.removedPath}.
          </Callout>
        )}
        {step.kind === "failed" && (
          <Callout variant="danger" role="alert">
            Nothing was deleted: {step.message}
          </Callout>
        )}
        <DialogFooter>
          <Button disabled={step.kind === "running"} onClick={onClose}>
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
