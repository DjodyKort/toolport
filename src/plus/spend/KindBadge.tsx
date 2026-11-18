import { Badge } from "@/components/ui/badge";
import type { Badge as Kind } from "./data";

const VARIANT = {
  Measured: "success",
  Counted: "outline",
  Estimate: "warning",
  "Not itemised": "off",
} as const;

/** What kind of number this is. The word is always shown: colour never carries it alone. */
export function KindBadge({ kind, className }: { kind: Kind; className?: string }) {
  return (
    <Badge variant={VARIANT[kind]} data-kind={kind} className={className}>
      {kind}
    </Badge>
  );
}
