import { useCallback, useMemo, useState } from "react";
import { skillsInventory } from "@/lib/api";
import type { SkillsInventory } from "@/lib/types";
import { CtlError } from "../bridge/ctl";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import {
  autoStatus,
  listLibrary,
  lintSkills,
  remoteStatus,
  syncPreview,
  syncStatus,
  undoSync,
} from "./commands";
import type {
  AutoStatus,
  LibrarySkill,
  LintData,
  RemoteStatus,
  StatusData,
  SyncData,
  UndoData,
} from "./data";
import { heldBack, previewArgv, tapSkillNames, type HeldBack } from "./model";

export type SyncOutcome =
  { kind: "ok"; data: SyncData } | { kind: "error"; code: string; message: string };

/** The dry run as one value, so a failure that means something (no library, another run, a folder
 * that cannot be written) is told apart from one that does not. */
export async function previewOutcome(argv: string[]): Promise<SyncOutcome> {
  try {
    return { kind: "ok", data: await syncPreview(argv) };
  } catch (error) {
    if (error instanceof CtlError) {
      return { kind: "error", code: error.code, message: error.message };
    }
    return { kind: "error", code: "bridge", message: errorText(error) };
  }
}

async function inventory(): Promise<SkillsInventory | null> {
  try {
    return await skillsInventory(null);
  } catch {
    return null;
  }
}

async function undoSnapshot(): Promise<UndoData | null> {
  try {
    return await undoSync(true);
  } catch {
    return null;
  }
}

async function optional<T>(load: () => Promise<T>): Promise<T | null> {
  try {
    return await load();
  } catch {
    return null;
  }
}

export interface LibraryData {
  outcome: SyncOutcome | null;
  /** What the next auto-sync run would hold back; null when auto-sync is off or paused. */
  autoHeld: SyncData | null;
  lint: LintData | null;
  lintError: string | null;
  status: StatusData | null;
  remote: RemoteStatus | null;
  auto: AutoStatus | null;
  skills: LibrarySkill[];
  inventory: SkillsInventory | null;
  undo: UndoData | null;
  held: HeldBack[];
  tapSkills: ReadonlySet<string>;
  reload: () => void;
  reloadRemote: (fetch?: boolean) => void;
  reloadLint: () => void;
}

/** Everything the Sync tab reads. The pieces load apart and fill in as they finish; the plan is
 * the one the page waits for. */
export function useLibraryData(clients: readonly string[] | null): LibraryData {
  const [fetchTick, setFetchTick] = useState(0);
  const key = (clients ?? []).join(",");
  const outcome = useLoad(() => previewOutcome(previewArgv(clients)), key);
  const auto = useLoad(() => optional(autoStatus), "auto");
  const lint = useLoad(async () => {
    try {
      return { data: await lintSkills(), error: null };
    } catch (error) {
      return { data: null, error: errorText(error) };
    }
  }, "lint");
  const status = useLoad(() => optional(syncStatus), "status");
  const remote = useLoad(() => optional(() => remoteStatus(fetchTick > 0)), fetchTick);
  const skills = useLoad(() => optional(listLibrary), "skills");
  const inv = useLoad(inventory, "inventory");
  const undo = useLoad(undoSnapshot, "undo");
  const autoRunning =
    auto.data !== null && !auto.data.paused && auto.data.schedule?.enabled === true;
  const autoHeld = useLoad(async () => {
    if (!autoRunning) return null;
    const result = await previewOutcome(["skills", "sync", "--dry-run", "--unattended"]);
    return result.kind === "ok" && result.data.held.length > 0 ? result.data : null;
  }, `${autoRunning}`);

  const reloadOutcome = outcome.reload;
  const reloadAuto = auto.reload;
  const reloadLintLoad = lint.reload;
  const reloadStatus = status.reload;
  const reloadRemoteLoad = remote.reload;
  const reloadSkills = skills.reload;
  const reloadInventory = inv.reload;
  const reloadUndo = undo.reload;
  const reloadAutoHeld = autoHeld.reload;

  const reload = useCallback(() => {
    reloadOutcome();
    reloadAuto();
    reloadLintLoad();
    reloadStatus();
    reloadRemoteLoad();
    reloadSkills();
    reloadInventory();
    reloadUndo();
    reloadAutoHeld();
  }, [
    reloadOutcome,
    reloadAuto,
    reloadLintLoad,
    reloadStatus,
    reloadRemoteLoad,
    reloadSkills,
    reloadInventory,
    reloadUndo,
    reloadAutoHeld,
  ]);
  const reloadRemote = useCallback(
    (fetch = false) => (fetch ? setFetchTick((n) => n + 1) : reloadRemoteLoad()),
    [reloadRemoteLoad],
  );

  const skillList = useMemo(() => skills.data?.skills ?? [], [skills.data]);
  const held = useMemo(() => heldBack(lint.data?.data ?? null), [lint.data]);
  const tapSkills = useMemo(() => tapSkillNames(skillList), [skillList]);

  return {
    outcome: outcome.data,
    autoHeld: autoHeld.data,
    lint: lint.data?.data ?? null,
    lintError: lint.data?.error ?? null,
    status: status.data,
    remote: remote.data,
    auto: auto.data,
    skills: skillList,
    inventory: inv.data,
    undo: undo.data,
    held,
    tapSkills,
    reload,
    reloadRemote,
    reloadLint: reloadLintLoad,
  };
}
