import { useState } from "react";
import { Check, ChevronRight, Network } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { Registry, ServerEntry } from "@/lib/types";
import { plural } from "../settings/model";
import type { AccessSetsData, ServerInfoData } from "./data";
import { changeMembership, clientBindings, type MembershipChange } from "./membership";
import { leftOutOfDefault } from "./model";
import { OperationRow } from "./OperationRow";
import type { Loaded } from "./useLoad";

export interface ReachRowProps {
  server: ServerEntry;
  enabled: boolean;
  registry: Registry | null;
  info: Loaded<ServerInfoData>;
  sets: Loaded<AccessSetsData>;
  /** Runs after a membership changed, so the app's registry and both readers follow. */
  onChanged: (change: MembershipChange) => void | Promise<void>;
  onOpenAccessSets: () => void;
}

/** Who can reach a server: the access sets that list it, whether the default one does, and the
 * clients that use a set of their own. A set cell is a switch with Undo. */
export function ReachRow({
  server,
  enabled,
  registry,
  info,
  sets,
  onChanged,
  onOpenAccessSets,
}: ReachRowProps) {
  const [open, setOpen] = useState<boolean | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const data = sets.data;
  const listed = new Set(info.data?.accessSets ?? []);

  if (!data) {
    return (
      <OperationRow
        icon={<Network />}
        title="Who can reach it"
        sub={
          sets.error
            ? `Could not read the access sets: ${sets.error}`
            : "Reading the access sets…"
        }
        tone={sets.error ? "danger" : undefined}
      />
    );
  }

  const defaultSet = data.accessSets.find((s) => s.id === data.defaultAccess);
  const leftOut = leftOutOfDefault(enabled, data, info.data);
  const defaultName = defaultSet ? defaultSet.name : "every enabled server";
  const bindings = clientBindings(registry);
  const isOpen = open ?? leftOut;

  let sub = `Default access: ${defaultName} · listed in ${listed.size} of ${plural(data.accessSets.length, "access set")}`;
  if (!enabled) sub = "Switched off · no client reaches it, and its access sets are kept";
  else if (leftOut) sub = `Clients on the default access (${defaultName}) cannot see it`;

  async function toggle(set: { id: string; name: string }, member: boolean) {
    setOpen(true);
    setBusy(set.id);
    await changeMembership(server, set, member, onChanged);
    setBusy(null);
  }

  return (
    <OperationRow
      icon={<Network />}
      title="Who can reach it"
      sub={sub}
      tone={leftOut ? "warning" : undefined}
      badge={
        !enabled ? (
          <Badge variant="off">Off</Badge>
        ) : leftOut ? (
          <Badge variant="warning">Left out</Badge>
        ) : (
          <Badge variant="info">
            {listed.size} of {data.accessSets.length}
          </Badge>
        )
      }
      open={isOpen}
      onToggle={() => setOpen(!isOpen)}
    >
      {data.accessSets.length === 0 ? (
        <p className="text-meta text-muted-foreground">
          There are no access sets, so every client sees every enabled server.
        </p>
      ) : (
        <ul className="flex flex-col divide-y divide-border rounded-lg border border-border">
          {data.accessSets.map((set) => {
            const member = listed.has(set.id);
            const users = bindings.filter((b) => b.setId === set.id).map((b) => b.name);
            return (
              <li
                key={set.id}
                className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2"
              >
                <span className="min-w-0 flex-1 basis-[180px]">
                  <span className="block text-sm font-medium break-words">
                    {set.name}
                    {set.id === data.defaultAccess && (
                      <span className="text-meta font-normal text-muted-foreground">
                        {" "}
                        · default
                      </span>
                    )}
                  </span>
                  <span className="block text-meta text-muted-foreground">
                    {plural(set.serverIds.length, "server")}
                    {users.length > 0 && ` · used by ${users.join(", ")}`}
                  </span>
                </span>
                <Button
                  variant={member ? "secondary" : "outline"}
                  size="sm"
                  aria-pressed={member}
                  aria-label={`${server.name} in ${set.name}`}
                  disabled={busy === set.id}
                  onClick={() => void toggle(set, !member)}
                >
                  {member ? <Check /> : null}
                  {member ? "In set" : "Add to set"}
                </Button>
              </li>
            );
          })}
        </ul>
      )}
      <div>
        <Button variant="ghost" size="sm" onClick={onOpenAccessSets}>
          Compare access sets <ChevronRight />
        </Button>
      </div>
    </OperationRow>
  );
}
