import { arr, obj, opt, type Shape } from "../bridge/shape";
import {
  taskLsData,
  taskRun,
  taskRunData,
  taskRunStateData,
  taskShowData,
} from "./tasks";

/** Results of the task self-MCP tools, checked against the golden results by `selfmcp.test.ts`.
 * Each answers with the same report as its `toolportctl task` command. */

const tasksHistoryData = obj({ runs: opt(arr(taskRun)), run: opt(taskRun) });

/** Tool name to the shape of its `structuredContent`. */
export const tasksToolShapes: Record<string, Shape<unknown>> = {
  tasks_list: taskLsData,
  tasks_get: taskShowData,
  tasks_history: tasksHistoryData,
  tasks_run: taskRunData,
  tasks_cancel: taskRunStateData,
};
