import { useEffect, useId } from "react";
import type { LucideIcon } from "lucide-react";
import { useI18n, type SourceMessage } from "@/lib/i18n";
import type { View } from "@/lib/types";
import { PLUS_NAVIGATE_EVENT } from "./navigate";
import { CLAUDE_GROUP_LABEL, PLUS_SCREENS, PLUS_VIEWS } from "./views";

type NavRow = (
  Icon: LucideIcon,
  label: SourceMessage,
  active: boolean,
  onClick: () => void,
) => React.ReactNode;

/** The "Claude" group after upstream's six entries, as a second navigation landmark under the
 * upstream one, so upstream's own list of views stays as it is. The rows are drawn by the sidebar's own row
 * builder, so they cannot drift from upstream's. Under 900 px the label is hidden with the other
 * sidebar labels and a thin rule keeps the group apart. */
export function PlusNavGroup({
  view,
  onSelectView,
  row,
}: {
  view: View;
  onSelectView: (view: View) => void;
  row: NavRow;
}) {
  const { t } = useI18n();
  const labelId = useId();
  useEffect(() => {
    const open = (event: Event) => onSelectView((event as CustomEvent<View>).detail);
    window.addEventListener(PLUS_NAVIGATE_EVENT, open);
    return () => window.removeEventListener(PLUS_NAVIGATE_EVENT, open);
  }, [onSelectView]);
  return (
    <nav aria-labelledby={labelId} className="flex flex-col gap-0.5 px-3">
      <div
        id={labelId}
        className="sidebar-label px-2.5 pt-3 pb-1 text-xs font-medium tracking-wide text-muted-foreground uppercase"
      >
        {t(CLAUDE_GROUP_LABEL)}
      </div>
      <div aria-hidden="true" className="mx-2 my-1 hidden border-t max-[900px]:block" />
      {PLUS_VIEWS.map((id) => {
        const screen = PLUS_SCREENS[id];
        return (
          <div key={id}>
            {row(screen.icon, screen.label, view === id, () => onSelectView(id))}
          </div>
        );
      })}
    </nav>
  );
}
