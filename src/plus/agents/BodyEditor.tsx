import { useId, useRef, useState } from "react";
import { Callout } from "@/components/Callout";
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
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type { CommandsData } from "../bridge/data";
import { CommandLine } from "../allcommands/RunFlow";
import { commandLine, toolCallArgv } from "../allcommands/model";
import { ErrorState, PlanPreview, ScreenSkeleton, errorText } from "../ui";
import { NOUN, editPlan, type BodyKind } from "./bodyEdit";
import { applyArgs, callTool, toolRow, useToolRead } from "./selfTool";

interface Loaded {
  body: string;
  description: string;
  name: string;
  path: string;
}

interface Saved {
  newHash: string;
  sourcePath: string;
}

type Step =
  | { kind: "edit" }
  | { kind: "review" }
  | { kind: "saving" }
  | { kind: "saved"; saved: Saved[] }
  | { kind: "failed"; message: string };

/** The editor of one agent, style or skill file. It reads the file with `<kind>_get`, shows a
 * preview of the change, and writes only after the confirmation, with `<kind>_edit_body` (and
 * `skills_edit_frontmatter` for the description of a skill). The text of the file lives in this
 * dialog and nowhere else: it is never logged and never put on a command line. */
export function BodyEditor({
  kind,
  name,
  registry,
  onClose,
  onSaved,
}: {
  kind: BodyKind;
  name: string;
  registry: CommandsData | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const noun = NOUN[kind];
  const read = useToolRead<Loaded>(`${kind}_get`, { name });
  const [body, setBody] = useState<string | null>(null);
  const [description, setDescription] = useState<string | null>(null);
  const [step, setStep] = useState<Step>({ kind: "edit" });
  const opener = useRef(document.activeElement as HTMLElement | null);
  const bodyId = useId();
  const descriptionId = useId();
  const loaded = read.data;

  const bodyTool = `${kind}_edit_body`;
  const bodyRow = toolRow(registry, bodyTool);
  const frontRow =
    kind === "skills" ? toolRow(registry, "skills_edit_frontmatter") : null;
  const text = body ?? loaded?.body ?? "";
  const summary = description ?? loaded?.description ?? "";
  const bodyChanged = !!loaded && text !== loaded.body;
  const descriptionChanged =
    !!loaded && kind === "skills" && summary !== loaded.description;
  const changed = bodyChanged || descriptionChanged;
  const busy = step.kind === "saving";
  const lacksRow =
    !bodyRow || (descriptionChanged && !frontRow)
      ? registry
        ? `Toolport does not know how safe \`${bodyTool}\` is, so it does not run it from here.`
        : "The tool list has not loaded yet. Try again in a moment."
      : null;

  async function apply() {
    if (!loaded || !bodyRow) return;
    setStep({ kind: "saving" });
    const saved: Saved[] = [];
    try {
      if (descriptionChanged && frontRow) {
        saved.push(
          await callTool<Saved>(
            "skills_edit_frontmatter",
            JSON.parse(
              applyArgs(frontRow, { name, patch: { description: summary } }),
            ) as Record<string, unknown>,
          ),
        );
      }
      if (bodyChanged) {
        saved.push(
          await callTool<Saved>(
            bodyTool,
            JSON.parse(applyArgs(bodyRow, { name, new_body: text })) as Record<
              string,
              unknown
            >,
          ),
        );
      }
      setStep({ kind: "saved", saved });
      onSaved();
    } catch (error) {
      setStep({ kind: "failed", message: errorText(error).message });
    }
  }

  const title =
    step.kind === "review" ? `Save ${noun} ${name}?` : `Edit the body of ${noun} ${name}`;

  return (
    <Dialog open onOpenChange={(open) => !open && !busy && onClose()}>
      <DialogContent
        aria-describedby={undefined}
        className="sm:max-w-3xl"
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          if (opener.current?.isConnected) opener.current.focus();
        }}
      >
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          {loaded && <DialogDescription>{loaded.path}</DialogDescription>}
        </DialogHeader>

        {read.status === "loading" && <ScreenSkeleton />}
        {read.status === "error" && (
          <ErrorState
            error={read.error}
            title={`Couldn't read ${noun} ${name}`}
            onRetry={read.reload}
            context={`mcp call ${kind}_get`}
          />
        )}

        {loaded && (step.kind === "edit" || step.kind === "failed") && (
          <div className="flex flex-col gap-3">
            {kind === "skills" && (
              <div className="flex flex-col gap-1.5">
                <Label htmlFor={descriptionId} className="text-xs">
                  Description
                </Label>
                <Input
                  id={descriptionId}
                  value={summary}
                  onChange={(event) => setDescription(event.target.value)}
                />
              </div>
            )}
            <div className="flex flex-col gap-1.5">
              <Label htmlFor={bodyId} className="text-xs">
                Body of {name}
              </Label>
              <Textarea
                id={bodyId}
                value={text}
                rows={16}
                spellCheck={false}
                onChange={(event) => setBody(event.target.value)}
                className="max-h-[50vh] font-mono text-xs"
              />
            </div>
            {step.kind === "failed" && (
              <Callout variant="danger" role="alert">
                Nothing was saved to the end: {step.message}
              </Callout>
            )}
            {lacksRow && changed && <Callout variant="warning">{lacksRow}</Callout>}
          </div>
        )}

        {loaded && step.kind === "review" && (
          <div className="flex flex-col gap-3">
            <PlanPreview
              data={{
                plan: editPlan({
                  kind,
                  name,
                  path: loaded.path,
                  body: { before: loaded.body, after: text },
                  description: { before: loaded.description, after: summary },
                }),
              }}
            />
            <CommandLine
              line={`${commandLine(toolCallArgv(bodyTool))} (arguments on stdin)`}
            />
          </div>
        )}

        {step.kind === "saving" && <ScreenSkeleton />}
        {step.kind === "saved" && (
          <Callout variant="success" role="status">
            Saved {noun} {name}.{" "}
            {step.saved.map((entry) => `New hash ${entry.newHash}.`).join(" ")}
          </Callout>
        )}

        <DialogFooter>
          {step.kind === "saved" ? (
            <Button onClick={onClose}>Done</Button>
          ) : (
            <>
              <Button variant="ghost" disabled={busy} onClick={onClose}>
                Cancel
              </Button>
              {step.kind === "review" ? (
                <>
                  <Button variant="outline" onClick={() => setStep({ kind: "edit" })}>
                    Back
                  </Button>
                  <Button onClick={() => void apply()}>Save</Button>
                </>
              ) : (
                <Button
                  disabled={!loaded || !changed || !!lacksRow || busy}
                  onClick={() => setStep({ kind: "review" })}
                >
                  Review changes
                </Button>
              )}
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
