import { ctlData } from "../bridge/ctl";
import type {
  SelectCheckData,
  SelectLsData,
  SelectPlanData,
  SelectSetData,
  SelectSources,
  SelectUndoData,
  SetChanges,
} from "./selectData";
import { escapeCommas } from "./selectModel";

const SELECT = ["library", "select"];

export const selectSources = () => ctlData<SelectSources>([...SELECT, "ls"]);

export const selectLs = (source: string) =>
  ctlData<SelectLsData>([...SELECT, "ls", source]);

export const selectSet = (source: string, changes: SetChanges) =>
  ctlData<SelectSetData>([
    ...SELECT,
    "set",
    source,
    ...changes.on.flatMap((ref) => ["--on", escapeCommas(ref)]),
    ...changes.off.flatMap((ref) => ["--off", escapeCommas(ref)]),
    ...changes.tick.flatMap((ref) => ["--tick-hook", escapeCommas(ref)]),
  ]);

/** Downloads the whole history of the source once and records whether the pinned commit is still
 * in it. Only ever sent from a button the person pressed. */
export const selectCheckHistory = (source: string) =>
  ctlData<SelectCheckData>([...SELECT, "check", source, "--deepen"]);

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
