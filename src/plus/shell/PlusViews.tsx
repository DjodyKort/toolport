import { EmptyState } from "@/components/ui/empty-state";
import type { Registry, View } from "@/lib/types";
import ToolportPlusPage from "../settings/ToolportPlusPage";
import { isPlusView, PLUS_SCREENS, type PlusScreen } from "./views";

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
}

/** Everything the fork renders inside upstream's content area: the pages of the "Claude" group and
 * the Toolport+ page of Settings. App.tsx mounts this once, lazily. */
export default function PlusViews({
  view,
  registry,
  onRegistryChange,
  onOpenServers,
}: PlusViewsProps) {
  if (isPlusView(view)) return <NotBuiltYet screen={PLUS_SCREENS[view]} />;
  return (
    <ToolportPlusPage
      registry={registry}
      onRegistryChange={onRegistryChange}
      onOpenServers={onOpenServers}
    />
  );
}
