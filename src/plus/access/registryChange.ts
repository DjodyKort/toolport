import type { Registry } from "@/lib/types";
import type { DiscoveryMode, WriteOutcome } from "./data";

/** What a write did to the registry, in the terms of upstream's own fields. */
export type AccessChange =
  | { kind: "set-added"; id: string; name: string; serverIds: string[] }
  | { kind: "set-removed"; id: string; detach: boolean }
  | { kind: "default"; id: string | null }
  | { kind: "client-scope"; client: string; setId: string | null }
  | { kind: "folder-route"; path: string; setId: string | null }
  | { kind: "tools"; setId: string; serverId: string; tools: string[] | null }
  | { kind: "discovery"; client: string; mode: DiscoveryMode };

const same = (a: string, b: string) => a.trim() === b.trim();

/** The registry with one write applied, the way upstream's own functions leave it. toolportctl
 * wrote the file from another process, so the app's copy trails it by a watcher tick; handing the
 * result to `onRegistryChange` shows the write on upstream's Access page at once. */
export function applyChange(registry: Registry, change: AccessChange): Registry {
  switch (change.kind) {
    case "set-added":
      return {
        ...registry,
        profiles: [
          ...(registry.profiles ?? []),
          { id: change.id, name: change.name, enabledServerIds: change.serverIds },
        ],
      };
    case "set-removed": {
      const names = (registry.profiles ?? [])
        .filter((p) => p.id === change.id)
        .flatMap((p) => [p.id, p.name.toLowerCase()]);
      const uses = (reference: string) => names.includes(reference.trim().toLowerCase());
      return {
        ...registry,
        profiles: (registry.profiles ?? []).filter((p) => p.id !== change.id),
        ...(change.detach && {
          clientScopes: Object.fromEntries(
            Object.entries(registry.clientScopes ?? {}).filter(([, ref]) => !uses(ref)),
          ),
          folderProfiles: (registry.folderProfiles ?? []).filter((f) => !uses(f.profile)),
        }),
      };
    }
    case "default":
      return { ...registry, defaultAccessProfileId: change.id };
    case "client-scope": {
      const scopes = { ...registry.clientScopes };
      if (change.setId === null) delete scopes[change.client];
      else scopes[change.client] = change.setId;
      return { ...registry, clientScopes: scopes };
    }
    case "folder-route": {
      const rest = (registry.folderProfiles ?? []).filter(
        (f) => !same(f.path, change.path),
      );
      return {
        ...registry,
        folderProfiles:
          change.setId === null
            ? rest
            : [...rest, { path: change.path, profile: change.setId }],
      };
    }
    case "tools":
      return {
        ...registry,
        profiles: (registry.profiles ?? []).map((p) => {
          if (p.id !== change.setId) return p;
          const scope = { ...p.toolScope };
          if (change.tools === null) delete scope[change.serverId];
          else scope[change.serverId] = change.tools;
          return { ...p, toolScope: scope };
        }),
      };
    case "discovery": {
      const modes = { ...registry.clientDiscovery };
      if (change.mode === "inherit") delete modes[change.client];
      else modes[change.client] = change.mode;
      return { ...registry, clientDiscovery: modes };
    }
  }
}

/** The change a successful apply made, read from the writer's answer. */
export function changeOf(
  kind: AccessChange["kind"],
  outcome: WriteOutcome,
  context: Record<string, unknown> = {},
): AccessChange | null {
  if (!outcome.result?.applied) return null;
  const set = outcome.accessSet ?? null;
  const members = set?.servers?.map((s) => s.id) ?? [];
  switch (kind) {
    case "set-added":
      return set
        ? {
            kind,
            id: set.id,
            name: set.name,
            serverIds: members,
          }
        : null;
    case "set-removed":
      return set ? { kind, id: set.id, detach: context.detach === true } : null;
    case "default":
      return { kind, id: outcome.defaultAccess?.id ?? null };
    case "client-scope":
      return outcome.client
        ? { kind, client: outcome.client, setId: set?.id ?? null }
        : null;
    case "folder-route":
      return outcome.path ? { kind, path: outcome.path, setId: set?.id ?? null } : null;
    case "tools":
      return set && outcome.server
        ? {
            kind,
            setId: set.id,
            serverId: outcome.server.id,
            tools: outcome.tools ?? null,
          }
        : null;
    case "discovery":
      return outcome.client && outcome.mode
        ? { kind, client: outcome.client, mode: outcome.mode as DiscoveryMode }
        : null;
  }
}
