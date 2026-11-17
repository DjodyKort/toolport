import { useRef, useState, type KeyboardEvent } from "react";
import { MoreHorizontal, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useLoad } from "../servers/useLoad";
import AgentsStyles from "./AgentsStyles";
import { listAgents, listSources, listStyles } from "./commands";
import PluginsTab from "./PluginsTab";
import SkillDetail from "./SkillDetail";
import SourcesTab from "./SourcesTab";
import SyncTab from "./SyncTab";
import {
  CleanDialog,
  ExportBundleDialog,
  ImportBundleDialog,
  NewSkillDialog,
} from "./writes";

const TABS = [
  { id: "sync", label: "Sync" },
  { id: "agents", label: "Agents & styles" },
  { id: "plugins", label: "Plugins" },
  { id: "sources", label: "Sources" },
] as const;
type TabId = (typeof TABS)[number]["id"];
type Dialog = "new" | "export" | "import" | "clean" | null;

async function optional<T>(load: () => Promise<T>): Promise<T | null> {
  try {
    return await load();
  } catch {
    return null;
  }
}

/** The Library page (slice S4): one tab row, one primary button (Apply on the Sync tab), a skill
 * as a page of its own with Back. */
export default function LibraryScreen() {
  const [tab, setTab] = useState<TabId>("sync");
  const [skill, setSkill] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [epoch, setEpoch] = useState(0);
  const tabs = useRef<HTMLDivElement>(null);
  const agents = useLoad(() => optional(listAgents), epoch);
  const styles = useLoad(() => optional(listStyles), epoch);
  const sources = useLoad(() => optional(listSources), epoch);

  const counts: Partial<Record<TabId, number>> = {
    agents: (agents.data?.agents.length ?? 0) + (styles.data?.styles.length ?? 0),
    sources: sources.data?.sources.length ?? 0,
  };
  const refresh = () => setEpoch((n) => n + 1);

  function move(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    const last = TABS.length - 1;
    const next =
      event.key === "ArrowRight"
        ? (index + 1) % TABS.length
        : event.key === "ArrowLeft"
          ? (index + last) % TABS.length
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (next === null) return;
    event.preventDefault();
    setTab(TABS[next].id);
    tabs.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  }

  if (skill) {
    return <SkillDetail name={skill} onBack={() => setSkill(null)} onChanged={refresh} />;
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div
          ref={tabs}
          role="tablist"
          aria-label="Library"
          className="flex max-w-full gap-1 overflow-x-auto"
        >
          {TABS.map((item, index) => (
            <button
              key={item.id}
              role="tab"
              type="button"
              id={`library-tab-${item.id}`}
              aria-selected={tab === item.id}
              aria-controls="library-panel"
              tabIndex={tab === item.id ? 0 : -1}
              onClick={() => setTab(item.id)}
              onKeyDown={(event) => move(event, index)}
              className="shrink-0 rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground hover:bg-muted aria-selected:bg-muted aria-selected:text-foreground"
            >
              {item.label}
              {counts[item.id] ? (
                <span className="ml-1.5 rounded-full bg-muted px-1.5 text-xs">
                  {counts[item.id]}
                </span>
              ) : null}
            </button>
          ))}
        </div>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={() => setDialog("new")}>
            <Plus /> New skill…
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="icon-sm" aria-label="More">
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onSelect={() => setDialog("export")}>
                Export bundle…
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={() => setDialog("import")}>
                Import bundle…
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem variant="destructive" onSelect={() => setDialog("clean")}>
                Clean synced files…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      <div id="library-panel" role="tabpanel" aria-labelledby={`library-tab-${tab}`}>
        {tab === "sync" && <SyncTab key={epoch} onOpenSkill={setSkill} />}
        {tab === "agents" && <AgentsStyles key={epoch} />}
        {tab === "plugins" && <PluginsTab />}
        {tab === "sources" && <SourcesTab key={epoch} />}
      </div>
      {dialog === "new" && (
        <NewSkillDialog onClose={() => setDialog(null)} onDone={refresh} />
      )}
      {dialog === "export" && (
        <ExportBundleDialog onClose={() => setDialog(null)} onDone={refresh} />
      )}
      {dialog === "import" && (
        <ImportBundleDialog onClose={() => setDialog(null)} onDone={refresh} />
      )}
      {dialog === "clean" && (
        <CleanDialog onClose={() => setDialog(null)} onDone={refresh} />
      )}
    </div>
  );
}
