import type { ReactNode } from "react";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { Tone } from "./model";

const VARIANT = {
  danger: "destructive",
  warning: "warning",
  success: "success",
  info: "info",
  off: "off",
} as const;

export function Chip({ tone = "off", children }: { tone?: Tone; children: ReactNode }) {
  return <Badge variant={VARIANT[tone]}>{children}</Badge>;
}

/** A bordered card whose children are separated by hairlines, as upstream's list cards. */
export function Group({
  children,
  dim = false,
  className,
}: {
  children: ReactNode;
  dim?: boolean;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "divide-y overflow-hidden rounded-xl border bg-card",
        dim && "opacity-60",
        className,
      )}
    >
      {children}
    </div>
  );
}

export function Quiet({
  children,
  className,
  role,
}: {
  children: ReactNode;
  className?: string;
  role?: "alert" | "status";
}) {
  return (
    <p
      role={role}
      className={cn("text-meta break-words text-muted-foreground", className)}
    >
      {children}
    </p>
  );
}

export function Mono({ children }: { children: ReactNode }) {
  return (
    <code className="rounded bg-muted px-1 py-0.5 text-[12px] break-all">{children}</code>
  );
}

export function Field({
  label,
  hint,
  htmlFor,
  children,
}: {
  label: string;
  hint?: ReactNode;
  htmlFor?: string;
  children: ReactNode;
}) {
  return (
    <div className="grid gap-1.5">
      <label htmlFor={htmlFor} className="text-sm font-medium">
        {label}
      </label>
      {children}
      {hint && <Quiet>{hint}</Quiet>}
    </div>
  );
}
