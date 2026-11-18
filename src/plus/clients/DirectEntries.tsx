import { useId, useMemo, useState } from "react";
import { Copy, Plus, Unplug } from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { DetectedClient } from "@/lib/types";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { OperationRow } from "../servers/OperationRow";
import { useLoad } from "../servers/useLoad";
import { AddDirectDialog } from "./AddDirectDialog";
import { applyDirect, listDirect, planDirect } from "./commands";
import type { DirectOutcome, DirectRow } from "./data";
import {
  ACTION_COPY,
  ACTION_LABEL,
  actionsOf,
  describe,
  DONE,
  foreignEntries,
  needsForce,
  STATES,
  undoOf,
  type RowAction,
} from "./model";
import { changes, usePlan } from "./plan";
import { PlanPreview } from "./PlanPreview";

const UNDO_MS = 10_000;

export interface DirectEntriesProps {
  client: DetectedClient;
  /** Runs after a write, so the app's list of clients follows. */
  onChanged: () => void;
}

interface Pending {
  row: DirectRow;
  action: RowAction;
}

function ActionDialog({
  pending,
  onClose,
  onDone,
}: {
  pending: Pending | null;
  onClose: () => void;
  onDone: (
    outcome: DirectOutcome,
    row: DirectRow,
    action: RowAction,
  ) => void | Promise<void>;
}) {
  const [retry, setRetry] = useState(0);
  const row = pending?.row;
  const copy = pending ? ACTION_COPY[pending.action] : null;
  const target = row && {
    server: row.server,
    client: row.client,
    force: needsForce(row),
  };
  const state = usePlan(
    pending && copy && target ? () => planDirect(copy.verb, target) : null,
    pending ? `${pending.action}:${row?.client}:${row?.entry}` : "",
    retry,
  );
  if (!pending || !copy || !target || !row) return null;

  async function confirm() {
    if (!state.plan || !copy || !target || !row) return;
    try {
      const outcome = await applyDirect(copy.verb, target, state.plan.planHash);
      await onDone(outcome, row, pending.action);
    } catch (error) {
      toastError(`Could not ${copy.confirm.toLowerCase()}`, {
        description: errorText(error),
      });
      setRetry((value) => value + 1);
      throw error;
    }
  }

  return (
    <ConfirmDialog
      open
      onOpenChange={(open) => !open && onClose()}
      title={copy.title(row)}
      contentClassName="sm:max-w-xl"
      destructive={copy.destructive}
      description={
        <div className="flex max-h-[70vh] flex-col gap-3 overflow-y-auto text-meta text-foreground">
          <p className="text-muted-foreground">{STATES[row.state].meaning}</p>
          <PlanPreview state={state} />
        </div>
      }
      confirmLabel={copy.confirm}
      confirmDisabled={!changes(state.plan)}
      onConfirm={confirm}
    />
  );
}

/** The Direct entries panel of a client's detail: the entries Toolport wrote into the client's own
 * file, in one of seven states, with what each state offers. Every write shows its plan first. */
