import { Puzzle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { EmptyState } from "@/components/ui/empty-state";

/** The plugins slice is not ported: the tab says so instead of showing data it cannot read. */
export default function PluginsTab() {
  return (
    <EmptyState
      icon={<Puzzle />}
      title={
        <span className="inline-flex items-center gap-2">
          Plugins <Badge variant="info">Planned</Badge>
        </span>
      }
      description="Installed Claude Code plugins, their marketplaces and updates arrive in a later slice. Turning a plugin off in one folder works today in Context."
    />
  );
}
