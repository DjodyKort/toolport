import { useId, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";

/** A quiet row that opens a detail list: the checks behind a status, the tools behind a count. */
export function Disclosure({
  label,
  children,
  defaultOpen = false,
}: {
  label: string;
  children: ReactNode;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const id = useId();
  const Icon = open ? ChevronDown : ChevronRight;
  return (
    <div className="border-t border-border">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-2 px-4 py-2.5 text-left text-meta font-medium text-muted-foreground hover:bg-muted/50 focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none"
      >
        <Icon className="size-3.5" aria-hidden="true" />
        {label}
      </button>
      {open && (
        <div id={id} className="border-t border-border">
          {children}
        </div>
      )}
    </div>
  );
}
