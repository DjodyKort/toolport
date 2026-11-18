import { ctlData } from "../bridge/ctl";
import type {
  ConfigSetData,
  ExportCsvData,
  PruneData,
  RangeId,
  SetupApplyData,
  SetupApplyDryRunData,
  SetupPlanData,
  SetupUndoData,
  SetupUndoDryRunData,
  StatusData,
  SummaryData,
} from "./data";

export interface SetupChoice {
  port?: number;
  /** Env keys that may be overwritten, or "all". */
  overwrite?: string[];
  /** Settings files (or their folders) to write; empty means every detected profile. */
  profiles?: string[];
}

function setupFlags(choice: SetupChoice): string[] {
  return [
    ...(choice.port === undefined ? [] : ["--port", String(choice.port)]),
    ...(choice.overwrite ?? []).flatMap((key) => ["--overwrite", key]),
    ...(choice.profiles ?? []).flatMap((profile) => ["--profile", profile]),
  ];
}

export const spendStatus = (signal?: AbortSignal) =>
  ctlData<StatusData>(["spend", "status"], { signal });

/** `project` narrows the report to one folder, named as the by-project rows name it. */
export const spendSummary = (
  range: RangeId,
  project?: string | null,
  signal?: AbortSignal,
) =>
  ctlData<SummaryData>(
    ["spend", "summary", "--range", range, ...(project ? ["--project", project] : [])],
    { signal },
  );

export const setupPlan = (choice: SetupChoice = {}) =>
  ctlData<SetupPlanData>(["spend", "setup", "plan", ...setupFlags(choice)]);

export const setupApply = (choice: SetupChoice = {}) =>
  ctlData<SetupApplyData>(["spend", "setup", "apply", ...setupFlags(choice)]);

export const setupApplyDryRun = (choice: SetupChoice = {}) =>
  ctlData<SetupApplyDryRunData>([
    "spend",
    "setup",
    "apply",
    "--dry-run",
    ...setupFlags(choice),
  ]);

export interface UndoChoice {
  /** Settings files (or their folders) to undo; empty means every profile Toolport set up. */
  profiles?: string[];
  /** Also delete the collected events and daily totals; only valid when no profile stays set up. */
  deleteData?: boolean;
}

const undoFlags = (choice: UndoChoice) => [
  ...(choice.profiles ?? []).flatMap((profile) => ["--profile", profile]),
  ...(choice.deleteData ? ["--delete-data"] : []),
];

export const setupUndoPreview = (choice: UndoChoice = {}) =>
  ctlData<SetupUndoDryRunData>([
    "spend",
    "setup",
    "undo",
    "--dry-run",
    ...undoFlags(choice),
  ]);

/** Deleting data needs the confirmation `--yes`, which the Undo dialog collects first. */
export const setupUndo = (choice: UndoChoice = {}) =>
  ctlData<SetupUndoData>([
    "spend",
    "setup",
    "undo",
    ...undoFlags(choice),
    ...(choice.deleteData ? ["--yes"] : []),
  ]);

export const prunePreview = () => ctlData<PruneData>(["spend", "prune", "--dry-run"]);

export const pruneNow = () => ctlData<PruneData>(["spend", "prune"]);

/** Deletes the daily totals that survive pruning, for good. */
export const deleteTotals = () =>
  ctlData<PruneData>(["spend", "prune", "--totals", "--yes"]);

export interface ConfigEdit {
  port?: number;
  retentionDays?: number;
  maxMb?: number;
  showCost?: boolean;
}

export function setConfig(edit: ConfigEdit) {
  const argv = ["spend", "config", "set"];
  if (edit.port !== undefined) argv.push("--port", String(edit.port));
  if (edit.retentionDays !== undefined)
    argv.push("--retention-days", String(edit.retentionDays));
  if (edit.maxMb !== undefined) argv.push("--max-mb", String(edit.maxMb));
  if (edit.showCost !== undefined) argv.push("--show-cost", edit.showCost ? "on" : "off");
  return ctlData<ConfigSetData>(argv);
}

/** Without `out` the CSV text comes back; with it the file is written. */
export const exportCsv = (range: RangeId, out?: string) =>
  ctlData<ExportCsvData>([
    "spend",
    "export-csv",
    "--range",
    range,
    ...(out ? ["--out", out] : []),
  ]);
