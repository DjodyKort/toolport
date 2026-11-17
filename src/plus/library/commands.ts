import { CtlError, ctlData } from "../bridge/ctl";
import type {
  AddData,
  AgentLintData,
  AgentListData,
  AgentStatusData,
  AgentSyncData,
  AuditData,
  AutoStatus,
  AutoSwitchData,
  BundleData,
  CleanData,
  CompareData,
  FilesData,
  FixData,
  InitData,
  InstallData,
  KeepData,
  LintData,
  MoveLinkData,
  PullData,
  PushData,
  RemoteStatus,
  RemoveData,
  RootChangeData,
  RootListData,
  SearchData,
  SkillListData,
  SourcesData,
  StatusData,
  StyleListData,
  StyleSyncData,
  SyncData,
  TapChangeData,
  TapListData,
  TapUpdateData,
  UndoData,
  UninstallData,
  UnbundleData,
  VerifyData,
} from "./data";

/** A failed envelope that still carries data (lint with errors, verify with a mismatch) returns
 * that data; one without data throws. */
async function dataOrFailure<T>(argv: string[]): Promise<T> {
  try {
    return await ctlData<T>(argv);
  } catch (error) {
    if (error instanceof CtlError && error.data !== undefined && error.data !== null) {
      return error.data as T;
    }
    throw error;
  }
}

const dry = (dryRun: boolean) => (dryRun ? ["--dry-run"] : []);

export const syncPreview = (argv: string[]) => ctlData<SyncData>(argv);
export const syncRun = (argv: string[]) => ctlData<SyncData>(argv);

export const syncStatus = () => ctlData<StatusData>(["skills", "status"]);
export const lintSkills = () => dataOrFailure<LintData>(["skills", "lint"]);
export const auditSkills = () => dataOrFailure<AuditData>(["skills", "audit"]);
export const listLibrary = () =>
  ctlData<SkillListData>(["skills", "ls", "--source", "library"]);
export const skillFiles = (name: string) => ctlData<FilesData>(["skills", "files", name]);

export const compareSkill = (name: string, file?: string) =>
  ctlData<CompareData>(["skills", "diff", name, ...(file ? ["--file", file] : [])]);
export const keepFiles = (name: string, files: string[], dryRun = false) =>
  ctlData<KeepData>(["skills", "keep", name, ...files, ...dry(dryRun)]);
/** Replaces one held file with the library copy (a backup is kept); nothing else is synced. */
export const takeLibraryCopy = (name: string, file: string) =>
  ctlData<SyncData>(["skills", "sync", "--only", name, "--take", `${name}/${file}`]);

export const autoStatus = () => ctlData<AutoStatus>(["skills", "auto", "status"]);
export const autoSet = (action: "pause" | "resume") =>
  ctlData<AutoStatus>(["skills", "auto", action]);
export const autoSwitch = (on: boolean) =>
  ctlData<AutoSwitchData>(["skills", "auto", on ? "on" : "off"]);

export const undoSync = (dryRun: boolean) =>
  ctlData<UndoData>(["skills", "undo", ...dry(dryRun)]);
export const verifySync = () => dataOrFailure<VerifyData>(["skills", "verify"]);

export const fixChmod = (skill: string | null, dryRun: boolean) =>
  ctlData<FixData>(["skills", "fix", "chmod", ...(skill ? [skill] : []), ...dry(dryRun)]);

export const moveLink = (skill: string, link: string, dryRun: boolean) =>
  ctlData<MoveLinkData>(["skills", "fix", "move-link", skill, link, ...dry(dryRun)]);

export const resolveCollisions = (migrate: boolean, dryRun: boolean) =>
  ctlData<SyncData>([
    "skills",
    "resolve",
    migrate ? "--migrate" : "--no-migrate",
    ...dry(dryRun),
  ]);

export const cleanSynced = (dryRun: boolean) =>
  ctlData<CleanData>(["skills", "clean", ...dry(dryRun)]);

