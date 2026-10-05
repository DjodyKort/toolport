import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { AsyncView, useCtlQuery } from "../ui";
import type { TaskHistoryRunData } from "../types/tasks";
import { RunSteps, RunSummary } from "./RunView";

/** One finished or running run with the redacted log of each step. Secret values are
 * replaced before the runner writes the log, so what shows here is already clean. */
export function LogDialog({ runId, onClose }: { runId: string; onClose: () => void }) {
  const query = useCtlQuery<TaskHistoryRunData>(["task", "history", "--run", runId]);
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent aria-describedby={undefined} className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Log of {runId}</DialogTitle>
        </DialogHeader>
        <div className="flex max-h-[60vh] flex-col gap-3 overflow-y-auto">
          <AsyncView
            query={query}
            errorTitle="Couldn't read the log"
            context="task history"
          >
            {({ run }) => (
              <>
                <RunSummary run={run} />
                <RunSteps run={run} />
                <p className="text-xs text-muted-foreground">
                  Logs are redacted: secret values never appear in a log or a
                  notification.
                </p>
              </>
            )}
          </AsyncView>
        </div>
        <DialogFooter>
          <Button onClick={onClose}>Close</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
