import { Badge } from "@/components/ui/badge";
import type { CommandRow } from "../bridge/data";
import type { Tier } from "./model";

const TIERS: Record<
  Tier,
  { label: string; variant: "secondary" | "warning" | "destructive"; hint: string }
> = {
  read: { label: "Read", variant: "secondary", hint: "Only reads" },
  write: { label: "Write", variant: "warning", hint: "Changes settings or files" },
  destructive: {
    label: "Destructive",
    variant: "destructive",
    hint: "Removes things or cannot be undone",
  },
};

export function TierBadge({ tier }: { tier: Tier | null }) {
  if (!tier) return <Badge variant="outline">Group</Badge>;
  const { label, variant, hint } = TIERS[tier];
  return (
    <Badge variant={variant} title={hint}>
      {label}
    </Badge>
  );
}

const NEEDS: Record<string, string> = {
  stdin: "Reads stdin",
  browser: "Opens a browser",
  "long-running": "Long-running",
  network: "Needs network",
  "terminal-only": "Terminal only",
};

export function NeedBadges({ row }: { row: CommandRow }) {
  const items = [
    ...row.needs.map((need) => NEEDS[need] ?? need),
    ...(row.cost ? ["Uses paid tokens"] : []),
    ...(row.planned ? ["Planned"] : []),
  ];
  return (
    <>
      {items.map((item) => (
        <Badge key={item} variant="outline">
          {item}
        </Badge>
      ))}
    </>
  );
}
