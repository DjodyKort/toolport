import { lazy, Suspense } from "react";
import { EmptyState } from "@/components/ui/empty-state";
import { SkeletonRows } from "@/components/ui/skeleton";
import type { Registry, View } from "@/lib/types";
import ContextScreen from "../context/ContextScreen";
import ToolportPlusPage from "../settings/ToolportPlusPage";
import TasksPage from "../tasks/TasksPage";

import { isPlusView, PLUS_SCREENS, type PlusScreen } from "./views";

const LibraryScreen = lazy(() => import("../library/LibraryScreen"));
const SpendPage = lazy(() => import("../spend/SpendPage"));

/** A page of the "Claude" group whose slice has not landed. It says what the page will do and
 * which slice delivers it, so a click on the sidebar entry never ends in a blank page. */
function NotBuiltYet({ screen }: { screen: PlusScreen }) {
  const Icon = screen.icon;
  return (
    <EmptyState
      icon={<Icon />}
      title={`${screen.title} arrives with slice ${screen.slice}`}
      description={`${screen.what} Nothing on this page works yet.`}
    />
  );
}

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
        <SpendPage />
      </Suspense>
    );
  }
  if (isPlusView(view)) return <NotBuiltYet screen={PLUS_SCREENS[view]} />;
  return (
    <ToolportPlusPage
      registry={registry}
      onRegistryChange={onRegistryChange}
      onOpenServers={onOpenServers}
    />
  );
}
