import { useId, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { hookScript } from "./selectModel";

export interface HookView {
  id: string;
  event: string;
  matcher: string | null;
  command: string;
  was?: string | null;
}

/** A hook runs a command in every session, so it is read here and ticked on its own. `readOnly`
 * shows the command without the tick, for a hook that can only be ticked after the plan applies. */
export default function HookReview({
  hook,
  readOnly,
  onCancel,
  onConfirm,
}: {
  hook: HookView;
  readOnly?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const [read, setRead] = useState(false);
  const tickId = useId();
  const script = hookScript(hook.command);
  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Review hook</DialogTitle>
          <DialogDescription>
            Each hook is ticked on its own and loses its tick when its command changes.
          </DialogDescription>
        </DialogHeader>
        <dl className="grid grid-cols-[5.5rem_minmax(0,1fr)] gap-x-3 gap-y-1.5 text-meta">
          <dt className="text-muted-foreground">Event</dt>
          <dd className="font-mono text-xs">{hook.event}</dd>
          <dt className="text-muted-foreground">Matcher</dt>
          <dd className="font-mono text-xs">{hook.matcher ?? "any"}</dd>
          <dt className="text-muted-foreground">Command</dt>
          <dd className="font-mono text-xs break-all">{hook.command}</dd>
          {hook.was && (
            <>
              <dt className="text-muted-foreground">Was</dt>
              <dd className="font-mono text-xs break-all">{hook.was}</dd>
            </>
          )}
          {script && (
            <>
              <dt className="text-muted-foreground">Script</dt>
              <dd className="font-mono text-xs break-all">{script}</dd>
            </>
          )}
        </dl>
        {readOnly ? (
          <p className="text-meta text-muted-foreground">
            The old command keeps running until you apply this plan and tick the hook
            again.
          </p>
        ) : (
          <label htmlFor={tickId} className="flex items-start gap-2 text-meta">
            <input
              id={tickId}
              type="checkbox"
              className="mt-0.5"
              checked={read}
              onChange={(event) => setRead(event.target.checked)}
            />
            I have read this hook and want it to run in my sessions.
          </label>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            {readOnly ? "Close" : "Cancel"}
          </Button>
          {!readOnly && (
            <Button disabled={!read} onClick={onConfirm}>
              Turn on hook
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
