import { useState } from "react";
import { History, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { AsyncView } from "../ui";
import { useHistory } from "./hooks";
import { LogDialog } from "./LogDialog";
import { TRIGGER_LABEL, formatDuration, formatWhen } from "./model";
import { RunBadge } from "./RunView";

/** Every run of every task, newest first, each with its redacted step log. */
export function HistoryTab() {
  const history = useHistory(50);
  const [log, setLog] = useState<string | null>(null);
  return (
    <div className="flex flex-col gap-4">
      <AsyncView
        query={history}
        errorTitle="Couldn't read the run history"
        context="task history"
        isEmpty={(data) => data.runs.length === 0}
        empty={
          <EmptyState
            icon={<History />}
            title="No runs yet"
            description="Every run of a task is kept here with a log in which secret values are replaced."
          />
        }
      >
        {(data) => (
          <>
            <div className="flex justify-end">
              <Button
                variant="ghost"
                size="sm"
                aria-label="Refresh the history"
                onClick={history.reload}
              >
                <RefreshCw /> Refresh
              </Button>
            </div>
            <div className="overflow-x-auto rounded-xl border">
              <table aria-label="Runs" className="w-full border-collapse text-sm">
                <thead>
                  <tr className="text-left text-2xs tracking-[0.06em] text-muted-foreground uppercase">
                    {["Task", "Started", "Took", "Result", "Trigger"].map((name) => (
                      <th key={name} scope="col" className="px-3 py-2 font-medium">
                        {name}
                      </th>
                    ))}
                    <th scope="col" className="px-3 py-2 text-right font-medium">
                      <span className="sr-only">Log</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {data.runs.map((run) => (
                    <tr key={run.id} className="border-t">
                      <th scope="row" className="px-3 py-2 text-left font-semibold">
                        {run.task}
                      </th>
                      <td className="px-3 py-2">{formatWhen(run.startedAt)}</td>
                      <td className="px-3 py-2 font-mono text-xs">
                        {formatDuration(run.durationMs)}
                      </td>
                      <td className="px-3 py-2">
                        <RunBadge status={run.status} />
                      </td>
                      <td className="px-3 py-2 text-muted-foreground">
                        {TRIGGER_LABEL[run.trigger]}
                      </td>
                      <td className="px-3 py-2 text-right">
                        <Button
                          size="xs"
                          variant="outline"
                          aria-label={`Log of ${run.id}`}
                          onClick={() => setLog(run.id)}
                        >
                          Log
                        </Button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p className="text-xs text-muted-foreground">
              Logs are redacted: secret values never appear in a log, a notification or
              this table.
            </p>
          </>
        )}
      </AsyncView>
      {log && <LogDialog runId={log} onClose={() => setLog(null)} />}
    </div>
  );
}
