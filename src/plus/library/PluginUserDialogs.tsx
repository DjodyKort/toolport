import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toastError } from "@/lib/toast";
import PlanDialog, { PlanView } from "../context/PlanDialog";
import type { Plan } from "../context/data";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { userScope } from "./pluginsCommands";
import type { PluginRow } from "./pluginsData";

/** Turning a plugin off for every folder asks for its name to be typed first. */
function DisableDialog({
  plugin,
  onClose,
  onDone,
}: {
  plugin: PluginRow;
  onClose: () => void;
  onDone: () => void;
}) {
  const plan = useLoad(() => userScope("disable", plugin.id, true), plugin.id);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);

  async function go() {
    setBusy(true);
    try {
      await userScope("disable", plugin.id, false);
      onDone();
      onClose();
    } catch (error) {
      toastError("Nothing was changed", { description: errorText(error) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Disable {plugin.name} everywhere</DialogTitle>
          <DialogDescription>
            Claude Code stops loading it in every folder that does not turn it on itself.
          </DialogDescription>
        </DialogHeader>
        {plan.error && <p role="alert">{plan.error}</p>}
        {!plan.data && !plan.error && <p className="text-meta">Preparing the plan…</p>}
        {plan.data && <PlanView plan={plan.data.plan as Plan} />}
        <div className="flex flex-col gap-1">
          <Label htmlFor="disable-confirm">Type {plugin.name} to confirm</Label>
          <Input
            id="disable-confirm"
            value={typed}
            autoComplete="off"
            onChange={(event) => setTyped(event.target.value)}
          />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant="destructive"
            disabled={!plan.data || busy || typed !== plugin.name}
            onClick={() => void go()}
          >
            Disable everywhere
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export type UserAction = "disable" | "enable" | "update";

export function UserScopeDialog({
  action,
  plugin,
  onClose,
  onDone,
}: {
  action: UserAction;
  plugin: PluginRow;
  onClose: () => void;
  onDone: () => void;
}) {
  if (action === "disable") {
    return <DisableDialog plugin={plugin} onClose={onClose} onDone={onDone} />;
  }
  const to = plugin.update.available;
  return (
    <PlanDialog
      title={
        action === "enable"
          ? `Enable ${plugin.name} everywhere`
          : `Update ${plugin.name}${to ? ` to ${to}` : ""}`
      }
      confirmLabel={action === "enable" ? "Enable everywhere" : "Update"}
      preview={() => userScope(action, plugin.id, true)}
      confirm={() => userScope(action, plugin.id, false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

/** One plan for every plugin that has an update waiting: the previews of each, then each runs. */
export function UpdateAllDialog({
  plugins,
  onClose,
  onDone,
}: {
  plugins: PluginRow[];
  onClose: () => void;
  onDone: () => void;
}) {
  return (
    <PlanDialog
      title={`Update ${plugins.length} ${plugins.length === 1 ? "plugin" : "plugins"}`}
      confirmLabel="Update all"
      preview={async () => {
        const plans = await Promise.all(
          plugins.map((p) => userScope("update", p.id, true)),
        );
        const warnings = [...new Set(plans.flatMap((p) => p.plan.warnings))];
        return {
          plan: {
            summary: `Update ${plugins.map((p) => p.name).join(", ")}`,
            steps: plans.flatMap((p) => p.plan.steps),
            effects: {},
            warnings,
            undo: "",
          },
        };
      }}
      confirm={async () => {
        for (const plugin of plugins) await userScope("update", plugin.id, false);
      }}
      onClose={onClose}
      onDone={onDone}
    />
  );
}
