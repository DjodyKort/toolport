import { CtlError, ctlData } from "../bridge/ctl";
import type {
  BundleConfigData,
  BundleLaunchData,
  BundleListData,
  BundleSaveData,
  BundleShowData,
  BundleStatusData,
  CheckpointData,
  ComposeData,
  FolderStatusData,
  LayersData,
  LayersUndoData,
  LaunchProfilesData,
  LaunchProfileWriteData,
  LoadsData,
  MeasureData,
  PlanWriteData,
  PresyncData,
  SyncData,
  WiringStatusData,
  WriteData,
} from "./data";

/** A failed envelope that still carries data (a measure that failed half way) returns that data;
 * one without data throws. */
async function ctlDataOrFailure<T>(argv: string[], signal?: AbortSignal): Promise<T> {
  try {
    return await ctlData<T>(argv, { signal });
  } catch (error) {
    if (error instanceof CtlError && error.data !== undefined && error.data !== null) {
      return error.data as T;
    }
    throw error;
  }
}

export const folderStatus = (cwd: string) =>
  ctlData<FolderStatusData>(["context", "status", "--cwd", cwd]);

export const loadStack = (cwd: string) =>
  ctlData<LoadsData>(["context", "loads", "--cwd", cwd, "--measured"]);

export const composeText = (cwd: string) =>
  ctlData<ComposeData>(["context", "compose", "--cwd", cwd]);

export interface MeasureOptions {
  /** `plugin:<id>` or `skill:<name>`: one more request that leaves the part out. */
  without?: string[];
  bundle?: string;
  /** Spends tokens; without it the command answers with the plan only. */
  start?: boolean;
  force?: boolean;
  signal?: AbortSignal;
}

export function measureFolder(cwd: string, options: MeasureOptions = {}) {
  const argv = [
    "context",
    "measure",
    "--cwd",
    cwd,
    ...(options.without ?? []).flatMap((part) => ["--without", part]),
    ...(options.bundle ? ["--bundle", options.bundle] : []),
    ...(options.force ? ["--force"] : []),
    ...(options.start ? ["--yes"] : []),
  ];
  return ctlDataOrFailure<MeasureData>(argv, options.signal);
}

const dry = (dryRun: boolean) => (dryRun ? ["--dry-run"] : []);

export type LeaveOutTarget = { file: string } | { memory: true };

export function leaveOut(
  action: "add" | "rm",
  target: LeaveOutTarget,
  cwd: string,
  dryRun = false,
) {
  const which = "file" in target ? [target.file] : ["--memory"];
  return ctlData<WriteData>([
    "context",
    "leave-out",
    action,
    ...which,
    "--cwd",
    cwd,
    ...dry(dryRun),
  ]);
}

export function switchPlugin(
  action: "off" | "on",
  id: string,
  cwd: string,
  dryRun = false,
) {
  return ctlData<WriteData>(["plugins", action, id, "--cwd", cwd, ...dry(dryRun)]);
}

export const listBundles = () => ctlData<BundleListData>(["context", "bundle", "ls"]);

export const showBundle = (name: string) =>
  ctlData<BundleShowData>(["context", "bundle", "show", name]);

export const bundleStatus = (cwd: string) =>
  ctlData<BundleStatusData>(["context", "bundle", "status", "--cwd", cwd]);

export const applyBundle = (name: string, cwd: string, dryRun: boolean) =>
  ctlData<WriteData>(["context", "bundle", "apply", name, "--cwd", cwd, ...dry(dryRun)]);

export const undoBundle = (cwd: string, dryRun: boolean) =>
  ctlData<WriteData>(["context", "bundle", "undo", "--cwd", cwd, ...dry(dryRun)]);

export const contextUse = (name: string, cwd: string, dryRun: boolean) =>
  ctlData<WriteData>(["context", "use", name, "--cwd", cwd, ...dry(dryRun)]);

export const stopUsingBundle = (cwd: string, dryRun: boolean) =>
  ctlData<WriteData>(["context", "use", "--none", "--cwd", cwd, ...dry(dryRun)]);

export const launchCommand = (name: string, cwd?: string) =>
  ctlData<BundleLaunchData>([
    "context",
    "bundle",
    "launch",
    name,
    ...(cwd ? ["--cwd", cwd] : []),
  ]);

export const bundleConfig = (autoApply?: boolean) =>
  ctlData<BundleConfigData>([
    "context",
    "bundle",
    "config",
    ...(autoApply === undefined ? [] : ["--auto-apply", autoApply ? "on" : "off"]),
  ]);

