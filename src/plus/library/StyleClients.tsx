import { useId, useState } from "react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { SkeletonRows } from "@/components/ui/skeleton";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { UNDO_MS } from "./bodyEdit";
import { applyStyle, removeStyle } from "./commands";
import type { RemoveData, StyleApplyData, StyleStatusData } from "./data";
import { FileLines, Mono, WriteDialog } from "./dialogs";
import { plural } from "./model";

/** The "no style" choice of the picker: it takes the applied style off. */
export const NO_STYLE = "";

const SELECT =
  "h-8 rounded-md border bg-background px-2 text-sm text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none";

type Row = StyleStatusData["applyRemove"][number];

/** Puts each client back on the style it had: one apply per style, one remove for the clients
 * that had none. */
async function restore(before: Row[]) {
  const byStyle = new Map<string, string[]>();
  for (const row of before) {
    byStyle.set(row.active ?? NO_STYLE, [
      ...(byStyle.get(row.active ?? NO_STYLE) ?? []),
      row.client,
    ]);
  }
  for (const [style, clients] of byStyle) {
    if (style === NO_STYLE) await removeStyle(false, clients);
    else await applyStyle(style, clients, false);
  }
}

function ChooseStep({
  styles,
  rows,
  initial,
  onClose,
  onNext,
}: {
  styles: string[];
  rows: Row[];
  initial: string | null;
  onClose: () => void;
  onNext: (style: string, clients: string[]) => void;
}) {
  const selectId = useId();
  const current = initial ? rows.find((row) => row.client === initial) : undefined;
  const [style, setStyle] = useState(current?.active ?? styles[0] ?? NO_STYLE);
  const [picked, setPicked] = useState<string[]>(initial ? [initial] : []);
  const toggle = (client: string, on: boolean) =>
    setPicked((now) => (on ? [...now, client] : now.filter((c) => c !== client)));
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Apply a style</DialogTitle>
          <DialogDescription>
            Pick the style and the clients that get it. The preview comes next; nothing is
            written before you confirm it.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-1.5">
          <label htmlFor={selectId} className="text-meta font-medium">
            Style
          </label>
          <select
            id={selectId}
            className={SELECT}
            value={style}
            onChange={(event) => setStyle(event.target.value)}
          >
            {styles.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
            <option value={NO_STYLE}>No style (take the applied one off)</option>
          </select>
        </div>
        <fieldset className="flex flex-col gap-1.5">
          <legend className="text-meta font-medium">Clients</legend>
          <ul className="flex max-h-56 flex-col gap-1 overflow-y-auto">
            {rows.map((row) => (
              <li key={row.client}>
                <label className="flex items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    checked={picked.includes(row.client)}
                    onChange={(event) => toggle(row.client, event.target.checked)}
                  />
                  <span>{row.name}</span>
                  <span className="text-meta text-muted-foreground">
                    {row.active ? `now ${row.active}` : "no style"}
                  </span>
                </label>
              </li>
            ))}
          </ul>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="self-start"
            onClick={() =>
              setPicked(picked.length === rows.length ? [] : rows.map((r) => r.client))
            }
          >
            {picked.length === rows.length ? "Clear all" : "Select all"}
          </Button>
        </fieldset>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={picked.length === 0} onClick={() => onNext(style, picked)}>
            Preview
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function ApplyStyleDialog({
  status,
  styles,
  initial,
  onClose,
  onDone,
}: {
  status: StyleStatusData;
  styles: string[];
  initial: string | null;
  onClose: () => void;
  onDone: () => void;
}) {
  const [choice, setChoice] = useState<{ style: string; clients: string[] } | null>(null);
  if (!choice) {
    return (
      <ChooseStep
        styles={styles}
        rows={status.applyRemove}
        initial={initial}
        onClose={onClose}
        onNext={(style, clients) => setChoice({ style, clients })}
      />
    );
  }
  const { style, clients } = choice;
  const before = status.applyRemove.filter((row) => clients.includes(row.client));
  const removing = style === NO_STYLE;

  async function confirm() {
    const done = removing
      ? await removeStyle(false, clients)
      : await applyStyle(style, clients, false);
    const text = removing
      ? `The applied style is off for ${plural(clients.length, "client")}`
      : `${style} applies to ${plural(clients.length, "client")}`;
    toast.message(text, {
      duration: UNDO_MS,
      action: {
        label: "Undo",
        onClick: () =>
          void restore(before)
            .catch((error: unknown) =>
              toastError("Could not put the styles back", {
                description: errorText(error),
              }),
            )
            .finally(onDone),
      },
    });
    return done;
  }

  return (
    <WriteDialog<StyleApplyData | RemoveData>
      title={removing ? "Take the applied style off" : `Apply ${style}`}
      description="Each client gets the style as one always-on rule file in your user folder. A style it had is replaced."
      confirmLabel={removing ? "Take it off" : `Apply ${style}`}
      destructive={removing}
      preview={() =>
        removing ? removeStyle(true, clients) : applyStyle(style, clients, true)
      }
      render={(data) =>
        "removed" in data ? (
          data.removed.length === 0 ? (
            <p className="text-meta">None of these clients has an applied style.</p>
          ) : (
            <FileLines
              title={`${plural(data.removed.length, "rule file")} removed`}
              files={data.removed.map((r) => r.path)}
            />
          )
        ) : (
          <div className="flex flex-col gap-2">
            <FileLines
              title={`${plural(data.applied.length, "rule file")} written`}
              files={data.applied.map((a) => a.path)}
            />
            {data.replaced.length > 0 && (
              <ul className="flex flex-col gap-0.5 text-meta" aria-label="Replaced">
                {data.replaced.map((r) => (
                  <li key={r.client}>
                    Replaces <Mono>{r.style}</Mono> on {r.client}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )
      }
      confirm={confirm}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

/** The output style of every client that takes one as a rule. A client with a native switch
 * (Claude Code, Roo Code) gets its styles from Sync styles above; the others are listed here with
 * the style each one has on, and a change goes through a preview and has Undo. The scope is the
 * user folder, which is where the Library writes everything. */
export default function StyleClients({
  status,
  error,
  styles,
  onChanged,
}: {
  status: StyleStatusData | null;
  error: string | null;
  styles: string[];
  onChanged: () => void;
}) {
  const [dialog, setDialog] = useState<{ client: string | null } | null>(null);
  if (!status) {
    return (
      <section aria-label="Style per client" className="flex flex-col gap-2">
        <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
          Style per client
        </h2>
        {error ? (
          <div role="alert" className="flex flex-wrap items-center gap-3 text-meta">
            <span className="text-destructive">
              Could not read the style of each client: {error}
            </span>
            <Button variant="outline" size="xs" onClick={onChanged}>
              Try again
            </Button>
          </div>
        ) : (
          <SkeletonRows label="Reading the style of each client" />
        )}
      </section>
    );
  }
  const rows = status.applyRemove;
  const on = rows.filter((row) => row.active).length;
  return (
    <section aria-label="Style per client" className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
          Style per client <span className="ml-1 font-normal normal-case">{on} on</span>
        </h2>
        <Button
          variant="outline"
          size="sm"
          disabled={styles.length === 0}
          onClick={() => setDialog({ client: null })}
        >
          Apply a style…
        </Button>
      </div>
      <p className="text-meta text-muted-foreground">
        User level, in your home folder.
        {styles.length === 0 && " Add an output style first."}
      </p>
      {status.native.length > 0 && (
        <ul className="divide-y overflow-hidden rounded-xl border bg-card">
          {status.native.map((row) => (
            <li
              key={row.client}
              className="flex flex-wrap items-center gap-3 px-4 py-2.5 text-sm"
            >
              <span className="min-w-32 flex-1 font-medium">{row.name}</span>
              <span className="text-meta text-muted-foreground">
                {row.styles.length > 0
                  ? `${row.styles.join(", ")} synced; you pick it inside the client`
                  : "No style synced"}
              </span>
            </li>
          ))}
        </ul>
      )}
      <ul className="divide-y overflow-hidden rounded-xl border bg-card">
        {rows.map((row) => (
          <li
            key={row.client}
            className="flex flex-wrap items-center gap-3 px-4 py-2.5 text-sm"
          >
            <span className="min-w-32 flex-1 font-medium">{row.name}</span>
            <Badge variant={row.active ? "success" : "off"}>
              {row.active ?? "No style"}
            </Badge>
            <Button
              variant="outline"
              size="xs"
              disabled={styles.length === 0 && !row.active}
              aria-label={`Change the style of ${row.name}`}
              onClick={() => setDialog({ client: row.client })}
            >
              Change…
            </Button>
          </li>
        ))}
      </ul>
      {dialog && (
        <ApplyStyleDialog
          status={status}
          styles={styles}
          initial={dialog.client}
          onClose={() => setDialog(null)}
          onDone={onChanged}
        />
      )}
    </section>
  );
}
