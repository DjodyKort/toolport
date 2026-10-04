import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { nextTabIndex, type TabOrientation } from "./keyboard";

export interface TabItem {
  id: string;
  label: ReactNode;
  /** A count pill after the label, like the section headers use. */
  count?: number;
}

interface Props {
  items: TabItem[];
  value: string;
  onValueChange: (id: string) => void;
  /** Accessible name of the tab list. */
  label: string;
  orientation?: TabOrientation;
  className?: string;
  /** The panel of the selected tab. Other tabs are not mounted. */
  children?: ReactNode;
}

/** A tab list with the roving tabindex pattern: one Tab stop, arrow keys select as they
 * move (see `keyboard.ts`). The panel of the selected tab is labelled by its tab. */
export function Tabs({
  items,
  value,
  onValueChange,
  label,
  orientation = "horizontal",
  className,
  children,
}: Props) {
  const base = useId();
  const list = useRef<HTMLDivElement>(null);
  const tabId = (id: string) => `${base}-tab-${id}`;
  const panelId = `${base}-panel`;
  const selected = items.findIndex((item) => item.id === value);
  const stop = selected >= 0 ? selected : 0;

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    const next = nextTabIndex(event.key, stop, items.length, orientation);
    if (next === null) return;
    event.preventDefault();
    onValueChange(items[next].id);
    list.current?.querySelectorAll<HTMLElement>('[role="tab"]')[next]?.focus();
  }

  return (
    <div className={cn("flex flex-col gap-4", className)}>
      <div
        ref={list}
        role="tablist"
        aria-label={label}
        aria-orientation={orientation}
        className={cn(
          "flex gap-1 overflow-auto border-b",
          orientation === "vertical" && "flex-col border-r border-b-0",
        )}
      >
        {items.map((item, index) => {
          const active = index === stop;
          return (
            <button
              key={item.id}
              id={tabId(item.id)}
              type="button"
              role="tab"
              aria-selected={active}
              aria-controls={active ? panelId : undefined}
              tabIndex={active ? 0 : -1}
              onClick={() => onValueChange(item.id)}
              onKeyDown={onKeyDown}
              className={cn(
                "inline-flex items-center gap-1.5 border-b-2 border-transparent px-3 py-2 text-sm whitespace-nowrap text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-1 focus-visible:ring-ring",
                "aria-selected:border-primary aria-selected:font-medium aria-selected:text-foreground",
              )}
            >
              {item.label}
              {item.count != null && (
                <span className="rounded-full bg-secondary px-2 py-px text-2xs font-semibold text-muted-foreground tabular-nums">
                  {item.count}
                </span>
              )}
            </button>
          );
        })}
      </div>
      {children !== undefined && (
        <div
          id={panelId}
          role="tabpanel"
          aria-labelledby={items[stop] ? tabId(items[stop].id) : undefined}
          tabIndex={0}
          className="outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {children}
        </div>
      )}
    </div>
  );
}
