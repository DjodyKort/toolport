import { useEffect, useRef, useState } from "react";
import { Check } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import { toastError } from "@/lib/toast";
import type { Registry } from "@/lib/types";
import { Group, RowShell } from "../context/StackRows";
import { plural } from "../settings/model";
import { errorText } from "../settings/useCtl";
import { listServers, loadReach } from "../servers/commands";
import type { ReachClient, ReachData, ServerListData } from "../servers/data";
import { useLoad } from "../servers/useLoad";
import {
  addSet,
  bindClient,
  listNarrowed,
  listRoutes,
  listSets,
  makeDefault,
  removeSet,
  routeFolder,
  setDiscovery,
  setTools,
  unrouteFolder,
  type Orphans,
  type ToolChange,
} from "./commands";
import {
  DISCOVERY_MODES,
  type AccessSetListData,
  type AccessSetRow,
  type DiscoveryMode,
  type NamedRef,
  type NarrowedServer,
  type RouteRow,
  type RoutesData,
} from "./data";
import { ChoiceForm, DeleteChoiceForm, NewSetForm, RouteForm, ToolsForm } from "./forms";
import { applyChange, changeOf } from "./registryChange";
import { runUndo } from "./undo";
import WriteDialog, { type Write } from "./WriteDialog";

export interface AccessWriteProps {
  registry: Registry | null;
  onRegistryChange: (registry: Registry) => void;
}

type Open =
  | { kind: "new-set" }
  | { kind: "delete-choice"; set: AccessSetRow }
  | { kind: "tools"; set: AccessSetRow; narrowed: NarrowedServer[] }
  | { kind: "client-set"; client: ReachClient }
  | { kind: "discovery"; client: ReachClient; mode: DiscoveryMode }
  | { kind: "route-new" }
  | { kind: "route-set"; route: RouteRow }
  | { kind: "plan"; write: Write };

interface Everything {
  sets: AccessSetListData;
  routes: RoutesData;
  /** Reach and the server list only decorate the page, so a failed read leaves them out. */
  reach: ReachData | null;
  servers: ServerListData | null;
}

async function loadEverything(): Promise<Everything> {
  const [sets, routes, reach, servers] = await Promise.all([
    listSets(),
    listRoutes(),
    loadReach().catch(() => null),
    listServers().catch(() => null),
  ]);
  return { sets, routes, reach, servers };
}

function usedBy(set: AccessSetRow): string {
  const parts = [
    set.clients.length > 0 && plural(set.clients.length, "client"),
    set.folders.length > 0 && plural(set.folders.length, "folder route"),
  ].filter(Boolean);
  return parts.length > 0
    ? `used by ${parts.join(" and ")}`
    : "not used by a client or folder";
}

const NO_OWN_SET = "";

/** The writers behind `access-set`, `client scope` and `folder route` (D-152), with the plan first
 * and Undo after (D-059): create, delete and tool-narrow access sets, choose the default access,
 * bind a client to a set, pick its tool listing and route folders. Sits under the compare table on
 * Settings > Access, beside upstream's own cards. */
