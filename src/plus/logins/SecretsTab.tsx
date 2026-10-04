import { useMemo, useState } from "react";
import { Eye, Lock, RefreshCw } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { CtlError, ctlData } from "../bridge/ctl";
import type { SecretGetData } from "../types/secret";
import { Gate } from "./atoms";
import type { TabProps } from "./LoginsTab";
import {
  authRowsOf,
  countableAuth,
  presenceKey,
  secretRows,
  summarize,
  type Presence,
  type Settled,
} from "./model";
import { ProbePanel } from "./ProbePanel";
import { RemoveDialog } from "./RemoveDialog";
import { RevealFlow } from "./RevealFlow";
import { SetSecretDialog } from "./SetSecretDialog";
import { Strip } from "./Strip";
import { useBatch } from "./useBatch";
import { useProbe } from "./useProbe";
import { POLL_MS, usePolicy, useRoster } from "./useRoster";

async function readPresence(pair: string): Promise<Presence> {
  const [server, key] = pair.split("\u0000");
  try {
    await ctlData<SecretGetData>(["secret", "get", server, key]);
    return "set";
  } catch (error) {
    if (error instanceof CtlError && error.code === "not_found") return "unset";
    throw error;
  }
}

type Dialog = {
  kind: "set" | "reveal" | "remove";
  id: string;
  name: string;
  key: string;
} | null;

function PresenceBadge({ answer }: { answer: Settled<Presence> | undefined }) {
  if (!answer) return <Badge variant="secondary">Checking…</Badge>;
  if (!answer.ok) return <Badge variant="warning">Unknown</Badge>;
  return answer.value === "set" ? (
    <Badge variant="success">set</Badge>
  ) : (
    <Badge variant="secondary">unset</Badge>
  );
}

/** Secrets: the key names each server declares, with a set or unset badge and never a value.
 * Replace and Reveal are separate confirmed actions; Remove is typed when the policy says so. */
export function SecretsTab({ onOpenCommands, pollMs = POLL_MS }: TabProps) {
  const data = useRoster(pollMs);
  const { reload } = data;
  const tierOf = usePolicy();
  const probe = useProbe(reload);
  const [dialog, setDialog] = useState<Dialog>(null);

  const groups = useMemo(
    () =>
      data.servers.data ? secretRows(data.servers.data.servers, data.infos.results) : [],
    [data.servers.data, data.infos.results],
  );
  const pairs = useMemo(
    () => groups.flatMap((group) => group.keys.map((key) => presenceKey(group.id, key))),
    [groups],
  );
  const presence = useBatch<Presence>(pairs, readPresence);
  const refreshKey = (id: string, key: string) =>
    void presence.refresh([presenceKey(id, key)]);

  const summary = summarize(countableAuth(authRowsOf(data.status.data?.auth.servers)));
  const answer = (id: string, key: string) => presence.results[presenceKey(id, key)];

  return (
    <div className="flex flex-col gap-4">
      <Gate
        queries={[data.status, data.servers]}
        pending={data.roster === null}
        title="Couldn't read the secrets"
        context="status, server ls"
        onOpenCommands={onOpenCommands}
      >
        {() => (
          <>
            <Strip summary={summary} />
            <div className="flex justify-end">
              <Button variant="ghost" size="sm" onClick={reload}>
                <RefreshCw /> Refresh
              </Button>
            </div>
            <ProbePanel probe={probe} />
            {groups.length === 0 ? (
              <EmptyState
                icon={<Lock />}
                title="No secrets declared"
                description="None of your servers declares a secret key. A server with an API key or token shows its key names here."
                action={
                  onOpenCommands && (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => onOpenCommands("secret")}
                    >
                      Open All commands
                    </Button>
                  )
                }
              />
            ) : (
              <ul aria-label="Secrets" className="divide-y rounded-xl border">
                {groups.map((group) => (
                  <li key={group.id} className="flex flex-col gap-1.5 px-3 py-2.5">
                    <b className="font-semibold">{group.name}</b>
                    {group.keys.map((key) => {
                      const current = answer(group.id, key);
                      const isSet = current?.ok && current.value === "set";
                      const label = `${key} of ${group.name}`;
                      return (
                        <div
                          key={key}
                          role="group"
                          aria-label={label}
                          className="flex flex-wrap items-center gap-2.5"
                        >
                          <code className="min-w-48 font-mono text-sm">{key}</code>
                          <PresenceBadge answer={current} />
                          <span className="flex-1" />
                          <Button
                            size="sm"
                            variant="outline"
                            aria-label={`${isSet ? "Replace" : "Set"} ${label}`}
                            onClick={() =>
                              setDialog({
                                kind: "set",
                                id: group.id,
                                name: group.name,
                                key,
                              })
                            }
                          >
                            {isSet ? "Replace…" : "Set…"}
                          </Button>
                          <Button
                            size="sm"
                            variant="outline"
                            disabled={!isSet}
                            aria-label={`Reveal ${label}`}
                            onClick={() =>
                              setDialog({
                                kind: "reveal",
                                id: group.id,
                                name: group.name,
                                key,
                              })
                            }
                          >
                            <Eye /> Reveal…
                          </Button>
                          <Button
                            size="sm"
                            variant="destructive"
                            disabled={!isSet}
                            aria-label={`Remove ${label}`}
                            onClick={() =>
                              setDialog({
                                kind: "remove",
                                id: group.id,
                                name: group.name,
                                key,
                              })
                            }
                          >
                            Remove
                          </Button>
                        </div>
                      );
                    })}
                  </li>
                ))}
              </ul>
            )}
            <p className="text-sm text-muted-foreground">
              Values never appear in the interface. Replace is a write-only field; Reveal
              asks for confirmation and hides again after 10 seconds.
            </p>
          </>
        )}
      </Gate>
      {dialog?.kind === "set" && (
        <SetSecretDialog
          server={{ id: dialog.id, name: dialog.name }}
          keys={groups.find((group) => group.id === dialog.id)?.keys ?? [dialog.key]}
          initialKey={dialog.key}
          existing={(key) => {
            const known = answer(dialog.id, key);
            return known?.ok ? known.value === "set" : null;
          }}
          onClose={() => setDialog(null)}
          onStored={(key) => refreshKey(dialog.id, key)}
          onProbe={() => {
            const id = dialog.id;
            setDialog(null);
            void probe.run({ server: id, force: true });
          }}
        />
      )}
      {dialog?.kind === "reveal" && (
        <RevealFlow
          server={{ id: dialog.id, name: dialog.name }}
          secretKey={dialog.key}
          tier={tierOf("secret get", "read")}
          onClose={() => setDialog(null)}
        />
      )}
      {dialog?.kind === "remove" && (
        <RemoveDialog
          server={{ id: dialog.id, name: dialog.name }}
          secretKey={dialog.key}
          tier={tierOf("secret rm", "destructive")}
          onClose={() => setDialog(null)}
          onRemoved={() => refreshKey(dialog.id, dialog.key)}
        />
      )}
    </div>
  );
}
