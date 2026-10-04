import { useId, type FormEvent, type ReactNode } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

export const SELECT_CLASS =
  "h-8 w-full rounded-lg border border-input bg-transparent px-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 dark:bg-input/30";

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: (id: string, describedBy: string | undefined) => ReactNode;
}) {
  const id = useId();
  const hintId = hint ? `${id}-hint` : undefined;
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="text-sm font-medium">
        {label}
      </label>
      {children(id, hintId)}
      {hint && (
        <small id={hintId} className="text-xs text-muted-foreground">
          {hint}
        </small>
      )}
    </div>
  );
}

/** A small form in a dialog; its submit starts the preview of a write or a read. */
export function OptionsDialog({
  title,
  description,
  submitLabel,
  canSubmit = true,
  onSubmit,
  onClose,
  children,
}: {
  title: string;
  description?: ReactNode;
  submitLabel: string;
  canSubmit?: boolean;
  onSubmit: () => void;
  onClose: () => void;
  children: ReactNode;
}) {
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (canSubmit) onSubmit();
  };
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            {description ? (
              <DialogDescription>{description}</DialogDescription>
            ) : (
              <DialogDescription className="sr-only">{title}</DialogDescription>
            )}
          </DialogHeader>
          {children}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!canSubmit}>
              {submitLabel}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
