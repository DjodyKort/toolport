import { useCtlQuery } from "../ui";
import type { TaskLsData } from "../types/tasks";
import { refreshTaskFor, type LsTask } from "./model";

/** The tasks that renew a login, for the Logins table. The read is best effort: when it fails
 * there is simply no Refresh task action, and the table says nothing about it. */
export function useRefreshTasks(): (server: string) => LsTask | null {
  const query = useCtlQuery<TaskLsData>(["task", "ls"]);
  const tasks = query.status === "ready" ? (query.data?.tasks ?? null) : null;
  return (server) => refreshTaskFor(tasks, server);
}
