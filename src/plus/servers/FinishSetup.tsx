import { useEffect } from "react";
import {
  CircleAlert,
  CircleCheck,
  KeyRound,
  Network,
  Pencil,
  Power,
  RotateCw,
} from "lucide-react";
import { toast } from "sonner";
import { LazySecretsDialog } from "@/components/LazySecretsDialog";
import { LazyServerDialog } from "@/components/LazyServerDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { SectionHeader } from "@/components/ui/section-header";
import { secretStatus } from "@/lib/api";
import { errorHeadline, shortenUrls } from "@/lib/errors";
import type { ProbeResult, Registry, ServerEntry } from "@/lib/types";
import { plural } from "../settings/model";
import type { AccessSetsData } from "./data";
import { changeMembership, type MembershipChange } from "./membership";
import { setupDone, setupFacts, type SetupFacts } from "./model";
import { OperationRow } from "./OperationRow";
import { restart, useRestartState } from "./restartStore";
import { useLoad, type Loaded } from "./useLoad";
import { closeSetup } from "./watch";

export interface FinishSetupProps {
  server: ServerEntry;
  enabled: boolean;
  health?: ProbeResult;
  registry: Registry | null;
  sets: Loaded<AccessSetsData>;
  /** Changes when a secret was set or removed elsewhere on the page, so the vault is asked again. */
  refreshKey?: number;
  onRegistryChange: (registry: Registry) => void;
  onMembershipChanged: (change: MembershipChange) => void | Promise<void>;
}

function StartsLine({
  server,
  health,
  facts,
  existingNames,
  onRegistryChange,
}: {
  server: ServerEntry;
  health?: ProbeResult;
  facts: SetupFacts;
  existingNames: string[];
  onRegistryChange: (registry: Registry) => void;
}) {
  const state = useRestartState(server.id);
  if (facts.starts === "off") return null;
  if (facts.starts === "checking")
    return (
      <OperationRow
        icon={<Power />}
        title="It starts"
        sub="Checking…"
        badge={<Badge variant="off">Checking</Badge>}
      />
    );
  if (facts.starts === "ok")
    return (
      <OperationRow
        icon={<CircleCheck />}
        tone="success"
        title="It starts"
        sub={
          health && health.toolCount > 0
            ? `Started · ${plural(health.toolCount, "tool")}`
            : "Started"
        }
        badge={<Badge variant="success">Ready</Badge>}
      />
    );
  if (facts.starts === "sign-in")
    return (
      <OperationRow
        icon={<CircleAlert />}
        tone="warning"
        title="It needs a sign-in"
        sub="Use the sign-in or key button in the Connection card. Everything else stays available."
        badge={<Badge variant="warning">Needs sign-in</Badge>}
      />
    );
  const error = health?.error ?? "";
  return (
    <OperationRow
      icon={<CircleAlert />}
      tone="danger"
      title="It does not start"
      sub={error ? errorHeadline(error) : "The gateway could not start it"}
      badge={<Badge variant="destructive">Failed</Badge>}
      open
    >
      {error && (
        <pre className="max-h-28 overflow-y-auto rounded-md bg-muted px-2 py-1.5 font-mono text-meta break-words whitespace-pre-wrap text-muted-foreground">
          {shortenUrls(error)}
        </pre>
      )}
      <div className="flex flex-wrap gap-2">
        <LazyServerDialog
          editId={server.id}
          initial={server}
          onSaved={onRegistryChange}
          existingNames={existingNames}
          trigger={
            <Button variant="outline" size="sm">
              <Pencil /> Edit command…
            </Button>
          }
        />
        <Button
          variant="outline"
          size="sm"
          disabled={state.phase.kind !== "idle"}
          onClick={() => void restart(server.id, server.name)}
        >
          <RotateCw /> Restart
        </Button>
      </div>
    </OperationRow>
  );
}

/** The few things a new server still needs, drawn from facts and closing itself when they are all
 * true. A person can also say not now; the same facts stay in Operations. */
export function FinishSetup({
  server,
  enabled,
  health,
  registry,
  sets,
  refreshKey = 0,
  onRegistryChange,
  onMembershipChanged,
}: FinishSetupProps) {
  const secretKeys = server.env.filter((e) => e.secret).map((e) => e.key);
  const status = useLoad(
    async () =>
      secretKeys.length === 0
        ? []
        : (await secretStatus(server.id, secretKeys))
            .filter(([, set]) => !set)
            .map(([key]) => key),
    `${server.id}:${secretKeys.join()}:${refreshKey}`,
  );
  const facts = setupFacts(server, enabled, health, status.data, registry, sets.data);
  const done = setupDone(facts);

  useEffect(() => {
    if (!done) return;
    closeSetup(server.id);
    toast.success(`${server.name} is set up`);
  }, [done, server.id, server.name]);

  const existingNames = registry?.servers.map((s) => s.name) ?? [];
  const open = [
    facts.starts === "failed" || facts.starts === "sign-in",
    (facts.missingSecrets?.length ?? 0) > 0,
    facts.narrow.length > 0,
  ].filter(Boolean).length;

  const missing = facts.missingSecrets ?? [];
  return (
    <section aria-label="Finish setup">
      <SectionHeader
        action={
          <Button variant="link" size="xs" onClick={() => closeSetup(server.id)}>
            Not now
          </Button>
        }
      >
        Finish setup{" "}
        <Badge variant={open > 0 ? "warning" : "success"}>
          {open === 0 ? "Done" : open === 1 ? "1 thing left" : `${open} things left`}
        </Badge>
      </SectionHeader>
      <div className="toolport-list">
        <StartsLine
          server={server}
          health={health}
          facts={facts}
          existingNames={existingNames}
          onRegistryChange={onRegistryChange}
        />
        {missing.length > 0 && (
          <OperationRow
            icon={<KeyRound />}
            tone="warning"
            title={missing.length === 1 ? "Give it its secret" : "Give it its secrets"}
            sub={`${missing.join(", ")} ${missing.length === 1 ? "is" : "are"} declared but empty. Tools that need ${missing.length === 1 ? "it" : "them"} will fail.`}
            actions={
              <LazySecretsDialog
                server={server}
                onSaved={onRegistryChange}
                onChanged={status.reload}
                trigger={
                  <Button size="sm">
                    {missing.length === 1 ? `Set ${missing[0]}…` : "Set secrets…"}
                  </Button>
                }
              />
            }
          />
        )}
        {facts.narrow.length > 0 && (
          <OperationRow
            icon={<Network />}
            tone="warning"
            title="Who can reach it"
            sub={`${facts.narrow.flatMap((n) => n.clients).join(", ")} ${facts.narrow.flatMap((n) => n.clients).length === 1 ? "uses a narrower access set and cannot see" : "use narrower access sets and cannot see"} it yet.`}
            actions={
              <Button
                variant="outline"
                size="sm"
                onClick={async () => {
                  for (const set of facts.narrow) {
                    await changeMembership(server, set, true, onMembershipChanged);
                  }
                }}
              >
                Add to {plural(facts.narrow.length, "set")}
              </Button>
            }
            open
          >
            <div
              className="flex flex-wrap gap-1.5"
              aria-label="Access sets that leave it out"
            >
              {facts.narrow.map((set) => (
                <Badge key={set.id} variant="outline">
                  {set.name} · {set.clients.join(", ")}
                </Badge>
              ))}
            </div>
          </OperationRow>
        )}
      </div>
    </section>
  );
}