export interface BundleFields {
  description: string;
  skillsOff: string;
  pluginsOff: string;
  agentsOff: string;
  layersExclude: string;
  servers: string;
  bind: string;
}

function bundleFlags(fields: BundleFields, edit: boolean) {
  const flags: string[] = [];
  const text = (flag: string, value: string) => {
    if (edit || value.trim()) flags.push(flag, value.trim());
  };
  text("--description", fields.description);
  text("--skills-off", fields.skillsOff);
  text("--plugins-off", fields.pluginsOff);
  text("--agents-off", fields.agentsOff);
  text("--layers-exclude", fields.layersExclude);
  text("--servers", fields.servers);
  text("--bind", fields.bind);
  return flags;
}

export function addBundle(
  name: string,
  fields: BundleFields,
  options: { fromFolder?: string; dryRun?: boolean } = {},
) {
  return ctlData<BundleSaveData>([
    "context",
    "bundle",
    "add",
    name,
    ...(options.fromFolder ? ["--from-folder", options.fromFolder] : []),
    ...bundleFlags(fields, false),
    ...dry(options.dryRun === true),
  ]);
}

export function editBundle(name: string, fields: BundleFields, dryRun = false) {
  return ctlData<BundleSaveData>([
    "context",
    "bundle",
    "edit",
    name,
    ...bundleFlags(fields, true),
    ...dry(dryRun),
  ]);
}

export const removeBundle = (name: string, dryRun: boolean) =>
  ctlData<BundleSaveData>(["context", "bundle", "rm", name, ...dry(dryRun)]);

export const syncContext = (dryRun: boolean, rules = false) =>
  ctlData<SyncData>(["context", "sync", ...dry(dryRun), ...(rules ? ["--rules"] : [])]);

export const listLayers = () => ctlData<LayersData>(["context", "layers", "ls"]);

export const undoLayers = (dryRun: boolean) =>
  ctlData<LayersUndoData>(["context", "layers", "undo", ...dry(dryRun)]);

export const wiringStatus = () =>
  ctlData<WiringStatusData>(["context", "wiring", "status"]);

export const initWiring = (dryRun: boolean) =>
  ctlData<PlanWriteData>(["context", "wiring", "init", ...dry(dryRun)]);

export const disableWiring = (dryRun: boolean) =>
  ctlData<PlanWriteData>(["context", "wiring", "disable", ...dry(dryRun)]);

/** The presync the shell runs before a launch, started by hand; it only runs when due. */
export const runPresync = (force: boolean) =>
  ctlData<PresyncData>(["context", "presync", ...(force ? ["--force"] : [])]);

export const checkpointStatus = (cwd?: string) =>
  ctlData<CheckpointData>([
    "context",
    "checkpoint-status",
    ...(cwd ? ["--cwd", cwd] : []),
  ]);

export const setCheckpoint = (at: number | null, dryRun: boolean) =>
  ctlData<PlanWriteData>([
    "context",
    "checkpoint-config",
    ...(at === null ? ["--off"] : ["--at", String(at)]),
    ...dry(dryRun),
  ]);

export const listLaunchProfiles = (cwd?: string) =>
  ctlData<LaunchProfilesData>([
    "context",
    "launch-profile",
    "ls",
    ...(cwd ? ["--cwd", cwd] : []),
  ]);

export interface LaunchProfileFields {
  /** Access set by name or id; required for a new profile. */
  set?: string;
  denyDirect?: boolean;
}

export const saveLaunchProfile = (
  name: string,
  fields: LaunchProfileFields,
  dryRun: boolean,
) =>
  ctlData<LaunchProfileWriteData>([
    "context",
    "launch-profile",
    "add",
    name,
    ...(fields.set ? ["--set", fields.set] : []),
    ...(fields.denyDirect === undefined
      ? []
      : ["--deny-direct", fields.denyDirect ? "on" : "off"]),
    ...dry(dryRun),
  ]);

export const removeLaunchProfile = (name: string, dryRun: boolean) =>
  ctlData<LaunchProfileWriteData>([
    "context",
    "launch-profile",
    "rm",
    name,
    ...dry(dryRun),
  ]);

export const renderLaunchPolicy = (name: string, cwd: string, dryRun: boolean) =>
  ctlData<LaunchProfileWriteData>([
    "context",
    "launch-profile",
    "policy",
    name,
    "--cwd",
    cwd,
    ...dry(dryRun),
  ]);
