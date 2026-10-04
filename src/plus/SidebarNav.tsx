import { Fragment, useId, type ReactNode } from "react";
import type { LucideIcon } from "lucide-react";
import type { View } from "@/lib/types";
import { useAttentionCount } from "./attention";
import { NAV_GROUPS, navItemActive } from "./nav";

export type NavRow = (
  Icon: LucideIcon,
  label: string,
  active: boolean,
  onClick: () => void,
  badge?: number | null,
  stale?: boolean,
  badgeLabel?: string,
  urgent?: boolean,
) => ReactNode;

function attentionLabel(count: number): string {
  return `${count} ${count === 1 ? "needs" : "need"} you`;
}

/** The items of the grouped sidebar B (D-069). The row itself is the one the sidebar draws
 * for every item, so a Toolport+ entry cannot drift from an upstream one. */
export function SidebarNav({
  view,
  onSelectView,
  row,
  quarantined,
  quarantineStale,
  readAttention,
}: {
  view: View;
  onSelectView: (view: View) => void;
  row: NavRow;
  quarantined: number | null;
  quarantineStale: boolean;
  /** Where the Attention number comes from; the default asks the CLI. */
  readAttention?: () => Promise<number | null>;
}) {
  const attention = useAttentionCount(readAttention);
  const prefix = useId();
  return (
    <>
      {NAV_GROUPS.map((group, index) => {
        const items = group.items.map((item) => (
          <Fragment key={item.view}>
            {row(
              item.icon,
              item.label,
              navItemActive(item, view),
              () => onSelectView(item.view),
              item.badge === "attention"
                ? attention
                : item.badge === "quarantine"
                  ? quarantined
                  : undefined,
              item.badge === "quarantine" ? quarantineStale : undefined,
              item.badge === "attention" ? attentionLabel(attention ?? 0) : undefined,
              item.badge === "attention",
            )}
          </Fragment>
        ));
        if (!group.label) return <Fragment key={index}>{items}</Fragment>;
        const labelId = `${prefix}-${index}`;
        return (
          <div
            key={group.label}
            role="group"
            aria-labelledby={labelId}
            className="flex flex-col gap-0.5"
          >
            <div
              id={labelId}
              className="px-2.5 pt-3 pb-1 text-xs font-medium tracking-wide text-muted-foreground uppercase"
            >
              {group.label}
            </div>
            {items}
          </div>
        );
      })}
    </>
  );
}
