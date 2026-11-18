import type { ReachData, ReachFinding, ReachServer } from "../servers/data";

export type ClientReachState = "through" | "no-gateway" | "unlisted" | "set-deleted";

export interface ClientReachView {
  state: ClientReachState;
  /** The access set the client uses: its own, or the default access it follows. */
  setName: string | null;
  followsDefault: boolean;
  /** Servers that are on and that the client's set lists. */
  reached: ReachServer[];
  /** Servers that are on and that the client's set leaves out. */
  notReached: ReachServer[];
  /** Another access set than the default one, listing exactly the default's servers. */
  sameAsDefault: boolean;
  findings: ReachFinding[];
}

/** What `client reach` says about one client, cut to what its detail page shows. `null` while the
 * report is not loaded. A client the report does not list holds no gateway entry and has no set. */
export function clientReach(
  reach: ReachData | null,
  clientId: string,
): ClientReachView | null {
  if (!reach) return null;
  const client = reach.clients.find((c) => c.id === clientId);
  if (!client) {
    return {
      state: "unlisted",
      setName: null,
      followsDefault: false,
      reached: [],
      notReached: [],
      sameAsDefault: false,
      findings: [],
    };
  }
  const reachedIds = new Set(client.servers);
  const on = reach.servers.filter((server) => server.enabled);
  const defaultServers = new Set(reach.defaultAccess?.servers ?? []);
  const sameAsDefault =
    client.ownSet &&
    !client.missing &&
    client.accessSet !== reach.defaultAccess?.accessSet &&
    client.servers.length === defaultServers.size &&
    client.servers.every((id) => defaultServers.has(id));
  return {
    state: client.missing ? "set-deleted" : client.gateway ? "through" : "no-gateway",
    setName: client.accessSetName,
    followsDefault: !client.ownSet,
    reached: on.filter((server) => reachedIds.has(server.id)),
    notReached: on.filter((server) => !reachedIds.has(server.id)),
    sameAsDefault,
    findings: reach.findings.filter(
      (finding) =>
        finding.client === clientId &&
        (finding.kind === "folder-outranks" || finding.kind === "missing-set"),
    ),
  };
}
