import { useRef, type ReactNode } from "react";
import { CircleAlert } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

export interface SpendDialogProps {
  title: string;
  description: ReactNode;
  children: ReactNode;
  footer: ReactNode;
  /** While work runs the dialog cannot be dismissed. */
  busy?: boolean;
  onClose: () => void;
}

/** The frame of every Spend dialog: focus starts on the heading so Enter never presses a
 * button, Esc and the close button dismiss it unless work is running, and at 390 px it is a
 * bottom sheet whose footer stays in view. */
export function SpendDialog({
  title,
  description,
  children,
  footer,
  busy = false,
  onClose,
}: SpendDialogProps) {
  const heading = useRef<HTMLHeadingElement>(null);
  return (
    <Dialog open onOpenChange={(open) => !open && !busy && onClose()}>
      <DialogContent
        closeDisabled={busy}
        className="sm:max-w-xl max-sm:top-auto max-sm:bottom-0 max-sm:max-w-full max-sm:translate-y-0 max-sm:rounded-b-none"
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          heading.current?.focus();
        }}
        onEscapeKeyDown={(event) => busy && event.preventDefault()}
        onPointerDownOutside={(event) => busy && event.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle ref={heading} tabIndex={-1} className="outline-none">
            {title}
          </DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        {children}
        <DialogFooter>{footer}</DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** A failure inside a dialog: names what failed, never only a colour. */
export function DialogError({ title, detail }: { title: string; detail: string }) {
  return (
    <div
      role="alert"
      className="flex gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-meta"
    >
      <CircleAlert className="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden />
      <div className="flex flex-col gap-0.5">
        <b className="text-foreground">{title}</b>
        <span className="break-words text-muted-foreground">{detail}</span>
      </div>
    </div>
  );
}
