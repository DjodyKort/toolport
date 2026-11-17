import { CtlError, ctlData } from "../bridge/ctl";
import type {
  BusyData,
  EnableData,
  RemoveData,
  RunPlanData,
  RunStateData,
  SavePlanData,
  TaskDefinition,
  TaskHistoryData,
  TaskListData,
  TaskMissedData,
  TaskRunLogData,
  TaskShowData,
} from "./data";

export const HISTORY_LIMIT = 200;

export const listTasks = () => ctlData<TaskListData>(["task", "ls", "--all"]);

export const showTask = (id: string) => ctlData<TaskShowData>(["task", "show", id]);

export const listRuns = (id?: string, limit = HISTORY_LIMIT) =>
  ctlData<TaskHistoryData>([
    "task",
    "history",
    ...(id ? [id] : []),
    "--limit",
    String(limit),
  ]);

export const runLog = (run: string) =>
  ctlData<TaskRunLogData>(["task", "history", "--run", run]);

export const listMissed = (id?: string) =>
  ctlData<TaskMissedData>(["task", "missed", ...(id ? [id] : [])]);

export const previewRun = (id: string) =>
  ctlData<RunPlanData>(["task", "run", id, "--dry-run"]);

/** A second start comes back as a conflict that names the live run. */
export class AlreadyRunning extends Error {
  constructor(
    message: string,
    readonly busy: BusyData,
  ) {
    super(message);
    this.name = "AlreadyRunning";
  }
}

export async function runTask(id: string): Promise<RunPlanData> {
  try {
    return await ctlData<RunPlanData>(["task", "run", id]);
  } catch (error) {
    if (error instanceof CtlError && error.code === "conflict") {
      const busy = error.data as BusyData | undefined;
      if (busy?.run) throw new AlreadyRunning(error.message, busy);
    }
    throw error;
  }
}

export const resumeRun = (run: string) => ctlData<RunStateData>(["task", "resume", run]);

export const cancelRun = (run: string) => ctlData<RunStateData>(["task", "cancel", run]);

export const setEnabled = (id: string, enabled: boolean) =>
  ctlData<EnableData>(["task", enabled ? "enable" : "disable", id]);

/** The definition travels over stdin; the preview is the same argv with `--dry-run`. */
export function saveTask(
  definition: TaskDefinition,
  create: boolean,
  dryRun: boolean,
): Promise<SavePlanData> {
  return ctlData<SavePlanData>(
    [
      "task",
      create ? "add" : "edit",
      definition.id,
      "--file",
      "-",
      ...(dryRun ? ["--dry-run"] : []),
    ],
    { stdinSecret: JSON.stringify(definition) },
  );
}

export const fromCommand = (id: string, path: string, dryRun: boolean) =>
  ctlData<SavePlanData>([
    "task",
    "add",
    id,
    "--from-command",
    path,
    ...(dryRun ? ["--dry-run"] : []),
  ]);

export const removeTask = (id: string, dryRun: boolean) =>
  ctlData<RemoveData>(["task", "rm", id, ...(dryRun ? ["--dry-run"] : [])]);
