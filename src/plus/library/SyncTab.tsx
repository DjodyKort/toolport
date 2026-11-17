import { useRef, useState } from "react";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import ChecksPanel from "./ChecksPanel";
import { autoSet, syncRun, verifySync } from "./commands";
import type { SyncData, VerifyData } from "./data";
import ItemsList, { Targets } from "./ItemsList";
import {
  applyChoice,
  headline,
  libraryItems,
  remoteState,
  rowCount,
  tickNeeds,
} from "./model";
import PlanPanel from "./PlanPanel";
import StatusArea from "./StatusArea";
import { useLibraryData } from "./useLibrary";
import { InitDialog, PullDialog, PushDialog, UndoDialog } from "./writes";

type Dialog = "undo" | "pull" | "push" | "init" | null;

export default function SyncTab({
  onOpenSkill,
}: {
  onOpenSkill: (name: string) => void;
}) {
  const [clients, setClients] = useState<string[] | null>(null);
  const lib = useLibraryData(clients);
  const [ticked, setTicked] = useState(false);
  const [applying, setApplying] = useState<number | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [done, setDone] = useState<SyncData | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [checked, setChecked] = useState<VerifyData | null>(null);
  const plan = useRef<HTMLDivElement>(null);

  const sync = lib.outcome?.kind === "ok" ? lib.outcome.data : null;
  const state = remoteState(lib.remote);
  const needs = sync ? tickNeeds(sync, lib.tapSkills) : { differs: 0, tapHooks: 0 };
  const choice = sync ? applyChoice(sync, needs, ticked, clients) : null;
  const head = sync
    ? headline(sync, lib.held, state, lib.auto, lib.status?.lockfilePresent ?? true)
    : null;
  const canUndo = lib.undo !== null;

  async function apply() {
    if (!choice) return;
    setApplying(choice.count);
    setFailed(null);
    setDone(null);
    try {
      setDone(await syncRun(choice.argv));
    } catch (error) {
      setFailed(errorText(error));
    } finally {
      setApplying(null);
      setTicked(false);
      lib.reload();
    }
  }

  async function auto(action: "pause" | "resume") {
    try {
      await autoSet(action);
    } catch (error) {
      toastError("Could not change auto-sync", { description: errorText(error) });
    }
    lib.reload();
  }

  const fresh = () => {
    setDone(null);
    setFailed(null);
    setChecked(null);
    lib.reload();
  };

  async function check() {
    fresh();
    try {
      setChecked(await verifySync());
    } catch (error) {
      toastError("Could not verify the files", { description: errorText(error) });
    }
  }

  return (
    <div className="@container flex flex-col gap-4">
      <StatusArea
        outcome={lib.outcome}
        headline={head}
        remote={lib.remote}
        auto={lib.auto}
        autoHeld={lib.autoHeld}
        done={done}
        applying={applying}
        failed={failed}
        applyCount={choice?.count ?? 0}
        needs={needs}
        ticked={ticked}
        canUndo={canUndo}
        lockfileSyncedAt={lib.status?.lockfileSyncedAt ?? null}
        onTick={setTicked}
        onApply={() => void apply()}
        onPull={() => setDialog("pull")}
        onPush={() => setDialog("push")}
        onFetch={() => lib.reloadRemote(true)}
        onUndo={() => setDialog("undo")}
        onRetry={fresh}
        onReload={() => void check()}
        verified={checked}
        onAuto={(action) => void auto(action)}
        onInit={() => setDialog("init")}
        onReview={() => plan.current?.scrollIntoView?.({ behavior: "smooth" })}
      />
      {sync && (
        <>
          <div ref={plan}>
            <PlanPanel
              sync={sync}
              held={lib.held}
              tapSkills={lib.tapSkills}
              canUndo={canUndo}
              onUndo={() => setDialog("undo")}
              onOpenSkill={onOpenSkill}
            />
          </div>
          <ChecksPanel
            lint={lib.lint}
            lintError={lib.lintError}
            sync={sync}
            onOpenSkill={onOpenSkill}
            onReload={lib.reloadLint}
            onChanged={fresh}
          />
          <Targets
            targeted={sync.targetedClients}
            pending={rowCount(sync.plan.changes)}
            onChange={(next) => {
              setDone(null);
              setClients(next);
            }}
          />
          <ItemsList
            items={libraryItems(lib.skills, lib.status, sync, lib.held, lib.inventory)}
            onOpen={onOpenSkill}
          />
        </>
      )}
      {dialog === "undo" && <UndoDialog onClose={() => setDialog(null)} onDone={fresh} />}
      {dialog === "pull" && <PullDialog onClose={() => setDialog(null)} onDone={fresh} />}
      {dialog === "push" && (
        <PushDialog onClose={() => setDialog(null)} onDone={() => lib.reloadRemote()} />
      )}
      {dialog === "init" && <InitDialog onClose={() => setDialog(null)} onDone={fresh} />}
    </div>
  );
}
