import { useState, type ComponentProps } from "react";
import { Badge } from "@/components/ui/badge";
import { Callout } from "@/components/Callout";
import { EmptyState } from "@/components/ui/empty-state";
import type { PluginRow, PluginsLsData, PluginsShowData } from "../types/plugins";
import { AsyncView } from "../ui";
import { FolderField, useFolderChoice, useRecentFolders } from "../context/folder";
import { useRead, useRegistryRows, useWrite } from "../skills/hooks";
import { WriteDialogs } from "../skills/WriteDialogs";
import { OfflineNote } from "../system/atoms";
import { PluginDetail } from "./PluginDetail";
import { TerminalDialog, useTerminalStep } from "./TerminalStep";
import { cwdArgs, updateText } from "./model";

function Row({
  row,
  selected,
  onSelect,
}: {
  row: PluginRow;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <li>
      <button
        type="button"
        aria-current={selected ? "true" : undefined}
        onClick={onSelect}
        className="flex w-full items-center gap-2 rounded-lg border px-3 py-2 text-left text-sm outline-none hover:bg-muted focus-visible:ring-1 focus-visible:ring-ring aria-[current=true]:border-primary aria-[current=true]:bg-muted"
      >
        <span className="flex min-w-0 flex-1 flex-col">
          <b className="truncate">{row.name}</b>
          <small className="truncate text-muted-foreground">
            {row.version} · {row.source}
          </small>
        </span>
        <Badge variant={row.update.state === "update" ? "warning" : "secondary"}>
          {updateText(row)}
        </Badge>
        {!row.enabled.effective && <Badge variant="outline">off here</Badge>}
      </button>
    </li>
  );
}

/** The detail of the chosen plugin. It mounts once the list names one, so no read ever runs
 * with an empty plugin id. */
function Selected({
  id,
  cwd,
  onChanged,
  onTerminal,
  onOpenHooks,
}: {
  id: string;
  cwd: string;
  onChanged: () => void;
  onTerminal: ComponentProps<typeof PluginDetail>["onTerminal"];
  onOpenHooks?: () => void;
}) {
  const show = useRead<PluginsShowData>(["plugins", "show", id, ...cwdArgs(cwd)]);
  const write = useWrite(useRegistryRows(), () => {
    onChanged();
    show.reload();
  });
  return (
    <>
      <PluginDetail
        query={show}
        cwd={cwd}
        write={write}
        onTerminal={onTerminal}
        onOpenHooks={onOpenHooks}
      />
      <WriteDialogs write={write} />
    </>
  );
}

/** Library > Plugins: every plugin Claude Code has installed, what it costs and the switches
 * that really exist (D-075). Reads are local; a change is a plan you confirm first. */
export function PluginsTab({ onOpenHooks }: { onOpenHooks?: () => void }) {
  const here = useFolderChoice();
  const { recent, remember } = useRecentFolders();
  const cwd = here.folder;
  const list = useRead<PluginsLsData>(["plugins", "ls", ...cwdArgs(cwd)]);
  const [picked, setPicked] = useState<string | null>(null);
  const rows = list.data?.plugins ?? [];
  const id = rows.find((row) => row.id === picked)?.id ?? rows[0]?.id ?? "";
  const terminal = useTerminalStep();
  const folderLine = cwd
    ? `Folder ${cwd}`
    : "No folder chosen: showing your own settings";
  return (
    <div role="group" aria-label="Plugins" className="flex flex-col gap-4">
      <FolderField
        label="Folder"
        value={here.draft}
        onChange={here.setDraft}
        options={recent}
        placeholder="/path/to/the/folder where Claude Code starts"
        submitLabel="Use folder"
        onSubmit={() => {
          here.show(here.draft);
          remember(here.draft);
        }}
      />
      <p className="text-xs text-muted-foreground">
        {folderLine}. Turning things off, settings and denied servers are per folder.
      </p>
      <OfflineNote />
      {list.data?.refreshError && (
        <Callout variant="warning" role="status">
          The marketplaces could not be refreshed: {list.data.refreshError}. Update states
          are from the last refresh.
        </Callout>
      )}
      {list.data?.restartRequired && (
        <Callout variant="info" role="status">
          Restart Claude Code for the latest plugin changes to load.
        </Callout>
      )}
      {list.data?.partial && (
        <Callout variant="info" role="status">
          Claude Code&apos;s own plugin list was not available, so some figures come from
          the plugin files.
        </Callout>
      )}
      <AsyncView
        query={list}
        errorTitle="Couldn't read the plugins"
        context="plugins ls"
        isEmpty={(data) => data.plugins.length === 0}
        empty={
          <EmptyState
            title="No plugins installed"
            description="Claude Code has no plugin installed for this folder. Install one with claude plugin install and it shows here."
          />
        }
      >
        {() => (
          <div className="grid gap-4 lg:grid-cols-[minmax(0,20rem)_minmax(0,1fr)]">
            <ul aria-label="Plugins" className="flex flex-col gap-2">
              {rows.map((row) => (
                <Row
                  key={row.id}
                  row={row}
                  selected={row.id === id}
                  onSelect={() => setPicked(row.id)}
                />
              ))}
            </ul>
            <Selected
              key={`${id}|${cwd}`}
              id={id}
              cwd={cwd}
              onChanged={list.reload}
              onTerminal={terminal.open}
              onOpenHooks={onOpenHooks}
            />
          </div>
        )}
      </AsyncView>
      <TerminalDialog step={terminal.step} onClose={terminal.close} />
    </div>
  );
}
