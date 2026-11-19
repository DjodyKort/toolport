import type { AgentStatusData, StatusData, StyleStatusData } from "./data";
import { TARGETS } from "./model";

export type ContentKindName = "skills" | "rules" | "agents" | "styles";

export const CONTENT_KINDS: ContentKindName[] = ["skills", "rules", "agents", "styles"];

export interface InstalledItem {
  name: string;
  /** False when the lock says the file was written and it is gone. */
  present: boolean;
}

export interface ClientContentRow {
  client: string;
  label: string;
  items: Record<ContentKindName, InstalledItem[]>;
}

const empty = (): ClientContentRow["items"] => ({
  skills: [],
  rules: [],
  agents: [],
  styles: [],
});

const byName = (a: InstalledItem, b: InstalledItem) => a.name.localeCompare(b.name);

/** Which skill, rule, agent and style is installed in which client, read from the three status
 * answers the Library already asks for. A client is listed when it holds something or when the
 * sync targets it, so a target with nothing in it shows as empty rather than missing. */
export function clientContent(
  skills: StatusData | null,
  agents: AgentStatusData | null,
  styles: StyleStatusData | null,
): ClientContentRow[] {
  const rows = new Map<string, ClientContentRow>();
  const labels = new Map<string, string>(TARGETS.map((t) => [t.id, t.label]));
  for (const row of [...(styles?.native ?? []), ...(styles?.applyRemove ?? [])]) {
    labels.set(row.client, row.name);
  }
  const row = (client: string) => {
    let found = rows.get(client);
    if (!found) {
      found = { client, label: labels.get(client) ?? client, items: empty() };
      rows.set(client, found);
    }
    return found;
  };

  for (const client of skills?.targetedClients ?? []) row(client);
  const rules = new Set(
    (skills?.entries ?? []).filter((e) => e.type === "rule").map((e) => e.name),
  );
  for (const out of skills?.outputs ?? []) {
    row(out.client).items[rules.has(out.name) ? "rules" : "skills"].push({
      name: out.name,
      present: out.present,
    });
  }
  for (const out of agents?.outputs ?? []) {
    row(out.client).items.agents.push({ name: out.name, present: out.present });
  }
  for (const native of styles?.native ?? []) {
    if (native.styles.length === 0) continue;
    row(native.client).items.styles.push(
      ...native.styles.map((name) => ({ name, present: true })),
    );
  }
  for (const applied of styles?.applyRemove ?? []) {
    if (applied.active) {
      row(applied.client).items.styles.push({ name: applied.active, present: true });
    }
  }

  const order = TARGETS.map((t) => t.id as string);
  const rank = (client: string) => {
    const at = order.indexOf(client);
    return at < 0 ? order.length : at;
  };
  return [...rows.values()]
    .map((r) => {
      for (const kind of CONTENT_KINDS) r.items[kind].sort(byName);
      return r;
    })
    .sort((a, b) => rank(a.client) - rank(b.client) || a.label.localeCompare(b.label));
}

export const total = (row: ClientContentRow) =>
  CONTENT_KINDS.reduce((sum, kind) => sum + row.items[kind].length, 0);

export const missing = (row: ClientContentRow) =>
  CONTENT_KINDS.reduce(
    (sum, kind) => sum + row.items[kind].filter((i) => !i.present).length,
    0,
  );
