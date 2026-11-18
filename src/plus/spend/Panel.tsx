import { useId, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import type { Badge as Kind } from "./data";
import { KindBadge } from "./KindBadge";

export interface PanelProps {
  title: string;
  badge?: Kind;
  note?: ReactNode;
  actions?: ReactNode;
  footer?: ReactNode;
  id?: string;
  className?: string;
  children: ReactNode;
}

/** One card of the page: heading, the kind of its numbers, optional actions, body, footnote. */
export function Panel({
  title,
  badge,
  note,
  actions,
  footer,
  id,
  className,
  children,
}: PanelProps) {
  const heading = useId();
  return (
    <section
      id={id}
      aria-labelledby={heading}
      data-slot="spend-panel"
      className={cn("flex min-w-0 flex-col rounded-xl border bg-card", className)}
    >
      <div className="flex flex-wrap items-center gap-2 border-b px-4 py-3">
        <h2 id={heading} className="text-sm font-semibold">
          {title}
        </h2>
        {note && <span className="text-meta text-muted-foreground">{note}</span>}
        <span className="flex-1" />
        {badge && <KindBadge kind={badge} />}
        {actions}
      </div>
      {children}
      {footer && (
        <div className="border-t px-4 py-3 text-meta text-muted-foreground">{footer}</div>
      )}
    </section>
  );
}
