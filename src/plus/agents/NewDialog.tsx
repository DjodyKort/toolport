import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { nameProblem } from "./model";

/** Asks for the name of a new agent or style; the template, the preview and the confirmation
 * are the CLI's (`agents add`, `styles add`). */
export function NewDialog({
  kind,
  onSubmit,
  onClose,
}: {
  kind: "agent" | "style";
  onSubmit: (name: string) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [touched, setTouched] = useState(false);
  const problem = nameProblem(name);
  const submit = () => {
    setTouched(true);
    if (!problem) onSubmit(name);
  };
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>New {kind}</DialogTitle>
          <DialogDescription>
            Toolport creates {kind === "agent" ? "an AGENT.md" : "a STYLE.md"} from its
            template in your skills repository. You see what it will write before anything
            is written.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <label className="flex flex-col gap-1.5 text-sm">
            Name
            <Input
              autoFocus
              value={name}
              onChange={(event) => setName(event.target.value.trim())}
              autoComplete="off"
              spellCheck={false}
              aria-invalid={touched && problem ? true : undefined}
              aria-describedby="new-name-problem"
            />
          </label>
          <p
            id="new-name-problem"
            role="status"
            className="min-h-4 text-xs text-destructive"
          >
            {touched && problem ? problem : ""}
          </p>
        </form>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={submit}>Preview</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
