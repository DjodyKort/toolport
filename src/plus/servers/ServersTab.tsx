import { useState } from "react";
import { Plus, Search, Server } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import type { ProfileEditData, ServerUninstallData } from "../bridge/data";
import { SectionHeader } from "@/components/ui/section-header";
import { Avatar, Gate, StateBadge } from "./atoms";
import { useWrite } from "./useWrite";
import { WriteDialogs } from "./WriteDialogs";
import { newServerArgv } from "./forms";
import {
  GROUP_TITLES,
  groupServers,
  profileUsers,
  serverSummary,
  type ServerGroup,
  type ServerInfo,
  type ServerView,
} from "./model";
import {
  editServerPlan,
  installPlan,
  newServerPlan,
  profileEditPlan,
  uninstallPlan,
} from "./plans";
import { ServerDetail } from "./ServerDetail";
import { AddServerDialog, EditServerDialog, RemoveServerDialog } from "./ServerDialogs";
import { useServers } from "./useServers";

const GROUP_TONE: Record<ServerGroup, "warning" | "success" | "muted"> = {
  attention: "warning",
  connected: "success",
  waiting: "muted",
};

export function ServersTab() {
  const { servers, profiles, status, views, clients } = useServers();
  const write = useWrite();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<ServerInfo | null>(null);
  const [removing, setRemoving] = useState<{
    view: ServerView;
    info: ServerInfo | null;
  } | null>(null);

  const groups = views ? groupServers(views, query) : [];
  const current =
    views?.find((view) => view.id === selected) ?? groups[0]?.servers[0] ?? null;

  if (current && current.id !== selected) setSelected(current.id);

  function toggleProfile(view: ServerView, profileId: string, on: boolean) {
    const profile = profiles.data?.profiles.find(
      (candidate) => candidate.id === profileId,
    );
    if (!profile) return;
    write.begin({
      command: "profile edit",
      title: `${on ? "Add" : "Remove"} ${view.name} ${on ? "to" : "from"} ${profile.name}`,
      argv: [
        "profile",
        "edit",
        profile.id,
        on ? "--add-server" : "--remove-server",
        view.id,
      ],
      adapt: (data, done) =>
        profileEditPlan(data as ProfileEditData, done, {
          users: profileUsers(profile, profile.active, clients.data?.clients ?? null),
        }),
      after: () => setVersion((n) => n + 1),
    });
  }

  function remove(options: { keepClients: boolean; keepSecrets: boolean }) {
    if (!removing) return;
    const { view, info } = removing;
    setRemoving(null);
    write.begin({
      command: "server uninstall",
      title: `Remove ${view.name}`,
      confirmLabel: "Remove server",
      phrase: view.name,
      argv: [
        "server",
        "uninstall",
        view.id,
        ...(options.keepClients ? ["--keep-clients"] : []),
        ...(options.keepSecrets ? ["--keep-secrets"] : []),
      ],
      adapt: (data, done) =>
        uninstallPlan(data as ServerUninstallData, done, { info, ...options }),
      after: () => setSelected(null),
    });
  }

  return (
    <div className="flex flex-col gap-4">
      <Gate
        queries={[servers, profiles, status]}
        title="Couldn't load the servers"
        context="server ls, profile ls, status"
      >
        {() => (
          <>
            {views && views.length === 0 ? (
              <EmptyState
                icon={<Server />}
                title="No servers yet"
                description="Add one from the catalog, or describe your own."
                action={
                  <Button onClick={() => setAdding(true)}>
                    <Plus /> Add server
                  </Button>
                }
              />
            ) : (
              <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)]">
                <div className="flex min-w-0 flex-col gap-4">
                  <div className="flex items-center gap-2">
                    <div className="relative flex-1">
                      <Search
                        className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground"
                        aria-hidden="true"
                      />
                      <Input
                        type="search"
                        aria-label="Search servers"
                        placeholder="Search servers"
                        value={query}
                        onChange={(event) => setQuery(event.target.value)}
                        className="pl-8"
                      />
                    </div>
                    <Button onClick={() => setAdding(true)}>
                      <Plus /> Add server
                    </Button>
                  </div>
                  {groups.length === 0 && (
                    <p className="text-sm text-muted-foreground">
                      No server matches {query}.
                    </p>
                  )}
                  {groups.map(({ group, servers: rows }) => (
                    <section key={group} aria-label={GROUP_TITLES[group]}>
                      <SectionHeader tone={GROUP_TONE[group]} count={rows.length}>
                        {GROUP_TITLES[group]}
                      </SectionHeader>
                      <ul className="flex flex-col rounded-lg border">
                        {rows.map((view) => (
                          <li
                            key={view.id}
                            className={cn(
                              "grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2 border-b px-2 py-1.5 last:border-b-0",
                              current?.id === view.id && "bg-muted/60",
                            )}
                          >
                            <button
                              type="button"
                              aria-pressed={current?.id === view.id}
                              onClick={() => setSelected(view.id)}
                              className="grid min-w-0 grid-cols-[2rem_minmax(0,1fr)_auto] items-center gap-3 rounded-md px-1 py-1 text-left outline-none focus-visible:ring-1 focus-visible:ring-ring"
                            >
                              <Avatar name={view.name} />
                              <span className="min-w-0">
                                <span className="block truncate text-sm font-medium">
                                  {view.name}
                                </span>
                                <span className="block truncate text-xs text-muted-foreground">
                                  {view.transport} · {serverSummary(view)}
                                </span>
                              </span>
                              <StateBadge view={view} />
                            </button>
                            <Switch
                              size="sm"
                              checked={view.enabled}
                              disabled={write.busy}
                              aria-label={`${view.name} in the active profile`}
                              onCheckedChange={(next) => {
                                const active = profiles.data?.activeProfile;
                                if (active) toggleProfile(view, active, next);
                              }}
                            />
                          </li>
                        ))}
                      </ul>
                    </section>
                  ))}
                </div>
                {current && profiles.data && (
                  <ServerDetail
                    key={`${current.id}:${version}`}
                    view={current}
                    profiles={profiles.data}
                    busy={write.busy}
                    write={write}
                    onChanged={() => setVersion((n) => n + 1)}
                    onEdit={setEditing}
                    onRemove={(info) => setRemoving({ view: current, info })}
                    onToggleProfile={(profileId, on) =>
                      toggleProfile(current, profileId, on)
                    }
                  />
                )}
              </div>
            )}
          </>
        )}
      </Gate>
      {adding && (
        <AddServerDialog
          open
          onOpenChange={setAdding}
          existing={(views ?? []).map((view) => view.name)}
          onInstall={(entry, offline) => {
            setAdding(false);
            write.begin({
              command: "server install",
              title: `Install ${entry.name}`,
              argv: ["server", "install", entry.name, ...(offline ? ["--offline"] : [])],
              planned: installPlan(entry),
            });
          }}
          onCreate={(fields) => {
            setAdding(false);
            write.begin({
              command: "server new",
              title: `Add ${fields.name}`,
              argv: newServerArgv(fields),
              planned: newServerPlan(fields),
            });
          }}
        />
      )}
      {editing && (
        <EditServerDialog
          info={editing}
          open
          onOpenChange={(open) => !open && setEditing(null)}
          onSubmit={(plan) => {
            const info = editing;
            setEditing(null);
            write.begin({
              command: "server edit",
              title: `Edit ${info.name}`,
              argv: plan.argv,
              planned: editServerPlan(info.name, info.id, plan.changes, plan.undo),
              after: () => setVersion((n) => n + 1),
            });
          }}
        />
      )}
      {removing && (
        <RemoveServerDialog
          view={removing.view}
          open
          onOpenChange={(open) => !open && setRemoving(null)}
          onContinue={remove}
        />
      )}
      <WriteDialogs write={write} />
    </div>
  );
}
