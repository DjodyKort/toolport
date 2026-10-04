import { type ReactNode } from "react";
import { cn } from "@/lib/utils";

export function Stat({
  label,
  value,
  note,
  title,
}: {
  label: string;
  value: ReactNode;
  note?: ReactNode;
  /** The exact figure when the value is shortened. */
  title?: string;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5 rounded-lg border bg-card p-3">
      <span className="text-xs text-muted-foreground">{label}</span>
      <b className="text-base font-semibold break-words tabular-nums" title={title}>
        {value}
      </b>
      {note && (
        <small className="text-xs break-words text-muted-foreground">{note}</small>
      )}
    </div>
  );
}

export function Section({
  title,
  note,
  action,
  className,
  children,
}: {
  title: string;
  note?: ReactNode;
  action?: ReactNode;
  className?: string;
  children: ReactNode;
}) {
  return (
    <section
      aria-label={title}
      className={cn(
        "flex min-w-0 flex-col gap-3 rounded-xl border bg-card p-4",
        className,
      )}
    >
      <header className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold">{title}</h3>
          {note && <p className="text-xs text-muted-foreground">{note}</p>}
        </div>
        {action}
      </header>
      {children}
    </section>
  );
}

export interface Column {
  label: string;
  numeric?: boolean;
}

/** A plain table: a caption for screen readers, a header row, numbers aligned right. */
export function DataTable({
  caption,
  columns,
  children,
}: {
  caption: string;
  columns: Column[];
  children: ReactNode;
}) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm">
        <caption className="sr-only">{caption}</caption>
        <thead>
          <tr className="border-b text-left text-xs text-muted-foreground">
            {columns.map((column) => (
              <th
                key={column.label}
                scope="col"
                className={cn(
                  "px-2 py-1.5 font-medium whitespace-nowrap",
                  column.numeric && "text-right",
                )}
              >
                {column.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y">{children}</tbody>
      </table>
    </div>
  );
}

export function Cell({
  numeric,
  className,
  title,
  children,
}: {
  numeric?: boolean;
  className?: string;
  title?: string;
  children: ReactNode;
}) {
  return (
    <td
      title={title}
      className={cn(
        "px-2 py-1.5 align-top",
        numeric && "text-right tabular-nums",
        className,
      )}
    >
      {children}
    </td>
  );
}

export function Muted({ children }: { children: ReactNode }) {
  return <p className="text-sm text-muted-foreground">{children}</p>;
}
