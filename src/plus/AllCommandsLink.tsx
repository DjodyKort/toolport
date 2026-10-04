import { TerminalSquare } from "lucide-react";
import { Button } from "@/components/ui/button";
import { SectionHeader } from "@/components/ui/section-header";

/** The way to the All commands page from Settings: it is not an entry of the sidebar. */
export function AllCommandsLink({ onOpen }: { onOpen: () => void }) {
  return (
    <section aria-label="All commands" className="mt-6">
      <SectionHeader icon={<TerminalSquare className="size-3.5" />}>
        All commands
      </SectionHeader>
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border p-4">
        <p className="min-w-0 text-sm text-muted-foreground">
          Every toolportctl command, generated from the registry. Anything without a
          screen of its own is reachable here.
        </p>
        <Button variant="outline" size="sm" onClick={onOpen}>
          Open All commands
        </Button>
      </div>
    </section>
  );
}
