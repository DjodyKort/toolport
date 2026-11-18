import { useState } from "react";
import { ChevronRight, Network } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { DetectedClient } from "@/lib/types";
import { plural } from "../settings/model";
import { loadReach } from "../servers/commands";
import { OperationRow } from "../servers/OperationRow";
import { useLoad } from "../servers/useLoad";
import { openSettingsPage } from "../shell/navigate";
import { clientReach, type ClientReachView } from "./reachModel";

const UNREACHED_BY_ANY = "No access set reaches it";

function summary(view: ClientReachView): string {
  const on = view.reached.length + view.notReached.length;
  const set = view.followsDefault
    ? `Follows the default access${view.setName ? ` (${view.setName})` : ""}`
    : `Access set ${view.setName ?? "of its own"}`;
  return `${set} · reaches ${view.reached.length} of the ${plural(on, "server")} that ${on === 1 ? "is" : "are"} on`;
}

function Names({
  label,
  servers,
  note,
}: {
  label: string;
  servers: { id: string; name: string; unreached: boolean }[];
  note?: boolean;
}) {
  return (
    <div>
      <p className="mb-1 text-meta font-semibold text-muted-foreground">
        {label} {servers.length}
      </p>
      {servers.length === 0 ? (
        <p className="text-meta text-muted-foreground">Nothing.</p>
      ) : (
        <ul aria-label={label} className="flex flex-wrap gap-1.5">
          {servers.map((server) => (
            <li
              key={server.id}
              className="rounded-full border border-border px-2 py-px text-xs break-all"
            >
              {server.name}
              {note && server.unreached && (
                <>
                  {" · "}
                  <b className="font-semibold">{UNREACHED_BY_ANY}</b>
                </>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/** Which servers the access set of one client reaches and which it leaves out, in the words of the
 * Access check of Attention and of the Reach row of a server. */
export default function ClientReach({ client }: { client: DetectedClient }) {
  const reach = useLoad(loadReach, client.id);
  const [open, setOpen] = useState<boolean | null>(null);
  const view = clientReach(reach.data, client.id);
  const title = `How ${client.name} reaches your servers`;

  if (!view) {
    return (
      <div className="toolport-list" aria-label="Reach">
        <OperationRow
          icon={<Network />}
          title={title}
          sub={
            reach.error ? `Could not read who reaches what: ${reach.error}` : "Reading…"
          }
          tone={reach.error ? "danger" : undefined}
        />
      </div>
    );
  }

  const attention =
    view.state === "set-deleted" ||
    view.findings.length > 0 ||
    (view.state === "through" && view.notReached.length > 0);
  const isOpen = open ?? attention;
  const lists = view.state !== "unlisted";

  const sub =
    view.state === "unlisted"
      ? "No Toolport gateway entry in its config file, so no access set applies to it"
      : view.state === "set-deleted"
        ? (view.findings.find((f) => f.kind === "missing-set")?.text ??
          "Its access set no longer exists, so it sees no servers.")
        : view.state === "no-gateway"
          ? `${summary(view)} · applies once the gateway entry is back`
          : summary(view);

  return (
    <div className="toolport-list" aria-label="Reach">
      <OperationRow
        icon={<Network />}
        title={title}
        sub={sub}
        tone={attention ? "warning" : undefined}
        badge={
          view.state === "through" ? (
            <Badge variant="success">Through Toolport</Badge>
          ) : view.state === "set-deleted" ? (
            <Badge variant="destructive">Set deleted</Badge>
          ) : (
            <Badge variant="off">Not connected</Badge>
          )
        }
        open={lists && isOpen}
        onToggle={lists ? () => setOpen(!isOpen) : undefined}
      >
        <Names label="Reaches" servers={view.reached} />
        <Names label="Does not reach" servers={view.notReached} note />
        {view.sameAsDefault && (
          <p className="text-meta text-muted-foreground">
            Same list as the default access: both reach the same{" "}
            {plural(view.reached.length, "server")}. Changing one does not change the
            other.
          </p>
        )}
        {view.findings
          .filter((finding) => finding.kind !== "missing-set")
          .map((finding) => (
            <p
              key={`${finding.kind}:${finding.folder ?? ""}`}
              className="text-meta text-muted-foreground"
            >
              {finding.text}
            </p>
          ))}
        <div>
          <Button
            variant="ghost"
            size="sm"
            onClick={() =>
              openSettingsPage({ page: "access", section: "Compare access sets" })
            }
          >
            Compare access sets <ChevronRight />
          </Button>
        </div>
      </OperationRow>
    </div>
  );
}
