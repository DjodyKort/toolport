import { useState } from "react";
import { MonitorCog, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { EmptyState } from "@/components/ui/empty-state";
import { SectionHeader } from "@/components/ui/section-header";
import type { ClientSyncData } from "../bridge/data";
import type {
  ClientDirectAddData,
  ClientDirectRmData,
  ClientEditData,
  ClientImportData,
} from "../types/client";
import { Gate } from "./atoms";
import {
  ClientDetailsDialog,
  GatewayBadge,
  ImportDialog,
  SyncDialog,
} from "./ClientDialogs";
import { useWrite } from "./useWrite";
import { WriteDialogs } from "./WriteDialogs";
import type { ClientView, DirectEntry } from "./model";
import {
  clientEditPlan,
  clientImportPlan,
  clientSyncPlan,
  directAddPlan,
  directRmPlan,
} from "./plans";
import { useServers } from "./useServers";

type Dialog =
  | { kind: "details"; id: string }
  | { kind: "import"; id: string }
  | { kind: "sync"; id: string | null };

export function ClientsTab() {
  const { clients, profiles, status, views, clientViews } = useServers();
  const write = useWrite();
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const shown = clientViews ?? [];
  const active = profiles.data?.profiles.find((profile) => profile.active) ?? null;
  const target = (id: string): ClientView | undefined =>
    shown.find((client) => client.id === id);

  function setProfile(client: ClientView, profileId: string) {
    const profile = profiles.data?.profiles.find(
      (candidate) => candidate.id === profileId,
    );
    if (!profile) return;
    setDialog(null);
    write.begin({
      command: "client edit",
      title: `Point ${client.name} at ${profile.name}`,
      argv: ["client", "edit", client.id, "--set-profiles", profile.id],
      adapt: (data, done) => clientEditPlan(data as ClientEditData, done),
    });
  }

  function sync(options: { client: string | null; keepOrphans: boolean }) {
    setDialog(null);
    const one = options.client ? target(options.client) : undefined;
    write.begin({
      command: "client sync",
      title: one ? `Sync ${one.name}` : "Sync the managed clients",
      argv: [
        "client",
        "sync",
        ...(options.client ? ["--client", options.client] : []),
        ...(options.keepOrphans ? ["--keep-orphans"] : []),
      ],
      adapt: (data, done) => clientSyncPlan(data as ClientSyncData, done),
    });
  }

  function directAdd(client: ClientView, serverId: string) {
    const server = views?.find((view) => view.id === serverId);
    if (!server) return;
    setDialog(null);
    write.begin({
      command: "client direct add",
      title: `Add a direct entry for ${server.name} in ${client.name}`,
      argv: ["client", "direct", "add", server.id, "--client", client.id],
      adapt: (data, done) => directAddPlan(data as ClientDirectAddData, done),
    });
  }

  function directRm(client: ClientView, entry: DirectEntry) {
    setDialog(null);
    write.begin({
      command: "client direct rm",
      title: `Remove the direct entry ${entry.entry} from ${client.name}`,
      argv: ["client", "direct", "rm", entry.server, "--client", client.id],
      adapt: (data, done) => directRmPlan(data as ClientDirectRmData, done),
    });
  }

  function importEntries(client: ClientView, selected: string[], profile: string) {
    setDialog(null);
    write.begin({
      command: "client import",
      title: `Import ${selected.length} entr${selected.length === 1 ? "y" : "ies"} from ${client.name}`,
      argv: [
        "client",
        "import",
        client.id,
        "--select",
        selected.join(","),
        ...(profile ? ["--profile", profile] : []),
      ],
      adapt: (data, done) => clientImportPlan(data as ClientImportData, done),
    });
  }

  const open = dialog?.kind === "details" ? target(dialog.id) : undefined;
  const importing = dialog?.kind === "import" ? target(dialog.id) : undefined;
  const orphaned = shown.filter((client) => client.orphans.length > 0);

  return (
    <div className="flex flex-col gap-4">
      <Gate
        queries={[clients, profiles, status]}
        title="Couldn't load the clients"
        context="client ls, profile ls, status"
      >
        {() => (
          <>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <p className="text-sm text-muted-foreground">
                What each AI client gets from Toolport, and which profile it uses.
                {active &&
                  ` Clients without a profile of their own follow ${active.name}.`}
              </p>
              <Button
                variant="outline"
                onClick={() => setDialog({ kind: "sync", id: null })}
              >
                <RefreshCw /> Sync clients…
              </Button>
            </div>
            {shown.length === 0 ? (
              <EmptyState
                icon={<MonitorCog />}
                title="No clients found"
                description="No AI client with an MCP config was detected on this computer."
              />
            ) : (
              <div className="overflow-auto rounded-lg border">
                <table className="w-full text-sm">
                  <caption className="sr-only">Clients</caption>
                  <thead className="border-b bg-muted/40 text-left text-xs text-muted-foreground">
                    <tr>
                      <th scope="col" className="px-3 py-2 font-medium">
                        Client
                      </th>
                      <th scope="col" className="px-3 py-2 font-medium">
                        Profile
                      </th>
                      <th scope="col" className="px-3 py-2 font-medium">
                        Servers
                      </th>
                      <th scope="col" className="px-3 py-2 font-medium">
                        What it sees
                      </th>
                      <th scope="col" className="px-3 py-2">
                        <span className="sr-only">Actions</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {shown.map((client) => (
                      <tr key={client.id} className="border-b last:border-b-0">
                        <th scope="row" className="px-3 py-2 text-left font-medium">
                          <span className="flex items-center gap-2">
                            {client.name} <GatewayBadge state={client.gateway} />
                          </span>
                        </th>
                        <td className="px-3 py-2">
                          {client.profile
                            ? client.followsActive
                              ? `${client.profile.name} (active)`
                              : client.profile.name
                            : (client.scope ?? "Active profile")}
                        </td>
                        <td className="px-3 py-2 tabular-nums">{client.seen.length}</td>
                        <td className="px-3 py-2 text-muted-foreground">
                          {client.gateway !== "managed"
                            ? "Not through Toolport"
                            : `${client.tools.toLocaleString("en")} tools from ${client.connectedServers} servers`}
                        </td>
                        <td className="px-3 py-2 text-right">
                          <Button
                            size="sm"
                            variant="outline"
                            aria-label={`Details of ${client.name}`}
                            onClick={() => setDialog({ kind: "details", id: client.id })}
                          >
                            Details
                          </Button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            {orphaned.length > 0 && (
              <section aria-label="Orphans" className="flex flex-col gap-2">
                <SectionHeader
                  tone="warning"
                  count={orphaned.reduce((n, c) => n + c.orphans.length, 0)}
                >
                  Orphans
                </SectionHeader>
                <ul className="flex flex-col divide-y rounded-lg border">
                  {orphaned.map((client) => (
                    <li
                      key={client.id}
                      className="flex flex-wrap items-center gap-2 px-3 py-2 text-sm"
                    >
                      <span className="font-medium">{client.name}</span>
                      <span className="min-w-0 flex-1 text-muted-foreground">
                        {client.orphans.join(", ")}
                      </span>
                      <Button
                        size="sm"
                        variant="outline"
                        aria-label={`Import the orphans of ${client.name}`}
                        onClick={() => setDialog({ kind: "import", id: client.id })}
                      >
                        Import…
                      </Button>
                    </li>
                  ))}
                </ul>
                <Callout variant="info">
                  An orphan is an entry in a client's own config that no server in the
                  registry owns. Sync removes it unless you keep it; import turns it into
                  a server.
                </Callout>
              </section>
            )}
          </>
        )}
      </Gate>
      {open && profiles.data && (
        <ClientDetailsDialog
          view={open}
          profiles={profiles.data.profiles}
          servers={views ?? []}
          busy={write.busy}
          onClose={() => setDialog(null)}
          onSetProfile={(id) => setProfile(open, id)}
          onSync={() => setDialog({ kind: "sync", id: open.id })}
          onImport={() => setDialog({ kind: "import", id: open.id })}
          onDirectAdd={(id) => directAdd(open, id)}
          onDirectRm={(entry) => directRm(open, entry)}
        />
      )}
      {importing && profiles.data && (
        <ImportDialog
          view={importing}
          profiles={profiles.data.profiles}
          onClose={() => setDialog(null)}
          onSubmit={(selected, profile) => importEntries(importing, selected, profile)}
        />
      )}
      {dialog?.kind === "sync" && (
        <SyncDialog
          clients={shown}
          only={dialog.id}
          onClose={() => setDialog(null)}
          onSubmit={sync}
        />
      )}
      <WriteDialogs write={write} />
    </div>
  );
}
