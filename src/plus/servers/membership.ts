import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import type { Registry } from "@/lib/types";
import { setMembership } from "./commands";
import { clientName } from "./format";
import { errorText } from "../settings/useCtl";

/** Lists a server in an access set or takes it out, and offers Undo in the toast. A membership
 * change is reversible and takes effect on a client's next connect, so it applies at once. */
export async function changeMembership(
  server: { id: string; name: string },
  set: { id: string; name: string },
  member: boolean,
  done: () => void | Promise<void>,
): Promise<boolean> {
  try {
    await setMembership(server.id, set.id, member);
    await done();
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
