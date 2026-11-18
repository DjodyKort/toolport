import { ctlData } from "../bridge/ctl";
import type {
  SelectLsData,
  SelectPlanData,
  SelectSetData,
  SelectSources,
  SelectUndoData,
  SetChanges,
} from "./selectData";

const SELECT = ["library", "select"];

export const selectSources = () => ctlData<SelectSources>([...SELECT, "ls"]);

export const selectLs = (source: string) =>
  ctlData<SelectLsData>([...SELECT, "ls", source]);

export const selectSet = (source: string, changes: SetChanges) =>
  ctlData<SelectSetData>([
    ...SELECT,
    "set",
    source,
    ...changes.on.flatMap((ref) => ["--on", ref]),
    ...changes.off.flatMap((ref) => ["--off", ref]),
    ...changes.tick.flatMap((ref) => ["--tick-hook", ref]),
  ]);

export const selectPlan = (source: string, switchWhole: boolean) =>
  ctlData<SelectPlanData>([
    ...SELECT,
    "plan",
    source,
    ...(switchWhole ? ["--switch"] : []),
  ]);

export const selectApply = (source: string, hash: string, switchWhole: boolean) =>
  ctlData<SelectPlanData>([
    ...SELECT,
    "apply",
    source,
    "--plan-hash",
    hash,
    ...(switchWhole ? ["--switch"] : []),
  ]);

export const selectUndo = (source: string) =>
  ctlData<SelectUndoData>([...SELECT, "undo", source]);