export default function AccessWrite({ registry, onRegistryChange }: AccessWriteProps) {
  const all = useLoad(loadEverything, "access-write");
  const [open, setOpen] = useState<Open | null>(null);
  const latest = useRef(registry);
  useEffect(() => {
    latest.current = registry;
  }, [registry]);
  const close = () => setOpen(null);
  const plan = (write: Write) => setOpen({ kind: "plan", write });

  if (all.error && !all.data) {
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<Check />}
        title="The access sets could not be read for changing"
        detail={all.error}
        actions={
          <Button variant="outline" size="sm" onClick={all.reload}>
            Retry
          </Button>
        }
      />
    );
  }
  if (!all.data) {
    return <p className="text-meta text-muted-foreground">Reading the access sets…</p>;
  }

  const { sets, routes, reach, servers } = all.data;
  const defaultSet = sets.accessSets.find((set) => set.default);
  const clients = (reach?.clients ?? []).filter(
    (client) => client.gateway || client.ownSet,
  );

  function applied(write: Write, outcome: Awaited<ReturnType<Write["run"]>>) {
    const change = write.change(outcome);
    if (change && latest.current) {
      latest.current = applyChange(latest.current, change);
      onRegistryChange(latest.current);
    }
    all.reload();
    if (!outcome.result?.applied) {
      toast.message("Nothing changed", { description: outcome.plan.summary });
      return;
    }
    const { undo } = outcome.result;
    toast.success(write.done, {
      duration: 10_000,
      action: {
        label: "Undo",
        onClick: () => {
          runUndo(undo).then(all.reload, (error: unknown) =>
            toastError("Could not undo", { description: errorText(error) }),
          );
        },
      },
    });
  }

  async function editTools(set: AccessSetRow) {
    try {
      const narrowed = await listNarrowed();
      setOpen({
        kind: "tools",
        set,
        narrowed:
          narrowed.accessSets.find((entry) => entry.id === set.id)?.narrowed ?? [],
      });
    } catch (error) {
      toastError("Could not read the tool lists", { description: errorText(error) });
    }
  }

  function deleteSet(set: AccessSetRow, orphans: Orphans) {
    plan({
      title: `Delete ${set.name}`,
      description: "The plan comes first. Nothing is deleted until you confirm.",
      confirmLabel: "Delete access set",
      destructive: true,
      run: (dry) => removeSet(set.id, orphans, dry),
      change: (outcome) =>
        changeOf("set-removed", outcome, { detach: orphans === "detach" }),
      done: `Deleted ${set.name}`,
    });
  }

  function setDefault(set: AccessSetRow | null) {
    plan({
      title: set ? `Make ${set.name} the default access` : "Use every enabled server",
      confirmLabel: set ? "Make default" : "Use every enabled server",
      run: (dry) => makeDefault(set?.id ?? null, dry),
      change: (outcome) => changeOf("default", outcome),
      done: set
        ? `${set.name} is the default access`
        : "The default access is every enabled server",
    });
  }

  function narrow(set: AccessSetRow, server: NamedRef, change: ToolChange) {
    plan({
      title: `Tools of ${server.name} in ${set.name}`,
      confirmLabel: "Change the tools",
      run: (dry) => setTools(set.id, server.id, change, dry),
      change: (outcome) => changeOf("tools", outcome),
      done: `Changed the tools of ${server.name} in ${set.name}`,
    });
  }

  function bind(client: ReachClient, setId: string) {
    const set = sets.accessSets.find((entry) => entry.id === setId);
    plan({
      title: `${client.name}: access set`,
      confirmLabel: set ? `Bind to ${set.name}` : "Follow the default access",
      run: (dry) => bindClient(client.id, set?.id ?? null, dry),
      change: (outcome) => changeOf("client-scope", outcome),
      done: set
        ? `${client.name} uses ${set.name}`
        : `${client.name} follows the default access`,
    });
  }

  function listing(client: ReachClient, mode: DiscoveryMode) {
    plan({
      title: `${client.name}: tool listing`,
      description:
        "Takes effect at the client's next connection. Nothing is written until you confirm.",
      confirmLabel: "Set the tool listing",
      run: (dry) => setDiscovery(client.id, mode, dry),
      change: (outcome) => changeOf("discovery", outcome),
      done: `${client.name} lists tools as: ${DISCOVERY_MODES.find((m) => m.mode === mode)?.label ?? mode}`,
    });
  }

  function route(path: string, setId: string) {
    const set = sets.accessSets.find((entry) => entry.id === setId);
    plan({
      title: `Route ${path}`,
      confirmLabel: "Route the folder",
      run: (dry) => routeFolder(path, setId, dry),
      change: (outcome) => changeOf("folder-route", outcome),
      done: `${path} uses ${set?.name ?? setId}`,
    });
  }

  function unroute(row: RouteRow) {
    plan({
      title: `Remove the route of ${row.path}`,
      confirmLabel: "Remove the route",
      destructive: true,
      run: (dry) => unrouteFolder(row.path, dry),
      change: (outcome) => changeOf("folder-route", outcome),
      done: `${row.path} is no longer routed`,
    });
  }

  const setOptions = sets.accessSets.map((set) => ({ value: set.id, label: set.name }));
  const modeOf = (client: ReachClient): DiscoveryMode => {
    const stored = registry?.clientDiscovery?.[client.id]?.trim().toLowerCase();
    return DISCOVERY_MODES.some((m) => m.mode === stored)
      ? (stored as DiscoveryMode)
      : "inherit";
  };
  const modeLabel = (mode: DiscoveryMode) =>
    mode === "inherit"
      ? "global mode"
      : (DISCOVERY_MODES.find((m) => m.mode === mode)?.label ?? mode);

  return (
    <section aria-label="Change access sets" className="mt-6 flex flex-col gap-4">
      <div>
        <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
          Change access sets
        </h2>
        <p className="text-meta text-muted-foreground">
          Create and delete sets, choose the default access, bind clients and route
          folders. Every change shows its plan first and offers Undo.
        </p>
      </div>

      <Group
        title="Access sets"
        count={sets.accessSets.length}
        action={
          <Button
            size="sm"
            variant="outline"
            onClick={() => setOpen({ kind: "new-set" })}
          >
            New access set…
          </Button>
        }
      >
        <RowShell
          title="Default access"
          subtitle={
            defaultSet
              ? `Clients without a set of their own use ${defaultSet.name}.`
              : "Clients without a set of their own reach every enabled server."
          }
          trailing={
            defaultSet && (
              <Button size="sm" variant="outline" onClick={() => setDefault(null)}>
                Use every enabled server…
              </Button>
            )
          }
        />
        {sets.accessSets.length === 0 && (
          <RowShell
            title="No access sets"
            subtitle="A set lists the servers a client or folder can reach."
          />
        )}
        {sets.accessSets.map((set) => (
          <RowShell
            key={set.id}
            title={set.name}
            badge={set.default ? <Badge variant="info">default</Badge> : undefined}
            subtitle={`${plural(set.servers.length, "server")}, ${set.reached} on · ${usedBy(set)}`}
            trailing={
              <>
                {!set.default && (
                  <Button
                    size="sm"
                    variant="outline"
                    aria-label={`Make ${set.name} the default access`}
                    onClick={() => setDefault(set)}
                  >
                    Make default…
                  </Button>
                )}
                <Button
                  size="sm"
                  variant="outline"
                  aria-label={`Choose the tools of ${set.name}`}
                  disabled={set.servers.length === 0}
                  onClick={() => void editTools(set)}
                >
                  Tools…
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  aria-label={`Delete ${set.name}`}
                  disabled={set.default || sets.accessSets.length <= 1}
                  title={
                    set.default
                      ? "The default access cannot be deleted; choose another default first."
                      : sets.accessSets.length <= 1
                        ? "The only access set cannot be deleted."
                        : undefined
                  }
                  onClick={() =>
                    set.clients.length + set.folders.length > 0
                      ? setOpen({ kind: "delete-choice", set })
                      : deleteSet(set, null)
                  }
                >
                  Delete…
                </Button>
              </>
            }
          />
        ))}
      </Group>

      <Group title="Clients" count={clients.length}>
        {!reach && (
          <RowShell
            title="The clients could not be read"
            subtitle="Binding a client needs the reach report; try again from Compare access sets."
          />
        )}
        {reach && clients.length === 0 && (
          <RowShell
            title="No client holds Toolport's gateway entry"
            subtitle="Connect a client from Clients first."
          />
        )}
        {clients.map((client) => (
          <RowShell
            key={client.id}
            title={client.name}
            subtitle={`${
              client.ownSet
                ? client.missing
                  ? "bound to an access set that no longer exists"
                  : `uses ${client.accessSetName ?? client.accessSet}`
                : "follows the default access"
            } · tool listing: ${modeLabel(modeOf(client))}`}
            trailing={
              <>
                <Button
                  size="sm"
                  variant="outline"
                  aria-label={`Change the access set of ${client.name}`}
                  onClick={() => setOpen({ kind: "client-set", client })}
                >
                  Access set…
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  aria-label={`Change the tool listing of ${client.name}`}
                  onClick={() =>
                    setOpen({ kind: "discovery", client, mode: modeOf(client) })
                  }
                >
                  Tool listing…
                </Button>
              </>
            }
          />
        ))}
      </Group>

      <Group
        title="Folder routes"
        count={routes.routes.length}
        action={
          <Button
            size="sm"
            variant="outline"
            disabled={sets.accessSets.length === 0}
            onClick={() => setOpen({ kind: "route-new" })}
          >
            Route a folder…
          </Button>
        }
      >
        {routes.routes.length === 0 && (
          <RowShell
            title="No folder is routed"
            subtitle="A route sends every client started in a folder, and below it, to one access set."
          />
        )}
        {routes.routes.map((row) => (
          <RowShell
            key={row.path}
            title={<span className="font-mono break-all">{row.path}</span>}
            badge={row.missing ? <Badge variant="warning">set deleted</Badge> : undefined}
            subtitle={
              row.missing
                ? `names ${row.reference}, which no longer exists, so clients there see no servers`
                : `${row.accessSet?.name ?? row.reference} · ${plural(row.servers.length, "server")}`
            }
            trailing={
              <>
                <Button
                  size="sm"
                  variant="outline"
                  aria-label={`Change the access set of ${row.path}`}
                  onClick={() => setOpen({ kind: "route-set", route: row })}
                >
                  Access set…
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  aria-label={`Remove the route of ${row.path}`}
                  onClick={() => unroute(row)}
                >
                  Remove…
                </Button>
              </>
            }
          />
        ))}
      </Group>
      <p className="text-meta text-muted-foreground">{routes.rule}</p>

      {open?.kind === "new-set" && (
        <NewSetForm
          servers={servers?.servers ?? []}
          existing={sets.accessSets.flatMap((set) => [set.name, set.id])}
          onCancel={close}
          onPreview={(name, serverIds) =>
            plan({
              title: "New access set",
              confirmLabel: "Add access set",
              run: (dry) => addSet(name, serverIds, dry),
              change: (outcome) => changeOf("set-added", outcome),
              done: `Added ${name}`,
            })
          }
        />
      )}
      {open?.kind === "delete-choice" && (
        <DeleteChoiceForm
          set={open.set}
          onCancel={close}
          onPreview={(orphans) => deleteSet(open.set, orphans)}
        />
      )}
      {open?.kind === "tools" && (
        <ToolsForm
          set={open.set}
          narrowed={open.narrowed}
          onCancel={close}
          onPreview={(server, change) => narrow(open.set, server, change)}
        />
      )}
      {open?.kind === "client-set" && (
        <ChoiceForm
          title={`${open.client.name}: access set`}
          description="The set this client reaches servers through. A folder route still outranks it."
          label="Access set"
          options={[
            { value: NO_OWN_SET, label: "Follow the default access" },
            ...setOptions,
          ]}
          initial={
            open.client.ownSet ? (open.client.accessSet ?? NO_OWN_SET) : NO_OWN_SET
          }
          onCancel={close}
          onPreview={(value) => bind(open.client, value)}
        />
      )}
      {open?.kind === "discovery" && (
        <ChoiceForm
          title={`${open.client.name}: tool listing`}
          description="How this client is shown the tools. Takes effect at its next connection."
          label="Tool listing"
          options={DISCOVERY_MODES.map((m) => ({
            value: m.mode,
            label: `${m.label}: ${m.hint}`,
          }))}
          initial={open.mode}
          onCancel={close}
          onPreview={(value) => listing(open.client, value as DiscoveryMode)}
        />
      )}
      {open?.kind === "route-new" && (
        <RouteForm sets={sets.accessSets} onCancel={close} onPreview={route} />
      )}
      {open?.kind === "route-set" && (
        <ChoiceForm
          title={`Route ${open.route.path}`}
          description="Inside this folder, and below it, the gateway uses this set for every client."
          label="Access set"
          options={setOptions}
          initial={open.route.accessSet?.id ?? setOptions[0]?.value ?? ""}
          onCancel={close}
          onPreview={(value) => route(open.route.path, value)}
        />
      )}
      {open?.kind === "plan" && (
        <WriteDialog write={open.write} onClose={close} onApplied={applied} />
      )}
    </section>
  );
}
