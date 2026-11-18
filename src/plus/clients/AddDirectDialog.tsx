import { useId, useState } from "react";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import type { DetectedClient } from "@/lib/types";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { listServers } from "../servers/commands";
import { useLoad } from "../servers/useLoad";
import { applyDirect, planDirect } from "./commands";
import type { DirectOutcome } from "./data";
import { changes, usePlan } from "./plan";
import { PlanPreview } from "./PlanPreview";

export interface AddDirectDialogProps {
  client: DetectedClient;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Server ids this client already has a recorded entry for. */
  recorded: Set<string>;
  /** Runs once the entry is written. */
  onAdded: (outcome: DirectOutcome) => void | Promise<void>;
}

/** Gives one client its own entry for one server: the plan comes first, and Add entry writes only
 * that plan. */
export function AddDirectDialog({
  client,
  open,
  onOpenChange,
  recorded,
  onAdded,
}: AddDirectDialogProps) {
  const field = useId();
  const [server, setServer] = useState("");
  const [retry, setRetry] = useState(0);
  const servers = useLoad(listServers, open);
  const chosen = servers.data?.servers.find((row) => row.id === server);
  const target = { server, client: client.id };
  const state = usePlan(
    open && server ? () => planDirect("add", target) : null,
    `${client.id}:${server}`,
    retry,
  );

  async function confirm() {
    if (!state.plan) return;
    try {
      await onAdded(await applyDirect("add", target, state.plan.planHash));
    } catch (error) {
      toastError("Could not add the entry", { description: errorText(error) });
      setRetry((value) => value + 1);
      throw error;
    }
  }

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) setServer("");
        onOpenChange(next);
      }}
      title="Add direct entry"
      contentClassName="sm:max-w-xl"
      description={
        <div className="flex max-h-[70vh] flex-col gap-4 overflow-y-auto text-meta text-foreground">
          <p className="text-muted-foreground">
            Gives {client.name} its own entry for one server, outside the gateway.
          </p>
          <div className="flex flex-col gap-1">
            <label htmlFor={field} className="font-medium">
              Server
            </label>
            <select
              id={field}
              value={server}
              onChange={(event) => setServer(event.target.value)}
              className="h-9 rounded-md border border-input bg-background px-2 text-sm focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none"
            >
              <option value="">Choose a server…</option>
              {(servers.data?.servers ?? []).map((row) => (
                <option key={row.id} value={row.id}>
                  {row.name}
                  {recorded.has(row.id) ? " (already added)" : ""}
                </option>
              ))}
            </select>
            {servers.error && (
              <span role="alert" className="text-destructive">
                {servers.error}
              </span>
            )}
          </div>
          <div className="flex flex-col gap-1">
            <span className="font-medium">Client</span>
            <span>
              {client.name}{" "}
              <span className="font-mono break-all text-muted-foreground">
                {client.configPath}
              </span>
            </span>
          </div>
          {chosen && (
            <div className="flex flex-col gap-1">
              <span className="font-medium">How the client starts it</span>
              {chosen.transport === "stdio" ? (
                <span className="text-muted-foreground">
                  Through Toolport's launcher. The client file holds{" "}
                  <code className="font-mono">toolportctl direct run {chosen.id}</code>.
                  Secrets and plain settings stay in Toolport and never land in the
                  client's file.
                </span>
              ) : (
                <span className="text-muted-foreground">
                  By its address. The client file holds the URL only; the client signs in
                  to the server by itself.
                </span>
              )}
            </div>
          )}
          <PlanPreview state={state} />
          <p className="text-muted-foreground">
            A backup of the file is made first and the change can be undone from the
            result. Toolport writes through the client's own lock and conflict check.
          </p>
        </div>
      }
      confirmLabel="Add entry"
      confirmDisabled={!changes(state.plan)}
      onConfirm={confirm}
    />
  );
}
