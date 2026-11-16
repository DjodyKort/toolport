import { useCallback, useEffect, useRef } from "react";
import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import type { Registry } from "@/lib/types";
import { setMembership } from "./commands";
import { clientName } from "./format";
import { errorText } from "../settings/useCtl";

export interface MembershipChange {
  serverId: string;
  setId: string;
  member: boolean;
}

/** The registry with one membership changed, the way upstream's own `set_access_server` answers. */
export function withMembership(registry: Registry, change: MembershipChange): Registry {
  return {
    ...registry,
    profiles: (registry.profiles ?? []).map((profile) =>
      profile.id === change.setId
        ? {
            ...profile,
            enabledServerIds: [
              ...profile.enabledServerIds.filter((id) => id !== change.serverId),
              ...(change.member ? [change.serverId] : []),
            ],
          }
        : profile,
    ),
  };
}

/** Hands a membership change to upstream's own refresh path, `onRegistryChange`. toolportctl wrote
 * the registry file from another process, so the app's cached registry (what `get_registry` reads)
 * trails it by a watcher tick; applying the change here shows it on upstream's Access page at once,
 * and the watcher's `registry-changed` then confirms it. */
export function useMembershipSync(
  registry: Registry | null,
  onRegistryChange: (registry: Registry) => void,
) {
  const latest = useRef(registry);
  useEffect(() => {
    latest.current = registry;
  }, [registry]);
  return useCallback(
    (change: MembershipChange) => {
      if (!latest.current) return;
      latest.current = withMembership(latest.current, change);
      onRegistryChange(latest.current);
    },
    [onRegistryChange],
  );
}

/** Lists a server in an access set or takes it out, and offers Undo in the toast. A membership
 * change is reversible and takes effect on a client's next connect, so it applies at once. */
export async function changeMembership(
  server: { id: string; name: string },
  set: { id: string; name: string },
  member: boolean,
  done: (change: MembershipChange) => void | Promise<void>,
): Promise<boolean> {
  try {
    await setMembership(server.id, set.id, member);
    await done({ serverId: server.id, setId: set.id, member });
  } catch (error) {
    toastError(`Could not change ${set.name}`, { description: errorText(error) });
    return false;
  }
  toast.success(
    member ? `${server.name} is in ${set.name}` : `${server.name} left ${set.name}`,
    {
      action: {
        label: "Undo",
        onClick: () => void changeMembership(server, set, !member, done),
      },
    },
  );
  return true;
}

export interface ClientBinding {
  clientId: string;
  name: string;
  setId: string;
}

/** The clients that were connected with an access set of their own, by the set they use. The
 * registry names the set by id or by name. */
export function clientBindings(registry: Registry | null): ClientBinding[] {
  if (!registry) return [];
  const sets = registry.profiles ?? [];
  const bindings: ClientBinding[] = [];
  for (const [clientId, scope] of Object.entries(registry.clientScopes ?? {})) {
    const set = sets.find(
      (s) => s.id === scope || s.name.toLowerCase() === scope.toLowerCase(),
    );
    if (set) bindings.push({ clientId, name: clientName(clientId), setId: set.id });
  }
  return bindings;
}
