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
  CommitData,
  ConnectData,
  CompareData,
  ContentAddData,
  ContentCleanData,
  ContentDiffData,
  ContentGet,
  FilesData,
  FrontmatterEdit,
  FixData,
  InitData,
  InstallData,
  KeepData,
  LintData,
  MoveLinkData,
  PullData,
  PushData,
  RemoteAddData,
  RemoteStatus,
  RemoveData,
  RootChangeData,
  RootListData,
  SearchData,
  SkillListData,
  SourcesData,
  StatusData,
  StyleApplyData,
  StyleListData,
  StyleStatusData,
  StyleSyncData,
  SyncData,
  ToolCall,
  TapChangeData,
  TapLinkData,
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

export const commitLibrary = (message: string, dryRun: boolean) =>
  dataOrFailure<CommitData>(["library", "commit", "-m", message, ...dry(dryRun)]);
export const connectLibrary = (url: string, cloneTo: string, dryRun: boolean) =>
  ctlData<ConnectData>([
    "library",
    "connect",
    url,
    ...(cloneTo ? ["--clone-to", cloneTo] : []),
    ...dry(dryRun),
  ]);
export const addRemote = (url: string, dryRun: boolean) =>
  ctlData<RemoteAddData>(["library", "remote", "add", url, ...dry(dryRun)]);

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
export const linkTap = (name: string, dryRun: boolean) =>
  ctlData<TapLinkData>(["skills", "tap", "link", name, ...dry(dryRun)]);
export const unlinkTap = (name: string, dryRun: boolean) =>
  ctlData<TapLinkData>(["skills", "tap", "unlink", name, ...dry(dryRun)]);
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
const clientFlags = (clients: readonly string[]) =>
  clients.flatMap((client) => ["--client", client]);

/** Takes the applied style off the given clients, or off every client when none is given. */
export const removeStyle = (dryRun: boolean, clients: readonly string[] = []) =>
  ctlData<RemoveData>(["styles", "remove", ...clientFlags(clients), ...dry(dryRun)]);
/** Applies one style as the always-on rule of the given clients, or of every client without a
 * native style switch when none is given. */
export const applyStyle = (name: string, clients: readonly string[], dryRun: boolean) =>
  ctlData<StyleApplyData>([
    "styles",
    "apply",
    name,
    ...clientFlags(clients),
    ...dry(dryRun),
  ]);
export const styleStatus = () => ctlData<StyleStatusData>(["styles", "status"]);

export type ContentKind = "agents" | "styles";
/** What has a body to edit: the three kinds of library content. */
export type BodyKind = "skills" | ContentKind;

export const addContent = (kind: ContentKind, name: string, dryRun: boolean) =>
  ctlData<ContentAddData>([kind, "add", name, ...dry(dryRun)]);
export const contentDiff = (kind: ContentKind) =>
  dataOrFailure<ContentDiffData>([kind, "diff"]);
export const cleanContent = (kind: ContentKind, dryRun: boolean) =>
  ctlData<ContentCleanData>([kind, "clean", ...dry(dryRun)]);

/** The self-management tools that have no command of their own run through `mcp call`; the
 * arguments travel on stdin, so a body of any size never reaches a command line. */
async function callTool<T>(tool: string, args: Record<string, unknown>): Promise<T> {
  const data = await ctlData<ToolCall<T>>(["mcp", "call", tool, "--args-stdin"], {
    stdinSecret: JSON.stringify(args),
  });
  if (data.isError) {
    const failure = (data.result as { error?: { message?: string } } | null)?.error;
    throw new Error(failure?.message ?? `${tool} failed`);
  }
  return data.result;
}

export const getContent = (kind: BodyKind, name: string) =>
  callTool<ContentGet>(`${kind}_get`, { name });

/** Replaces the body of a skill, an agent or a style in the library; the frontmatter stays. */
export const editContentBody = (kind: BodyKind, name: string, body: string) =>
  callTool<{ sourcePath: string; newHash: string }>(`${kind}_edit_body`, {
    name,
    new_body: body,
    confirm: true,
  });

type FrontmatterPatch = Record<string, unknown>;

/** The fields of a skill as they are in its file, read with an empty patch that writes nothing. */
export const readFrontmatter = (name: string) =>
  callTool<FrontmatterEdit>("skills_edit_frontmatter", {
    name,
    patch: {},
    dry_run: true,
  });

/** What a patch would write, and whether the result is a skill that passes: nothing is written. */
export const planFrontmatter = (name: string, patch: FrontmatterPatch) =>
  callTool<FrontmatterEdit>("skills_edit_frontmatter", { name, patch, dry_run: true });

/** Sets the fields of the patch; `null` removes one. Only their lines in the file change. */
export const editFrontmatter = (name: string, patch: FrontmatterPatch) =>
  callTool<FrontmatterEdit>("skills_edit_frontmatter", {
    name,
    patch,
    confirm: true,
  });
