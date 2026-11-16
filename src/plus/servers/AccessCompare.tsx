import { useRef, type KeyboardEvent } from "react";
import { Check } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { StatusCard } from "@/components/ui/status-card";
import { getRegistry } from "@/lib/api";
import type { Registry } from "@/lib/types";
import { plural } from "../settings/model";
import { listAccessSets, listServers } from "./commands";
import type { AccessSetRow, ServerListRow } from "./data";
import { changeMembership } from "./membership";
import { identicalSets } from "./model";
import { useLoad } from "./useLoad";

const ARROWS: Record<string, [number, number]> = {
  ArrowLeft: [0, -1],
  ArrowRight: [0, 1],
  ArrowUp: [-1, 0],
  ArrowDown: [1, 0],
};

export interface AccessCompareProps {
  registry: Registry | null;
  onRegistryChange: (registry: Registry) => void;
}

/** Servers by access set in one table (Settings > Access). A cell lists the server in the set or
 * takes it out and offers Undo; servers that are off are dimmed because no set reaches them. */
export default function AccessCompare({
  registry,
  onRegistryChange,
}: AccessCompareProps) {
  const servers = useLoad(listServers, registry);
  const sets = useLoad(listAccessSets, registry);
  const grid = useRef<HTMLDivElement>(null);

  if (servers.error || sets.error) {
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<Check />}
        title="The access sets could not be read"
        detail={servers.error ?? sets.error}
        actions={
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              servers.reload();
              sets.reload();
            }}
          >
            Retry
          </Button>
        }
      />
    );
  }
  if (!servers.data || !sets.data) {
    return <p className="text-meta text-muted-foreground">Reading the access sets…</p>;
  }

  const rows = servers.data.servers;
  const columns = sets.data.accessSets;
  const same = identicalSets(columns);

  async function refresh() {
    onRegistryChange(await getRegistry());
    servers.reload();
    sets.reload();
  }

  function toggle(server: ServerListRow, set: AccessSetRow) {
    return changeMembership(server, set, !set.serverIds.includes(server.id), refresh);
  }

  function move(event: KeyboardEvent<HTMLButtonElement>, row: number, col: number) {
    const step = ARROWS[event.key];
    if (!step) return;
    event.preventDefault();
    grid.current
      ?.querySelector<HTMLButtonElement>(
        `[data-cell="${row + step[0]}:${col + step[1]}"]`,
      )
      ?.focus();
  }

  const offBadge = (server: ServerListRow) =>
    !server.enabled && (
      <Badge variant="off" className="ml-2">
        Off
      </Badge>
    );

  return (
    <section
      aria-label="Compare access sets"
      className="@container mt-6 flex flex-col gap-2"
    >
      <div>
        <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
          Compare access sets
        </h2>
        <p className="text-meta text-muted-foreground">
          Servers by access set. A cell lists the server in the set or takes it out, and
          the toast offers Undo. Servers that are off are dimmed: no set reaches them, and
          their memberships are kept.
        </p>
      </div>
      {columns.length === 0 || rows.length === 0 ? (
        <p className="toolport-list px-4 py-3 text-meta text-muted-foreground">
          {rows.length === 0
            ? "There are no servers yet."
            : "There are no access sets yet."}
        </p>
      ) : (
        <div ref={grid} className="toolport-list">
          <table className="hidden w-full text-left text-meta @min-[600px]:table">
            <thead>
              <tr className="border-b border-border">
                <th scope="col" className="px-4 py-2 font-medium">
                  Server
                </th>
                {columns.map((set) => (
                  <th
                    key={set.id}
                    scope="col"
                    className="px-2 py-2 text-center font-medium"
                  >
                    <span className="block">{set.name}</span>
                    <span className="block font-normal text-muted-foreground">
                      {plural(set.serverIds.length, "server")}
                    </span>
                    {same.has(set.id) && (
                      <Badge variant="info">same as {same.get(set.id)}</Badge>
                    )}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((server, row) => (
                <tr
                  key={server.id}
                  className={`border-b border-border last:border-b-0 ${server.enabled ? "" : "opacity-60"}`}
                >
                  <th scope="row" className="px-4 py-1.5 font-medium">
                    {server.name}
                    {offBadge(server)}
                  </th>
                  {columns.map((set, col) => {
                    const member = set.serverIds.includes(server.id);
                    return (
                      <td key={set.id} className="px-2 py-1.5 text-center">
                        <Button
                          data-cell={`${row}:${col}`}
                          variant={member ? "secondary" : "ghost"}
                          size="icon-sm"
                          aria-pressed={member}
                          aria-label={`${server.name} in ${set.name}`}
                          onKeyDown={(event) => move(event, row, col)}
                          onClick={() => void toggle(server, set)}
                        >
                          {member ? (
                            <Check />
                          ) : (
                            <span aria-hidden="true" className="text-muted-foreground">
                              ·
                            </span>
                          )}
                        </Button>
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
          <ul className="@min-[600px]:hidden">
            {rows.map((server) => (
              <li
                key={server.id}
                className={`flex flex-col gap-1.5 border-b border-border px-4 py-2.5 last:border-b-0 ${server.enabled ? "" : "opacity-60"}`}
              >
                <span className="text-sm font-medium">
                  {server.name}
                  {offBadge(server)}
                </span>
                <span className="flex flex-wrap gap-1.5">
                  {columns.map((set) => {
                    const member = set.serverIds.includes(server.id);
                    return (
                      <Button
                        key={set.id}
                        variant={member ? "secondary" : "outline"}
                        size="xs"
                        aria-pressed={member}
                        aria-label={`${server.name} in ${set.name} (list)`}
                        onClick={() => void toggle(server, set)}
                      >
                        {member && <Check />}
                        {set.name}
                      </Button>
                    );
                  })}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}