export const uninstallSkill = (name: string, dryRun: boolean) =>
  ctlData<UninstallData>(["skills", "uninstall", name, ...dry(dryRun)]);

export const addSkill = (name: string, type: "skill" | "rule", dryRun: boolean) =>
  ctlData<AddData>([
    "skills",
    "add",
    name,
    ...(type === "rule" ? ["--type", "rule"] : []),
    ...dry(dryRun),
  ]);

export const initLibrary = (path: string, name: string, dryRun: boolean) =>
  ctlData<InitData>([
    "skills",
    "init",
    ...(path ? ["--path", path] : []),
    ...(name ? ["--name", name] : []),
    ...dry(dryRun),
  ]);

export const bundleExport = (dryRun: boolean) =>
  ctlData<BundleData>(["skills", "bundle", ...dry(dryRun)]);
export const bundleImport = (zip: string, dryRun: boolean) =>
  ctlData<UnbundleData>(["skills", "unbundle", zip, ...dry(dryRun)]);

export const remoteStatus = (fetch = false) =>
  ctlData<RemoteStatus>(["library", "status", ...(fetch ? ["--fetch"] : [])]);
export const pullLibrary = (dryRun: boolean) =>
  ctlData<PullData>(["library", "pull", ...dry(dryRun)]);
export const pushLibrary = (dryRun: boolean) =>
  dataOrFailure<PushData>(["library", "push", ...dry(dryRun)]);

export const listTaps = () => ctlData<TapListData>(["skills", "tap", "ls"]);
export const addTap = (repo: string, dryRun: boolean) =>
  ctlData<TapChangeData>(["skills", "tap", "add", repo, ...dry(dryRun)]);
export const removeTap = (name: string, dryRun: boolean) =>
  ctlData<TapChangeData>(["skills", "tap", "remove", name, ...dry(dryRun)]);
export const updateTaps = (name?: string, dryRun = false) =>
  ctlData<TapUpdateData>([
    "skills",
    "tap",
    "update",
    ...(name ? [name] : []),
    ...dry(dryRun),
  ]);
export const searchTaps = (query: string) =>
  ctlData<SearchData>(["skills", "search", query]);
export const installSkill = (spec: string, dryRun: boolean) =>
  ctlData<InstallData>(["skills", "install", spec, ...dry(dryRun)]);

export const listSources = () => ctlData<SourcesData>(["sources", "ls"]);
export const listRoots = () => ctlData<RootListData>(["sources", "root", "ls"]);
export const addRoot = (path: string, dryRun: boolean) =>
  ctlData<RootChangeData>(["sources", "root", "add", path, ...dry(dryRun)]);
export const removeRoot = (path: string, dryRun: boolean) =>
  ctlData<RootChangeData>(["sources", "root", "rm", path, ...dry(dryRun)]);

export const listAgents = () => ctlData<AgentListData>(["agents", "ls"]);
export const agentStatus = () => ctlData<AgentStatusData>(["agents", "status"]);
export const agentLint = () => dataOrFailure<AgentLintData>(["agents", "lint"]);
export const agentAudit = () => dataOrFailure<AuditData>(["agents", "audit"]);
export const syncAgents = (dryRun: boolean) =>
  ctlData<AgentSyncData>(["agents", "sync", ...dry(dryRun)]);
export const uninstallAgent = (name: string, dryRun: boolean) =>
  ctlData<UninstallData>(["agents", "uninstall", name, ...dry(dryRun)]);

export const listStyles = () => ctlData<StyleListData>(["styles", "ls"]);
export const styleLint = () => dataOrFailure<AgentLintData>(["styles", "lint"]);
export const syncStyles = (dryRun: boolean) =>
  ctlData<StyleSyncData>(["styles", "sync", ...dry(dryRun)]);
export const removeStyle = (dryRun: boolean) =>
  ctlData<RemoveData>(["styles", "remove", ...dry(dryRun)]);
export const styleStatus = () =>
  ctlData<{ lockfilePresent: boolean }>(["styles", "status"]);
