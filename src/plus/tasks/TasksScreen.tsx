import { useState } from "react";
import { NOT_BUILT_TABS } from "../notBuiltTabs";
import { useRestoreFocus } from "../agents/useRestoreFocus";
import { Tabs } from "../ui";
import { HistoryTab } from "./HistoryTab";
import { TasksTab } from "./TasksTab";

const TABS = NOT_BUILT_TABS.tasks ?? [];

/** The Tasks screen: the named jobs Toolport can run, and the history of their runs. */
export function TasksScreen({
  initialTab = "tasks",
  initialTask,
  pollMs,
}: {
  initialTab?: string;
  initialTask?: string;
  pollMs?: number;
  onOpenCommands?: (group?: string) => void;
}) {
  useRestoreFocus();
  const [tab, setTab] = useState(initialTab);
  const current = TABS.find((item) => item.id === tab) ?? TABS[0];
  return (
    <Tabs
      items={TABS.map(({ id, label }) => ({ id, label }))}
      value={current?.id ?? ""}
      onValueChange={setTab}
      label="Tasks sections"
    >
      {current?.id === "history" ? (
        <HistoryTab />
      ) : (
        <TasksTab initialTask={initialTask} pollMs={pollMs} />
      )}
    </Tabs>
  );
}
