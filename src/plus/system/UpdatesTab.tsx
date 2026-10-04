import { useState } from "react";
import { RefreshCw, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { EmptyState } from "@/components/ui/empty-state";
import { ctlData } from "../bridge/ctl";
import { AsyncView, errorText } from "../ui";
import { Intro, Mono, OfflineNote, Tag } from "./atoms";
import { rowsOf, useRead, useRegistry, useWrite } from "./hooks";
import {
  canUpdate,
  kindLabel,
  planOfUpdate,
  plural,
  postUpdateOf,
  statusOf,
  updateReport,
  type UpdateServer,
} from "./model";
import { PluginUpdates } from "./PluginUpdates";
import { UpdateOptions, type UpdateChoice } from "./UpdateOptions";
import { WriteDialogs } from "./WriteDialogs";

function Version({ server }: { server: UpdateServer }) {
  if (server.current && server.latest) {
    return (
      <span>
        <Mono>{server.current.slice(0, 12)}</Mono> to{" "}
        <Mono>{server.latest.slice(0, 12)}</Mono>
      </span>
    );
  }
  if (server.behind) return <span>{plural(server.behind, "commit")} behind</span>;
  return <span className="text-muted-foreground">none</span>;
}

/** System > Updates: where each server's updates come from, check, and apply with a preview
 * (D-059). The `post_update` command of a server is shown before it can run. */
export function UpdatesTab({
  onOpenCommands,
}: {
  onOpenCommands: (group?: string) => void;
}) {
  const registry = useRegistry();
  const rows = rowsOf(registry);
  const check = useRead<unknown>(["update", "--check"]);
  const [checked, setChecked] = useState<Record<string, UpdateServer>>({});
  const [checking, setChecking] = useState<string | null>(null);
  const [rowError, setRowError] = useState<string | null>(null);
  const [options, setOptions] = useState<{
    mode: "apply" | "init";
    single?: string;
  } | null>(null);

  const refresh = () => {
    setChecked({});
    check.reload();
  };
  const write = useWrite(rows, refresh);

  async function checkOne(id: string) {
    setChecking(id);
    setRowError(null);
    try {
      const data = await ctlData<unknown>(["update", id, "--check"]);
      const found = updateReport(data).servers.find((server) => server.id === id);
      if (found) setChecked((prev) => ({ ...prev, [id]: found }));
    } catch (error) {
      setRowError(`${id}: ${errorText(error).message}`);
    } finally {
      setChecking(null);
    }
  }

  function begin(choice: UpdateChoice) {
    setOptions(null);
    write.begin({
      command: "update",
      title: choice.title,
      argv: choice.argv,
      confirmLabel: choice.argv.includes("--init") ? "Store sources" : "Update",
      adapt: planOfUpdate,
    });
  }

  return (
    <div className="flex flex-col gap-4">
      <OfflineNote />
      <AsyncView
        query={check}
        errorTitle="Couldn't check for updates"
        isEmpty={(data) => updateReport(data).servers.length === 0}
        empty={
          <EmptyState
            icon={<Search />}
            title="No servers to update"
            description="Add a server first. Updates are checked for the servers in your registry."
          />
        }
      >
        {(data) => {
          const report = updateReport(data);
          const servers = report.servers.map((server) => checked[server.id] ?? server);
          const updatable = servers.filter(canUpdate);
          return (
            <>
              <div className="flex flex-wrap items-center justify-between gap-2">
                <p className="text-sm text-muted-foreground">
                  {Object.entries(report.counts)
                    .map(
                      ([status, count]) =>
                        `${count} ${statusOf(status).label.toLowerCase()}`,
                    )
                    .join(", ") || "No result"}
                </p>
                <div className="flex flex-wrap gap-2">
                  <Button size="sm" variant="outline" onClick={refresh}>
                    <RefreshCw /> Check all
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => setOptions({ mode: "init" })}
                  >
                    Detect sources…
                  </Button>
                  <Button
                    size="sm"
                    disabled={updatable.length === 0}
                    onClick={() => setOptions({ mode: "apply" })}
                  >
                    Update all…
                  </Button>
                </div>
              </div>
              {rowError && (
                <Callout variant="danger" role="alert">
                  {rowError}
                </Callout>
              )}
              <div className="overflow-auto rounded-lg border bg-card">
                <table className="w-full text-sm">
                  <thead className="text-left text-xs text-muted-foreground">
                    <tr>
                      <th scope="col" className="px-3 py-2 font-medium">
                        Server
                      </th>
                      <th scope="col" className="px-3 py-2 font-medium">
                        Source
                      </th>
                      <th scope="col" className="px-3 py-2 font-medium">
                        State
                      </th>
                      <th scope="col" className="px-3 py-2 font-medium">
                        Version
                      </th>
                      <th scope="col" className="px-3 py-2">
                        <span className="sr-only">Actions</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {servers.map((server) => {
                      const hook = postUpdateOf(server);
                      const status = statusOf(server.status);
                      return (
                        <tr key={server.id} className="border-t align-top">
                          <th scope="row" className="px-3 py-2 text-left font-semibold">
                            {server.id}
                          </th>
                          <td className="px-3 py-2">
                            {kindLabel(server.kind)}
                            <div className="text-xs break-words text-muted-foreground">
                              {server.message}
                            </div>
                            {hook && (
                              <div className="text-xs text-muted-foreground">
                                Update command: <Mono>{hook.command}</Mono>
                                {hook.held && " (only runs if you allow it)"}
                              </div>
                            )}
                          </td>
                          <td className="px-3 py-2">
                            <Tag tone={status.tone}>{status.label}</Tag>
                          </td>
                          <td className="px-3 py-2">
                            <Version server={server} />
                          </td>
                          <td className="px-3 py-2 text-right">
                            <div className="flex justify-end gap-2">
                              <Button
                                size="xs"
                                variant="outline"
                                disabled={checking === server.id}
                                aria-label={`Check ${server.id}`}
                                onClick={() => void checkOne(server.id)}
                              >
                                {checking === server.id ? "Checking…" : "Check"}
                              </Button>
                              {canUpdate(server) && (
                                <Button
                                  size="xs"
                                  aria-label={`Update ${server.id}`}
                                  onClick={() =>
                                    setOptions({ mode: "apply", single: server.id })
                                  }
                                >
                                  Update…
                                </Button>
                              )}
                            </div>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
              <Intro>
                An update runs the server's update command only after you see it here and
                confirm.
              </Intro>
              {options && (
                <UpdateOptions
                  mode={options.mode}
                  single={options.single}
                  servers={
                    options.mode === "init"
                      ? servers
                      : options.single
                        ? servers.filter((server) => server.id === options.single)
                        : updatable
                  }
                  onPreview={begin}
                  onClose={() => setOptions(null)}
                />
              )}
            </>
          );
        }}
      </AsyncView>
      <PluginUpdates onOpenCommands={onOpenCommands} />
      <WriteDialogs write={write} />
    </div>
  );
}
