import { useState } from "react";
import { ChevronRight, Puzzle, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { SkeletonRows } from "@/components/ui/skeleton";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import FolderPicker from "../context/FolderPicker";
import { useFolder } from "../context/useFolder";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { plural } from "./model";
import PluginDetail from "./PluginDetail";
import { HooksSection } from "./PluginHooks";
import { listPlugins } from "./pluginsCommands";
import type { PluginsLsData, PluginRow } from "./pluginsData";
import { bringsText, tokenText } from "./pluginsModel";

const BIG_PLUGIN = 50;

function PluginLine({
  plugin,
  onOpen,
}: {
  plugin: PluginRow;
  onOpen: (id: string) => void;
}) {
  const { enabled, update } = plugin;
  const cost = tokenText(plugin.cost.measured ?? plugin.cost.projected);
  return (
    <li>
      <button
        type="button"
        aria-label={`Open ${plugin.id}`}
        onClick={() => onOpen(plugin.id)}
        className="flex w-full flex-wrap items-center gap-3 px-4 py-3 text-left hover:bg-muted/60"
      >
        <div className="min-w-48 flex-1">
          <p className="text-sm font-medium">
            {plugin.name}
            {plugin.marketplace && (
              <span className="ml-1 font-normal text-muted-foreground">
                @{plugin.marketplace}
              </span>
            )}
          </p>
          <p className="text-meta text-muted-foreground">
            {plugin.version ?? "unknown version"} · {bringsText(plugin.brings)}
            {cost && <> · about {cost} always loaded</>}
          </p>
        </div>
        {update.state === "update" && (
          <Badge variant="warning">Update to {update.available}</Badge>
        )}
        {update.state === "current" && <Badge variant="off">Up to date</Badge>}
        {enabled.managed !== null && <Badge variant="partial">Managed</Badge>}
        <Badge variant={enabled.effective ? "success" : "off"}>
          {enabled.effective ? "On" : "Off"}
        </Badge>
        <ChevronRight aria-hidden="true" className="size-4 text-muted-foreground" />
      </button>
    </li>
  );
}

function List({
  data,
  folder,
  refreshing,
  onRefresh,
  onOpen,
}: {
  data: PluginsLsData;
  folder: string | null;
  refreshing: boolean;
  onRefresh: () => void;
  onOpen: (id: string) => void;
}) {
  const waiting = data.updatesAvailable;
  const big = data.plugins.filter((p) => p.brings.skills >= BIG_PLUGIN);
  return (
    <>
      <StatusCard
        variant={waiting > 0 ? "warning" : "success"}
        icon={<RefreshCw />}
        title={
          waiting > 0
            ? `${plural(waiting, "plugin update")} available`
            : "Plugins are up to date"
        }
        detail={
          data.refreshError
            ? `The marketplaces were not refreshed: ${data.refreshError}`
            : "Updates come from the marketplace folders on this machine. Check for updates asks Claude Code to refresh them first."
        }
        actions={
          <Button variant="outline" size="sm" disabled={refreshing} onClick={onRefresh}>
            <RefreshCw className={refreshing ? "animate-spin" : undefined} /> Check for
            updates
          </Button>
        }
      />
      {data.managedSettings.present && (
        <p className="rounded-xl border bg-card px-4 py-3 text-meta text-muted-foreground">
          An administrator manages some settings for Claude Code on this machine (
          <span className="font-mono text-xs">{data.managedSettings.path}</span>). A
          plugin they switch is marked Managed and cannot be turned on here.
        </p>
      )}
      <section aria-label="Installed plugins" className="flex flex-col gap-2">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
            Installed plugins
            <span className="ml-1 font-normal normal-case">{data.plugins.length}</span>
          </h2>
          <span className="text-meta text-muted-foreground">
            {plural(data.marketplaces.length, "marketplace")}
            {folder
              ? " · on or off as in the chosen folder"
              : " · on or off in your user settings"}
          </span>
        </div>
        <ul className="divide-y overflow-hidden rounded-xl border bg-card">
          {data.plugins.map((plugin) => (
            <PluginLine key={plugin.id} plugin={plugin} onOpen={onOpen} />
          ))}
        </ul>
        {big.map((plugin) => (
          <p key={plugin.id} className="text-meta text-muted-foreground">
            {plugin.name} brings {plural(plugin.brings.skills, "skill")}. They are listed
            in Skills under “Claude Code plugin” and are never part of your library or
            your sync.
          </p>
        ))}
      </section>
    </>
  );
}

/** Library › Plugins: the installed Claude Code plugins, one as a page of its own, and the hooks
 * Claude Code would run. Switching a plugin in a folder, its settings and its MCP servers go
 * through the same plans as the Context page. */
export default function PluginsTab({
  onChooseItems,
}: {
  /** Opens Sources on the selective-loading screen of a tap source. */
  onChooseItems?: (source: string) => void;
}) {
  const { folder, recent, choose } = useFolder();
  const [open, setOpen] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const plugins = useLoad(() => listPlugins(folder), `${folder}|${refresh}`);

  async function check() {
    setRefreshing(true);
    try {
      const data = await listPlugins(folder, true);
      if (data.refreshError) {
        toastError("The marketplaces were not refreshed", {
          description: data.refreshError,
        });
      } else {
        toast.success(
          data.updatesAvailable > 0
            ? `${plural(data.updatesAvailable, "plugin update")} available`
            : "Every plugin is up to date",
        );
      }
      setRefresh((n) => n + 1);
    } catch (error) {
      toastError("Could not check for updates", { description: errorText(error) });
    } finally {
      setRefreshing(false);
    }
  }

  if (open) {
    return (
      <PluginDetail
        id={open}
        folder={folder}
        onBack={() => setOpen(null)}
        onChanged={plugins.reload}
        onChooseItems={onChooseItems}
      />
    );
  }
  if (plugins.error && !plugins.data) {
    return (
      <p role="alert" className="text-meta text-destructive">
        {plugins.error}
      </p>
    );
  }
  if (!plugins.data) return <SkeletonRows label="Reading the installed plugins" />;
  const data = plugins.data;

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-center gap-2">
        <FolderPicker
          folder={folder}
          home={null}
          suggestions={recent.map((path) => ({ path }))}
          onChoose={choose}
        />
        <span className="text-meta text-muted-foreground">
          The folder decides where a plugin is on and where a switch is written.
        </span>
      </div>
      {data.plugins.length === 0 ? (
        <EmptyState
          icon={<Puzzle />}
          title="No Claude Code plugins installed"
          description={
            data.refreshError
              ? `Claude Code answered: ${data.refreshError}`
              : "Plugins you install with Claude Code are listed here with what they bring, where they are on and whether a newer version is waiting."
          }
        />
      ) : (
        <List
          data={data}
          folder={folder}
          refreshing={refreshing}
          onRefresh={() => void check()}
          onOpen={setOpen}
        />
      )}
      <HooksSection folder={folder} />
    </div>
  );
}
