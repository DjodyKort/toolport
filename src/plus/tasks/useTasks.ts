import { useCallback, useEffect, useRef, useState } from "react";
import { isAutostartEnabled } from "@/lib/api";
import { errorText } from "../settings/useCtl";
import { listMissed, listRuns, listTasks } from "./commands";
import { now } from "./clock";
import type { MissedRecord, RunView, TaskListData } from "./data";
import { isLive } from "./model";

export type Autostart = "on" | "off" | "unknown" | "loading";

export interface TasksState {
  list: TaskListData | null;
  runs: RunView[];
  missed: MissedRecord[];
  autostart: Autostart;
  /** Set when the last refresh failed; the last good list stays in `list`. */
  error: string | null;
  updatedAt: number | null;
  reload: () => void;
  retryAutostart: () => void;
}

const SLOW_MS = 30_000;
const LIVE_MS = 1_000;

/** Everything the Tasks page reads. It reloads on focus and every 30 seconds, and once a second
 * while a run is live (the commands have no event stream). A slower, older answer never replaces
 * a newer one. */
export function useTasksState(): TasksState {
  const [state, setState] = useState<Omit<TasksState, "reload" | "retryAutostart">>({
    list: null,
    runs: [],
    missed: [],
    autostart: "loading",
    error: null,
    updatedAt: null,
  });
  const sequence = useRef(0);
  const live = state.runs.some((r) => r.status === "running");

  const load = useCallback(async (full: boolean) => {
    const mine = ++sequence.current;
    try {
      const [list, runs] = await Promise.all([listTasks(), listRuns()]);
      const [missed, autostart] = full
        ? await Promise.all([
            listMissed().then(
              (m) => m.missed,
              () => null,
            ),
            isAutostartEnabled().then(
              (on): Autostart => (on ? "on" : "off"),
              (): Autostart => "unknown",
            ),
          ])
        : [null, null];
      if (mine !== sequence.current) return;
      setState((current) => ({
        list,
        runs: runs.runs,
        missed: missed ?? current.missed,
        autostart: autostart ?? current.autostart,
        error: null,
        updatedAt: now(),
      }));
    } catch (error) {
      if (mine === sequence.current)
        setState((current) => ({ ...current, error: errorText(error) }));
    }
  }, []);

  useEffect(() => {
    void load(true);
    const onFocus = () => void load(true);
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [load]);

  useEffect(() => {
    const timer = setInterval(() => void load(!live), live ? LIVE_MS : SLOW_MS);
    return () => clearInterval(timer);
  }, [live, load]);

  const reload = useCallback(() => void load(true), [load]);
  const retryAutostart = useCallback(() => {
    setState((current) => ({ ...current, autostart: "loading" }));
    void load(true);
  }, [load]);
  return { ...state, reload, retryAutostart };
}

export const liveRunOf = (runs: RunView[], task: string) =>
  runs.find((r) => r.task === task && isLive(r.status));
