import type { ProbeResult, Registry, ServerEntry } from "@/lib/types";
import type {
  AccessSetRow,
  AccessSetsData,
  ReachData,
  ReachServer,
  ReachSet,
  ServerInfoData,
} from "./data";
import { clientBindings } from "./membership";

/** Sets whose members are exactly the same servers, so the table can say so instead of leaving two
 * identical columns unexplained. Nothing is merged. */
export function identicalSets(sets: AccessSetRow[]): Map<string, string> {
  const members = (set: AccessSetRow) => [...set.serverIds].sort().join("|");
  const same = new Map<string, string>();
  sets.forEach((set, index) => {
    const twin = sets.slice(0, index).find((other) => members(other) === members(set));
    if (twin) same.set(set.id, twin.name);
  });
  return same;
}

/** The default access lists other servers only, so clients that use it cannot see this one. */
export function leftOutOfDefault(
  enabled: boolean,
  sets: AccessSetsData | null,
  info: ServerInfoData | null,
): boolean {
  if (!enabled || !sets || !info || sets.defaultAccess === null) return false;
  return !info.accessSets.includes(sets.defaultAccess);
}

export interface SetupFacts {
  starts: "checking" | "ok" | "sign-in" | "failed" | "off";
  /** `null` until the vault has been asked. */
  missingSecrets: string[] | null;
  /** Access sets used by a client that leave the server out. */
  narrow: Array<{ id: string; name: string; clients: string[] }>;
  reachKnown: boolean;
}

/** The facts behind the lines of Finish setup: each line is drawn from one of them and disappears
 * when it is true. */
export function setupFacts(
  server: ServerEntry,
  enabled: boolean,
  health: ProbeResult | undefined,
  missingSecrets: string[] | null,
  registry: Registry | null,
  sets: AccessSetsData | null,
): SetupFacts {
  const starts = !enabled
    ? "off"
    : !health
      ? "checking"
      : health.ok
        ? "ok"
        : health.authRequired
          ? "sign-in"
          : "failed";
  const narrow: SetupFacts["narrow"] = [];
  if (sets && enabled) {
    for (const binding of clientBindings(registry)) {
      const set = sets.accessSets.find((s) => s.id === binding.setId);
      if (!set || set.serverIds.includes(server.id)) continue;
      const entry = narrow.find((n) => n.id === set.id);
      if (entry) entry.clients.push(binding.name);
      else narrow.push({ id: set.id, name: set.name, clients: [binding.name] });
    }
  }
  return { starts, missingSecrets, narrow, reachKnown: sets !== null };
}

export function setupDone(facts: SetupFacts): boolean {
  return (
    (facts.starts === "ok" || facts.starts === "off") &&
    facts.missingSecrets !== null &&
    facts.missingSecrets.length === 0 &&
    facts.reachKnown &&
    facts.narrow.length === 0
  );
}

/** Who uses a set, as its column header says it: the default access, the clients bound to it and
 * folder routing; `none` when nothing does. */
export function usedBy(set: ReachSet, reach: ReachData): string {
  const names = new Map(reach.clients.map((client) => [client.id, client.name]));
  const parts = [
    ...(set.default ? ["default"] : []),
    ...set.clients.map((id) => names.get(id) ?? id),
    ...(set.folders.length > 0 ? ["folder routing"] : []),
  ];
  return parts.length > 0 ? parts.join(", ") : "none";
}

/** Names of the clients and folders that reach a server, for the Reach row. */
export function reachedBy(server: ReachServer, reach: ReachData): string[] {
  const names = new Map(reach.clients.map((client) => [client.id, client.name]));
  return [
    ...(server.default ? ["the default access"] : []),
    ...server.clients.map((id) => names.get(id) ?? id),
    ...server.folders.map((path) => `folder ${path}`),
  ];
}
