import { Hammer } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, Intro } from "./atoms";

/** The Claude Code plugins section of System > Updates. MIG-GUI-12 builds it together with
 * Library > Plugins (D-075), so until then it is a marked placeholder: replace this file. */
export function PluginUpdates({
  onOpenCommands,
}: {
  onOpenCommands: (group?: string) => void;
}) {
  return (
    <Card
      title="Claude Code plugins"
      actions={
        <Badge variant="warning">
          <Hammer /> Not built yet
        </Badge>
      }
    >
      <Intro>
        Plugin updates are listed here with the server updates. MIG-GUI-12 builds this
        section with Library &gt; Plugins. Until then `cc list` and `cc update` are on the
        All commands page.
      </Intro>
      <div>
        <Button size="sm" variant="outline" onClick={() => onOpenCommands("cc")}>
          Open All commands
        </Button>
      </div>
    </Card>
  );
}
