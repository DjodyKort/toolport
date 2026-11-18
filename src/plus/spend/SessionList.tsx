import type { SummaryData } from "./data";
import { sessionLines } from "./model";
import { KindBadge } from "./KindBadge";
import { Panel } from "./Panel";

export interface SessionListProps {
  summary: SummaryData;
  showCost: boolean;
  className?: string;
}

/** The sessions of the filtered page, largest first. */
export function SessionList({ summary, showCost, className }: SessionListProps) {
  const list = summary.sessionList ?? [];
  const lines = sessionLines(list, showCost);
  const total = summary.totals.sessions;
  return (
    <Panel
      title={summary.project ? "Largest sessions in this folder" : "Largest sessions"}
      badge={summary.totals.cost.badge === "Estimate" ? "Estimate" : "Measured"}
      className={className}
      footer={`${list.length} of ${total} ${total === 1 ? "session" : "sessions"}, largest first.`}
    >
      {lines.length === 0 ? (
        <p className="p-4 text-meta text-muted-foreground">
          No session in this range matches the filter.
        </p>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-meta" data-slot="session-table">
            <thead>
              <tr className="text-left text-muted-foreground">
                <th scope="col" className="px-4 py-2 font-medium">
                  Started
                </th>
                <th scope="col" className="px-2 py-2 font-medium">
                  Task
                </th>
                <th scope="col" className="px-2 py-2 font-medium">
                  Model
                </th>
                <th scope="col" className="px-2 py-2 text-right font-medium">
                  Length
                </th>
                <th scope="col" className="px-2 py-2 text-right font-medium">
                  Tokens
                </th>
                {showCost && (
                  <th scope="col" className="px-4 py-2 text-right font-medium">
                    Cost
                  </th>
                )}
              </tr>
            </thead>
            <tbody>
              {lines.map((line) => (
                <tr key={line.id} className="border-t">
                  <td className="px-4 py-2 whitespace-nowrap">{line.started}</td>
                  <td className="px-2 py-2">
                    {line.task ? (
                      <span className="rounded-md bg-muted px-1.5 py-0.5 font-mono text-xs">
                        {line.task}
                      </span>
                    ) : (
                      <span className="text-muted-foreground">{line.taskLabel}</span>
                    )}
                  </td>
                  <td className="px-2 py-2">{line.model}</td>
                  <td className="px-2 py-2 text-right whitespace-nowrap tabular-nums">
                    {line.length}
                  </td>
                  <td className="px-2 py-2 text-right tabular-nums">{line.tokens}</td>
                  {showCost && (
                    <td className="px-4 py-2 text-right font-semibold whitespace-nowrap tabular-nums">
                      {line.cost}
                      {line.badge === "Estimate" && (
                        <KindBadge kind="Estimate" className="ml-1.5" />
                      )}
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </Panel>
  );
}
