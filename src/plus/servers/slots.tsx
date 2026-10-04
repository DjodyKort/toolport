import { Hammer } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import type { SlotTab } from "./slotTabs";

export function Slot({
  tab,
  onOpenCommands,
}: {
  tab: SlotTab;
  onOpenCommands: (group?: string) => void;
}) {
  return (
    <EmptyState
      icon={<Hammer />}
      title={
        <span className="inline-flex items-center gap-2">
          {tab.label} <Badge variant="warning">Not built yet</Badge>
        </span>
      }
      description={
        <>
          This tab is built by {tab.builtBy}. Until then everything it will cover is
          available on the All commands page.
        </>
      }
      action={
        <Button variant="outline" size="sm" onClick={() => onOpenCommands(tab.group)}>
          Open All commands
        </Button>
      }
    />
  );
}
