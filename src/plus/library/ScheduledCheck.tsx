import { useLoad } from "../servers/useLoad";
import { listTasks } from "../tasks/commands";
import { now } from "../tasks/clock";
import { ago, ms, scheduleText } from "../tasks/model";

const CHECK = "selective-update-check";

/** The one scheduled job that looks for updates: its schedule, when it last ran and how it ended.
 * It only fetches; nothing is updated until you press Update. */
export function ScheduledCheck() {
  const tasks = useLoad(listTasks, "scheduled-check");
  const row = tasks.data?.tasks.find((task) => task.id === CHECK);
  if (!row) return null;
  const cron = row.triggers.schedule?.cron;
  const next = ms(row.nextRun);
  const last = row.lastRun;
  const startedAt = ms(last?.startedAt);
  return (
    <p className="text-meta text-muted-foreground">
      {row.enabled && cron ? (
        <>
          Updates are looked for on a schedule ({scheduleText(cron, next)}) by the task{" "}
          <span className="font-medium text-foreground">{row.title}</span>
        </>
      ) : (
        <>
          The scheduled check{" "}
          <span className="font-medium text-foreground">{row.title}</span> is off, so
          updates are only looked for when you press Check for updates
        </>
      )}
      {last && startedAt !== null
        ? `; last run ${ago(startedAt, now())}, ${last.status}.`
        : row.enabled
          ? "; it has not run yet."
          : "."}{" "}
      It fetches only; nothing is updated until you press Update.
    </p>
  );
}
