import { lazy, Suspense, useCallback, useRef, useState, type KeyboardEvent } from "react";
import { FolderOpen } from "lucide-react";
import { EmptyState } from "@/components/ui/empty-state";
import { SkeletonRows } from "@/components/ui/skeleton";
import { useLoad } from "../servers/useLoad";
import { listBundles } from "./commands";
import Bundles from "./Bundles";
import FolderPicker from "./FolderPicker";
import RulesLaunch from "./RulesLaunch";
import ThisFolder from "./ThisFolder";
import { useFolder } from "./useFolder";

const HooksCompression = lazy(() => import("../compression/HooksCompression"));

const TABS = [
  { id: "folder", label: "This folder" },
  { id: "bundles", label: "Folder bundles" },
  { id: "rules", label: "Rules & launch" },
  { id: "hooks", label: "Hooks & compression" },
] as const;
type TabId = (typeof TABS)[number]["id"];

/** The Context page (slice S5): four tabs, the folder in the page header for the tabs that
 * depend on it. */
export default function ContextScreen() {
  const [tab, setTab] = useState<TabId>("folder");
  const [home, setHome] = useState<string | null>(null);
  const { folder, recent, choose } = useFolder();
  const bundles = useLoad(listBundles, "picker");
  const tabs = useRef<HTMLDivElement>(null);
  const onHome = useCallback((value: string | null) => setHome(value), []);

  const applied = (bundles.data?.bundles ?? []).flatMap((b) =>
    b.appliedTo.map((path) => ({ path, note: `bundle ${b.name}` })),
  );
  const suggestions = [
    ...recent.map((path) => ({ path, note: applied.find((a) => a.path === path)?.note })),
    ...applied.filter((a) => !recent.includes(a.path)),
  ];

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

  const needsFolder = !folder && (tab === "folder" || tab === "bundles");
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div
          ref={tabs}
          role="tablist"
          aria-label="Context"
          className="flex max-w-full gap-1 overflow-x-auto"
        >
          {TABS.map((item, index) => (
            <button
              key={item.id}
              role="tab"
              type="button"
              id={`context-tab-${item.id}`}
              aria-selected={tab === item.id}
              aria-controls="context-panel"
              tabIndex={tab === item.id ? 0 : -1}
              onClick={() => setTab(item.id)}
              onKeyDown={(event) => move(event, index)}
              className="shrink-0 rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground hover:bg-muted aria-selected:bg-muted aria-selected:text-foreground"
            >
              {item.label}
            </button>
          ))}
        </div>
        {tab !== "rules" && (
          <FolderPicker
            folder={folder}
            home={home}
            suggestions={suggestions}
            onChoose={choose}
          />
        )}
      </div>
      <div id="context-panel" role="tabpanel" aria-labelledby={`context-tab-${tab}`}>
        {needsFolder ? (
          <EmptyState
            icon={<FolderOpen />}
            title="Choose a folder"
            description="Pick the folder where Claude Code starts to see what it loads there and what that costs. The folder chip above also lists the folders a bundle is applied in."
          />
        ) : (
          <>
            {tab === "folder" && folder && (
              <ThisFolder
                key={folder}
                folder={folder}
                onHome={onHome}
                onOpenBundles={() => setTab("bundles")}
              />
            )}
            {tab === "bundles" && <Bundles folder={folder} home={home} />}
            {tab === "rules" && <RulesLaunch home={home} />}
            {tab === "hooks" && (
              <Suspense
                fallback={<SkeletonRows label="Loading the compression hooks…" />}
              >
                <HooksCompression />
              </Suspense>
            )}
          </>
        )}
      </div>
    </div>
  );
}
