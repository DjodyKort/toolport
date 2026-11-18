import { ctlData } from "../bridge/ctl";
import type {
  MemoryClearData,
  MemoryLsData,
  MemorySetData,
  MemoryStatusData,
  OverviewData,
  RetentionData,
  SessionsClearData,
  SessionsLsData,
  SwitchPosition,
  UndoData,
} from "./data";

const dry = (dryRun: boolean) => (dryRun ? ["--dry-run"] : []);

export const memoryStatus = (cwd: string) =>
  ctlData<MemoryStatusData>(["memory", "status", "--cwd", cwd]);

export const memoryFiles = (cwd: string) =>
  ctlData<MemoryLsData>(["memory", "ls", "--cwd", cwd]);

export const setMemory = (
  position: SwitchPosition,
  target: { cwd: string } | { global: true },
  dryRun = false,
) =>
  ctlData<MemorySetData>([
    "memory",
    "set",
    position,
    ...("global" in target ? ["--global"] : ["--cwd", target.cwd]),
    ...dry(dryRun),
  ]);

export interface ClearOptions {
  forever?: boolean;
  confirm?: string;
  alsoOff?: boolean;
  dryRun?: boolean;
}

export const clearMemory = (cwd: string, o: ClearOptions = {}) =>
  ctlData<MemoryClearData>([
    "memory",
    "clear",
    "--cwd",
    cwd,
    ...(o.forever ? ["--forever", "--confirm", o.confirm ?? ""] : []),
    ...(o.alsoOff ? ["--also-off"] : []),
    ...dry(o.dryRun ?? false),
  ]);

export const undoMemory = (cwd: string, id?: string, dryRun = false) =>
  ctlData<UndoData>([
    "memory",
    "undo",
    ...(id ? [id] : []),
    "--cwd",
    cwd,
    ...dry(dryRun),
  ]);

export const sessionsOf = (cwd: string) =>
  ctlData<SessionsLsData>(["sessions", "ls", "--cwd", cwd]);

export interface ClearSessionsOptions {
  olderThan: number;
  to: "trash" | "archive";
  dryRun?: boolean;
}

export const clearSessions = (cwd: string, o: ClearSessionsOptions) =>
  ctlData<SessionsClearData>([
    "sessions",
    "clear",
    "--cwd",
    cwd,
    "--older-than",
    String(o.olderThan),
    "--to",
    o.to,
    ...dry(o.dryRun ?? false),
  ]);

export const undoSessions = (cwd: string, id?: string, dryRun = false) =>
  ctlData<UndoData>([
    "sessions",
    "undo",
    ...(id ? [id] : []),
    "--cwd",
    cwd,
    ...dry(dryRun),
  ]);

export const restoreSessions = (archiveId: string, dryRun = false) =>
  ctlData<UndoData>(["sessions", "restore", archiveId, ...dry(dryRun)]);

export const retentionOf = () => ctlData<RetentionData>(["sessions", "retention"]);

export const memoryOverview = (filter?: string, sort = "name") =>
  ctlData<OverviewData>([
    "memory",
    "overview",
    "--all",
    "--sort",
    sort,
    ...(filter ? ["--filter", filter] : []),
  ]);
