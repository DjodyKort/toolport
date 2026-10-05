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
import type { AttentionItem } from "../types/attention";
import { DISMISS_CHOICES, type DismissChoice } from "./model";

/** How long a row stays hidden. Choosing only opens the usual preview: nothing is written
 * until that is confirmed. */
export function DismissDialog({
  item,
  onChoose,
  onClose,
}: {
  item: AttentionItem;
  onChoose: (choice: DismissChoice) => void;
  onClose: () => void;
}) {
  const [choice, setChoice] = useState<DismissChoice>("tomorrow");
  const name = useId();
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Hide this row?</DialogTitle>
          <DialogDescription>{item.title}</DialogDescription>
        </DialogHeader>
        <fieldset className="flex flex-col gap-2">
          <legend className="mb-1 text-sm text-muted-foreground">Hide it</legend>
          {DISMISS_CHOICES.map((option) => (
            <label key={option.id} className="flex items-center gap-2 text-sm">
              <input
                type="radio"
                name={name}
                value={option.id}
                checked={choice === option.id}
                onChange={() => setChoice(option.id)}
                className="size-4 accent-primary"
              />
              {option.label}
            </label>
          ))}
        </fieldset>
        <p className="text-xs text-muted-foreground">
          You see the plan before anything is written, and the way to bring it back.
        </p>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => onChoose(choice)}>Show plan</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
