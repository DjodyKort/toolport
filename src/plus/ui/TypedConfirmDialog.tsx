import { useId, useState, type ReactNode } from "react";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Input } from "@/components/ui/input";

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  /** What the user has to type, exactly, to enable the button. */
  phrase: string;
  /** What is about to change, usually a `PlanPreview`. */
  children?: ReactNode;
  confirmLabel: string;
  onConfirm: () => void | Promise<void>;
}

/** The confirmation of a `destructive` policy tier (D-081): the plan, then a field in which
 * the user types the phrase before the button works. Enter never confirms it. The field is
 * empty every time the dialog opens. */
export function TypedConfirmDialog({ open, ...rest }: Props) {
  if (!open) return null;
  return <TypedBody {...rest} />;
}

function TypedBody({
  onOpenChange,
  title,
  phrase,
  children,
  confirmLabel,
  onConfirm,
}: Omit<Props, "open">) {
  const [typed, setTyped] = useState("");
  const fieldId = useId();
  const matches = typed === phrase;
  return (
    <ConfirmDialog
      open
      onOpenChange={onOpenChange}
      title={title}
      destructive
      contentClassName="sm:max-w-lg"
      confirmLabel={confirmLabel}
      confirmDisabled={!matches}
      onConfirm={() => (matches ? onConfirm() : undefined)}
      description={
        <div className="flex flex-col gap-3">
          {children}
          <label htmlFor={fieldId} className="flex flex-col gap-1.5 text-sm">
            <span>
              Type <b className="font-semibold text-foreground">{phrase}</b> to confirm
            </span>
            <Input
              id={fieldId}
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
              autoComplete="off"
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
              aria-invalid={typed !== "" && !matches ? true : undefined}
            />
          </label>
        </div>
      }
    />
  );
}