export default function DirectEntries({ client, onChanged }: DirectEntriesProps) {
  const heading = useId();
  const rows = useLoad(() => listDirect(client.id), client);
  const [adding, setAdding] = useState(false);
  const [pending, setPending] = useState<Pending | null>(null);

  const all = useMemo(() => rows.data?.entries ?? [], [rows.data]);
  const written = all.filter((row) => row.state !== "unrecorded");
  const untracked = all.filter((row) => row.state === "unrecorded");
  const names = useMemo(() => new Set(all.map((row) => row.entry)), [all]);
  const others = rows.data ? foreignEntries(client, names) : [];
  const inFile = new Set(client.servers.map((server) => server.name));
  const recorded = useMemo(() => new Set(written.map((row) => row.server)), [written]);

  if (!client.configExists && !client.appPresent) return null;

  async function refresh() {
    rows.reload();
    onChanged();
  }

  async function undo(outcome: DirectOutcome) {
    const verb = undoOf(outcome);
    if (!verb) return;
    const target = { server: outcome.server, client: outcome.client };
    try {
      const plan = await planDirect(verb, target);
      await applyDirect(verb, target, plan.planHash);
      toast.success(`Undid the change to ${outcome.entry}`);
    } catch (error) {
      toastError(`Could not undo the change to ${outcome.entry}`, {
        description: errorText(error),
      });
    }
    await refresh();
  }

  async function done(outcome: DirectOutcome, message: string) {
    toast.success(message, {
      duration: UNDO_MS,
      action: undoOf(outcome)
        ? { label: "Undo", onClick: () => void undo(outcome) }
        : undefined,
    });
    await refresh();
  }

  function rowActions(row: DirectRow) {
    return (
      <>
        {actionsOf(row, inFile.has(row.entry)).map((action) => (
          <Button
            key={action}
            size="sm"
            variant="outline"
            aria-label={`${ACTION_LABEL[action]} ${row.entry}`}
            onClick={() => setPending({ row, action })}
          >
            {ACTION_LABEL[action]}
          </Button>
        ))}
        <Button
          size="sm"
          variant="ghost"
          aria-label={`Copy the path of ${row.entry}'s file`}
          onClick={() => void navigator.clipboard?.writeText(row.path)}
        >
          <Copy aria-hidden="true" />
          Copy path
        </Button>
      </>
    );
  }

  const badge = (row: DirectRow) => {
    const copy = STATES[row.state];
    return <Badge variant={copy.tone}>{copy.label}</Badge>;
  };

  return (
    <section aria-labelledby={heading} className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <h3
            id={heading}
            className="text-xs font-semibold tracking-[0.09em] text-muted-foreground uppercase"
          >
            Direct entries
          </h3>
          <p className="text-meta text-muted-foreground">
            {rows.data
              ? `${written.length === 1 ? "1 entry" : `${written.length} entries`} Toolport wrote and keeps track of · bypass the gateway`
              : "Entries Toolport writes into this client's own file · bypass the gateway"}
          </p>
        </div>
        <Button size="sm" variant="outline" onClick={() => setAdding(true)}>
          <Plus aria-hidden="true" />
          Add direct entry…
        </Button>
      </div>

      {rows.error && (
        <p className="text-meta text-muted-foreground">
          The direct entries could not be read: {rows.error}{" "}
          <button type="button" className="underline" onClick={rows.reload}>
            Try again
          </button>
        </p>
      )}

      {written.length > 0 && (
        <div className="toolport-list" aria-label="Written by Toolport">
          {written.map((row) => (
            <OperationRow
              key={`${row.client}:${row.entry}`}
              icon={<Unplug />}
              title={row.entry}
              tone={row.state === "ok" ? undefined : "warning"}
              sub={
                <>
                  {describe(row)}
                  {row.state !== "ok" && (
                    <span className="block">{STATES[row.state].meaning}</span>
                  )}
                </>
              }
              badge={badge(row)}
              actions={rowActions(row)}
            />
          ))}
        </div>
      )}

      {(untracked.length > 0 || others.length > 0) && (
        <>
          <p className="text-meta font-medium text-muted-foreground">
            Entries Toolport did not write
          </p>
          <div className="toolport-list" aria-label="Entries Toolport did not write">
            {untracked.map((row) => (
              <OperationRow
                key={`${row.client}:${row.entry}`}
                icon={<Unplug />}
                title={row.entry}
                sub={`${describe(row)} · ${STATES[row.state].meaning}`}
                badge={badge(row)}
                actions={rowActions(row)}
              />
            ))}
            {others.map((server) => (
              <OperationRow
                key={server.name}
                icon={<Unplug />}
                title={server.name}
                sub={`Entry Toolport did not write · ${server.transport}. Toolport leaves it alone; Import into Toolport below can move it into the gateway.`}
                badge={<Badge variant="off">Not managed</Badge>}
              />
            ))}
          </div>
        </>
      )}

      {rows.data &&
        written.length === 0 &&
        untracked.length === 0 &&
        others.length === 0 && (
          <p className="toolport-list px-4 py-3 text-meta text-muted-foreground">
            No direct entries. {client.name} reaches your servers through Toolport's one
            gateway entry; add a direct entry only when it has to start a server by
            itself.
          </p>
        )}

      <AddDirectDialog
        client={client}
        open={adding}
        onOpenChange={setAdding}
        recorded={recorded}
        onAdded={(outcome) =>
          done(outcome, `Added the direct entry ${outcome.entry} to ${client.name}`)
        }
      />
      <ActionDialog
        pending={pending}
        onClose={() => setPending(null)}
        onDone={(outcome, row, action) => done(outcome, DONE[action](row))}
      />
    </section>
  );
}
