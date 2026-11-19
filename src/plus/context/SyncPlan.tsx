import { useState } from "react";
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
import { Switch } from "@/components/ui/switch";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import type { Loaded } from "../servers/useLoad";
import { syncContext, undoLayers } from "./commands";
import type { SyncData } from "./data";
import { plural, syncRows } from "./model";
import { Group, RowShell } from "./StackRows";

function Rows({ rows }: { rows: ReturnType<typeof syncRows> }) {
  if (rows.length === 0) {
    return (
      <p className="text-meta text-muted-foreground">
        = Everything this sync covers is up to date.
      </p>
    );
  }
  return (
    <ul className="flex flex-col gap-1.5" aria-label="What a sync would write">
      {rows.map((row, index) => (
        <li key={`${row.text}-${index}`} className="flex flex-col">
          <span className="text-meta break-all">
            <span className="mr-2 inline-block w-3 font-mono">{row.sign}</span>
            {row.text}
          </span>
          {row.note && (
            <span className="pl-5 text-meta text-muted-foreground">{row.note}</span>
          )}
        </li>
      ))}
    </ul>
  );
}

/** "What a sync would write now": the plan of `context sync`, with the switch that also deploys
 * the global layers. Sync now shows the same plan again and writes it after one confirmation. */
export default function SyncPlan({
  preview,
  rules,
  onRules,
  home,
  open,
  onOpen,
  onClose,
  onDone,
}: {
  preview: Loaded<SyncData>;
  rules: boolean;
  onRules: (on: boolean) => void;
  home: string | null;
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
  onDone: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const data = preview.data;
  const rows = data ? syncRows(data, home) : [];
  const writes = rows.filter((row) => row.sign !== "\u00b7" && row.sign !== "!").length;

  async function confirm() {
    setBusy(true);
    try {
      const done = await syncContext(false, rules);
      const undo = done.layers.undo;
      toast.success(
        writes > 0 ? `Synced ${plural(writes, "change")}` : "Nothing to sync",
        {
          description:
            "Folders with open Claude sessions keep what they loaded; a new session picks the files up.",
          duration: 10_000,
          action: undo
            ? {
                label: "Undo layers",
                onClick: () => {
                  undoLayers(false).then(onDone, (error: unknown) =>
                    toastError("Could not undo", { description: errorText(error) }),
                  );
                },
              }
            : undefined,
        },
      );
      onClose();
      onDone();
    } catch (error) {
      toastError("Sync did not finish", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <Group
        title="What a sync would write now"
        action={
          <Button size="sm" disabled={!data} onClick={onOpen}>
            Sync now…
          </Button>
        }
      >
        <RowShell
          title={
            data
              ? writes > 0
                ? `${plural(writes, "change")} waiting`
                : "Nothing waiting"
              : "Reading the plan…"
          }
          badge={
            data && (
              <Badge variant={data.autoApply ? "success" : "off"}>
                {data.autoApply ? "bundle auto-apply on" : "bundle auto-apply off"}
              </Badge>
            )
          }
          subtitle={preview.error ?? undefined}
        >
          {data && <Rows rows={rows} />}
          <label className="mt-1 flex items-center gap-2 text-meta">
            <Switch
              size="sm"
              checked={rules}
              onCheckedChange={onRules}
              aria-label="Deploy rules to ~/.claude/rules"
            />
            <span>
              Deploy rules to <code className="font-mono">~/.claude/rules</code>
            </span>
          </label>
        </RowShell>
      </Group>
      {open && data && (
        <Dialog open onOpenChange={(value) => !value && onClose()}>
          <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
            <DialogHeader>
              <DialogTitle>Sync now?</DialogTitle>
              <DialogDescription>
                The same steps the presync runs. Preview first; nothing is written until
                you confirm.
              </DialogDescription>
            </DialogHeader>
            <Rows rows={rows} />
            <p className="text-meta text-muted-foreground">
              Folders with open Claude sessions keep what they loaded; a new session picks
              the files up.
            </p>
            <DialogFooter>
              <Button variant="outline" onClick={onClose}>
                Cancel
              </Button>
              <Button
                data-confirm
                autoFocus
                disabled={busy}
                onClick={() => void confirm()}
              >
                Sync
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      )}
    </>
  );
}
