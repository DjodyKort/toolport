import { lazy, Suspense } from "react";
import { SkeletonRows } from "@/components/ui/skeleton";
import type { Registry, View } from "@/lib/types";
import ContextScreen from "../context/ContextScreen";
import ToolportPlusPage from "../settings/ToolportPlusPage";
import TasksPage from "../tasks/TasksPage";

const LibraryScreen = lazy(() => import("../library/LibraryScreen"));
const SpendScreen = lazy(() => import("../spend/SpendScreen"));

export interface PlusViewsProps {
  view: View;
  registry: Registry | null;
  onRegistryChange: (registry: Registry) => void;
  onOpenServers: () => void;
  onOpenSettings: () => void;
}

/** Everything the fork renders inside upstream's content area: the pages of the "Claude" group and
 * the Toolport+ page of Settings. App.tsx mounts this once, lazily. */
export default function PlusViews({
  view,
  registry,
  onRegistryChange,
  onOpenServers,
  onOpenSettings,
}: PlusViewsProps) {
  if (view === "tasks") return <TasksPage onOpenSettings={onOpenSettings} />;
  if (view === "context") return <ContextScreen />;
  if (view === "library") {
    return (
      <Suspense fallback={<SkeletonRows label="Loading the library" />}>
        <LibraryScreen />
      </Suspense>
    );
  }
  if (view === "spend") {
    return (
      <Suspense fallback={<SkeletonRows label="Loading Spend" />}>
        <SpendScreen />
      </Suspense>
    );
  }
  return (
    <ToolportPlusPage
      registry={registry}
      onRegistryChange={onRegistryChange}
      onOpenServers={onOpenServers}
    />
  );
}
