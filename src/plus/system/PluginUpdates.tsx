import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import type { CcUpdateData } from "../types/cc";
import { AsyncView } from "../ui";
import { updateSpec } from "../plugins/model";
import { useRead, useRegistryRows, useWrite } from "../skills/hooks";
import { WriteDialogs } from "../skills/WriteDialogs";
import { Card, Intro } from "./atoms";

const STATUS: Record<string, string> = {
  current: "up to date",
  update: "update available",
  unknown: "update state unknown",
  blocked: "update blocked",
};

type Row = CcUpdateData["plugins"][number];

function PluginRow({
  row,
  busy,
  onUpdate,
}: {
  row: Row;
  busy: boolean;
  onUpdate: () => void;
}) {
  const behind = row.status === "update" || row.status === "unknown";
  return (
    <li className="flex flex-wrap items-center gap-2 px-3 py-2 text-sm">
      <span className="flex min-w-0 flex-1 flex-col">
        <b className="truncate">{row.name}</b>
        <small className="text-muted-foreground">
          {row.installed}
          {row.available ? ` to ${row.available}` : ""} · {row.marketplace}
        </small>
      </span>
      <Badge variant={row.status === "update" ? "warning" : "secondary"}>
        {STATUS[row.status] ?? row.status}
      </Badge>
      {!row.enabled && <Badge variant="outline">disabled</Badge>}
      {row.blocked && <Badge variant="destructive">blocked</Badge>}
      <Button
        size="xs"
        variant="outline"
        disabled={busy || row.blocked || !behind}
        title={behind ? undefined : "This plugin is already up to date"}
        onClick={onUpdate}
      >
        Update…
      </Button>
    </li>
  );
}

/** The Claude Code plugins section of System > Updates: `cc list`, and `cc update` through a
 * preview. Library > Plugins runs the same update for one plugin (D-075). */
export function PluginUpdates({
  onOpenCommands,
}: {
  onOpenCommands: (group?: string) => void;
}) {
  const query = useRead<CcUpdateData>(["cc", "list"]);
  const write = useWrite(useRegistryRows(), query.reload);
  return (
    <Card
      title="Claude Code plugins"
      actions={
        <>
          <Button size="sm" variant="ghost" onClick={() => onOpenCommands("cc")}>
            All plugin commands
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={write.busy || (query.data?.plugins.length ?? 0) === 0}
            onClick={() => write.begin(updateSpec())}
          >
            Update all plugins…
          </Button>
        </>
      }
    >
      <Intro>
        Plugins Claude Code has installed, next to the server updates. Updating one shows
        what changes first; Claude Code loads the new version after a restart.
      </Intro>
      <AsyncView
        query={query}
        errorTitle="Couldn't read the plugins"
        context="cc list"
        isEmpty={(data) => data.plugins.length === 0}
        empty={
          <p className="text-sm text-muted-foreground">
            No Claude Code plugin is installed.
          </p>
        }
      >
        {(data) => (
          <>
            {data.refreshError && (
              <Callout variant="warning" role="status">
                The marketplaces could not be refreshed: {data.refreshError}.
              </Callout>
            )}
            <ul
              aria-label="Plugin updates"
              className="flex flex-col divide-y rounded-lg border"
            >
              {data.plugins.map((row) => (
                <PluginRow
                  key={row.id}
                  row={row}
                  busy={write.busy}
                  onUpdate={() => write.begin(updateSpec(row.name))}
                />
              ))}
            </ul>
          </>
        )}
      </AsyncView>
      <WriteDialogs write={write} />
    </Card>
  );
}
