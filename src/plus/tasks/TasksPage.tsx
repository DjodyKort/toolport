import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { AttentionBanner } from "../attention/AttentionBanner";
import { now } from "./clock";
import { ms } from "./model";
import TaskDetail from "./TaskDetail";
import { duplicateOf } from "./draft";
import { FromCommand, TaskForm, type FormMode } from "./TaskForm";
import { Banners, HistoryTab, TasksTab, type ListActions } from "./TasksList";
import { useTasksState } from "./useTasks";

type Route =
  | { name: "list" }
  | { name: "detail"; id: string; planFirst?: boolean }
  | { name: "form"; mode: FormMode }
  | { name: "command" };

export interface TasksPageProps {
  /** Opens Settings; upstream's Settings has no anchor for Launch at login, so it opens the page
   * that holds the switch and highlights nothing. */
  onOpenSettings: () => void;
}

const RECENT_MS = 15 * 60_000;

/** The Tasks screen: a list with two tabs, a detail page, and one form for New and Edit. Everything
 * on it is a `task` command (D-058). */
export default function TasksPage({ onOpenSettings }: TasksPageProps) {
  const state = useTasksState();
  const [route, setRoute] = useState<Route>({ name: "list" });
  const [tab, setTab] = useState<"tasks" | "history">("tasks");
  const [historyTask, setHistoryTask] = useState("");
  const told = useRef(false);

  useEffect(() => {
    if (told.current || !state.list) return;
    told.current = true;
    const recent = state.missed.filter(
      (m) => m.outcome !== "busy" && now() - (ms(m.at) ?? 0) < RECENT_MS,
    );
    if (recent.length === 0) return;
    const caught = recent.filter((m) => m.outcome === "caught-up");
    const old = recent.filter((m) => m.outcome === "too-old");
    const title = (id: string) => state.list?.tasks.find((t) => t.id === id)?.title ?? id;
    const parts = [
      ...caught.map(
        (m) =>
          `${title(m.task)} ran once for ${m.slots} missed slot${m.slots === 1 ? "" : "s"}.`,
      ),
      ...old.map((m) => `${title(m.task)} was too old to run.`),
    ];
    toast.message(
      caught.length > 0
        ? `Caught up ${caught.length} task${caught.length === 1 ? "" : "s"} while Toolport was closed`
        : "A scheduled run was too old to run",
      {
        description: parts.join(" "),
        action: {
          label: "View",
          onClick: () => {
            setRoute({ name: "list" });
            setTab("history");
          },
        },
      },
    );
  }, [state.list, state.missed]);

  const back = () => {
    setRoute({ name: "list" });
    state.reload();
  };
  const actions: ListActions = {
    open: (id) => setRoute({ name: "detail", id }),
    runNow: (id) => setRoute({ name: "detail", id, planFirst: true }),
    openSettings: onOpenSettings,
    newTask: () => setRoute({ name: "form", mode: { kind: "new" } }),
    fromCommand: () => setRoute({ name: "command" }),
  };
  const done = (id: string) => {
    state.reload();
    setRoute({ name: "detail", id });
  };

  if (route.name === "detail")
    return (
      <TaskDetail
        key={`${route.id}:${route.planFirst ? "plan" : ""}`}
        id={route.id}
        state={state}
        planFirst={route.planFirst}
        onBack={back}
        onEdit={(id, fix) => setRoute({ name: "form", mode: { kind: "edit", id, fix } })}
        onDuplicate={(task) =>
          setRoute({ name: "form", mode: { kind: "new", seed: duplicateOf(task) } })
        }
        onOpenSettings={onOpenSettings}
        onShowHistory={(id) => {
          setHistoryTask(id);
          setTab("history");
          setRoute({ name: "list" });
        }}
      />
    );
  if (route.name === "form")
    return (
      <TaskForm
        mode={route.mode}
        onDone={done}
        onCancel={() =>
          setRoute(
            route.mode.kind === "edit"
              ? { name: "detail", id: route.mode.id }
              : { name: "list" },
          )
        }
      />
    );
  if (route.name === "command")
    return <FromCommand onDone={done} onCancel={() => setRoute({ name: "list" })} />;

  const count = state.runs.length;
  const tabs = [
    { id: "tasks", label: "Tasks", count: state.list?.tasks.length ?? 0 },
    { id: "history", label: "History", count },
  ] as const;
  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" onClick={actions.newTask}>
          New task
        </Button>
        <Button size="sm" variant="outline" onClick={actions.fromCommand}>
          New from a command…
        </Button>
      </div>
      <AttentionBanner page="tasks" except={["tasks:waiting:"]} />
      <Banners state={state} actions={actions} />
      <div role="tablist" aria-label="Tasks" className="flex gap-1 border-b">
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            onClick={() => {
              setTab(t.id);
              if (t.id === "tasks") setHistoryTask("");
            }}
            className={`-mb-px border-b-2 px-3 py-2 text-sm font-medium ${tab === t.id ? "border-primary text-foreground" : "border-transparent text-muted-foreground"}`}
          >
            {t.label}{" "}
            <span className="ml-1 tabular-nums text-muted-foreground">{t.count}</span>
          </button>
        ))}
      </div>
      {tab === "tasks" ? (
        <TasksTab state={state} actions={actions} />
      ) : (
        <HistoryTab
          state={state}
          actions={actions}
          task={historyTask}
          onTask={setHistoryTask}
        />
      )}
    </div>
  );
}
