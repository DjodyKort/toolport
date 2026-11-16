import { useId, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";

const BAR = {
  warning: "shadow-[inset_3px_0_0_var(--warning)]",
  danger: "shadow-[inset_3px_0_0_var(--destructive)]",
  success: "shadow-[inset_3px_0_0_var(--success)]",
} as const;

const TILE = {
  warning: "bg-warning/12 text-warning",
  danger: "bg-destructive/12 text-destructive",
  success: "bg-success/12 text-success",
  none: "bg-muted text-muted-foreground",
} as const;

export interface OperationRowProps {
  icon: ReactNode;
  title: string;
  sub: ReactNode;
  /** A problem row wears the status bar of upstream's cards; a quiet row has none. */
  tone?: keyof typeof BAR;
  badge?: ReactNode;
  actions?: ReactNode;
  /** Without `onToggle` the row has nothing behind it and shows no chevron. */
  open?: boolean;
  onToggle?: () => void;
  children?: ReactNode;
}

/** One line of the Operations card and of Finish setup: tile, name, one line of state, a badge, one
 * action and a chevron that opens what is behind it. The same grammar as upstream's server rows. */
export function OperationRow({
  icon,
  title,
  sub,
  tone,
  badge,
  actions,
  open = false,
  onToggle,
  children,
}: OperationRowProps) {
  const panel = useId();
  const head = (
    <>
      <div
        aria-hidden="true"
        className={cn(
          "grid size-8 shrink-0 place-items-center rounded-md [&_svg]:size-4",
          TILE[tone ?? "none"],
        )}
      >
        {icon}
      </div>
      <span className="min-w-0 flex-1 text-left">
        <span className="block text-sm font-semibold break-words">{title}</span>
        <span className="block text-meta break-words text-muted-foreground">{sub}</span>
      </span>
    </>
  );
  return (
    <div
      data-operation={title}
      className={cn("border-b border-border last:border-b-0", tone && BAR[tone])}
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3">
        {onToggle ? (
          <button
            type="button"
            aria-expanded={open}
            aria-controls={panel}
            onClick={onToggle}
            className="flex min-w-0 flex-1 basis-[220px] items-center gap-3 rounded-md focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none"
          >
            {head}
          </button>
        ) : (
          <div className="flex min-w-0 flex-1 basis-[220px] items-center gap-3">
            {head}
          </div>
        )}
        {badge}
        {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
        {onToggle && (
          <button
            type="button"
            tabIndex={-1}
            aria-hidden="true"
            onClick={onToggle}
            className="grid size-6 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-muted"
          >
            <ChevronDown
              className={cn("size-4 transition-transform", open && "rotate-180")}
            />
          </button>
        )}
      </div>
      {open && children && (
        <div
          id={panel}
          className="flex flex-col gap-3 pr-4 pb-3 pl-[3.75rem] max-sm:pl-4"
        >
          {children}
        </div>
      )}
    </div>
  );
}
