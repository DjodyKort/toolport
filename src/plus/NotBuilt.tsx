import { useState } from "react";
import { Hammer } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { View } from "@/lib/types";
import { EmptyState } from "@/components/ui/empty-state";
import { PLUS_SCREENS, isPlusView, type PlusView } from "./nav";
import { NOT_BUILT_TABS } from "./notBuiltTabs";
import { Tabs } from "./ui";

interface PlusScreenLink {
  view: View;
  title: string;
}

export function NotBuiltPanel({
  name,
  builtBy,
  group,
  opens,
  onOpenCommands,
  onOpenView,
}: {
  name: string;
  builtBy: string;
  group?: string;
  opens?: PlusScreenLink;
  onOpenCommands: (group?: string) => void;
  onOpenView?: (view: View) => void;
}) {
  return (
    <EmptyState
      icon={<Hammer />}
      title={
        <span className="inline-flex items-center gap-2">
          {name} <Badge variant="warning">Not built yet</Badge>
        </span>
      }
      description={
        <>
          This screen is built by {builtBy}. Until then everything it will cover is
          available on the All commands page.
        </>
      }
      action={
        <div className="flex flex-wrap justify-center gap-2">
          {opens && onOpenView && (
            <Button size="sm" onClick={() => onOpenView(opens.view)}>
              Open {opens.title}
            </Button>
          )}
          <Button variant="outline" size="sm" onClick={() => onOpenCommands(group)}>
            Open All commands
          </Button>
        </div>
      }
    />
  );
}

/** A Toolport+ screen that no item has built yet: the tabs of the mockup, and in each a
 * marked placeholder that names the item that builds it. */
export function NotBuilt({
  view,
  onOpenCommands,
  onOpenView,
}: {
  view: PlusView;
  onOpenCommands: (group?: string) => void;
  onOpenView?: (view: View) => void;
}) {
  const screen = PLUS_SCREENS[view];
  const tabs = NOT_BUILT_TABS[view];
  const [tab, setTab] = useState(tabs?.[0]?.id ?? "");
  const current = tabs?.find((item) => item.id === tab) ?? tabs?.[0];
  if (!tabs || !current) {
    return (
      <NotBuiltPanel
        name={screen.title}
        builtBy={screen.builtBy}
        onOpenCommands={onOpenCommands}
      />
    );
  }
  return (
    <Tabs
      items={tabs.map(({ id, label }) => ({ id, label }))}
      value={current.id}
      onValueChange={setTab}
      label={`${screen.title} sections`}
    >
      <NotBuiltPanel
        name={current.label}
        builtBy={current.builtBy}
        group={current.group}
        opens={
          current.opens && isPlusView(current.opens)
            ? { view: current.opens, title: PLUS_SCREENS[current.opens].title }
            : undefined
        }
        onOpenCommands={onOpenCommands}
        onOpenView={onOpenView}
      />
    </Tabs>
  );
}
