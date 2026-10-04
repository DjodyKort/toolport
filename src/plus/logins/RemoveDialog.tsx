import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { ctlData } from "../bridge/ctl";
import { PlanPreview, TypedConfirmDialog, errorText } from "../ui";
import { removeLine, removePlan, type ServerRef } from "./plans";
import type { Tier } from "./useRoster";

/** `secret rm`: the plan, then a typed confirmation when the policy tier is destructive (the
 * phrase is the key), then the removal. The command has no dry run, so the plan is written
 * from what it does. */
export function RemoveDialog({
  server,
  secretKey,
  tier,
  onClose,
  onRemoved,
}: {
  server: ServerRef;
  secretKey: string;
  tier: Tier;
  onClose: () => void;
  onRemoved: () => void;
}) {
  async function confirm() {
    try {
      await ctlData(["secret", "rm", server.id, secretKey]);
    } catch (error) {
      toastError(`Couldn't remove ${secretKey}: ${errorText(error).message}`);
      throw error;
    }
    toast.success(`Removed ${secretKey} for ${server.name}`);
    onRemoved();
  }
  const body = (
    <div className="flex flex-col gap-3">
      <PlanPreview data={removePlan(server, secretKey)} />
      <p className="text-xs text-muted-foreground">
        This command has no preview of its own. It runs{" "}
        <code className="font-mono">{removeLine(server, secretKey)}</code> as soon as you
        confirm.
      </p>
    </div>
  );
  const onOpenChange = (open: boolean) => !open && onClose();
  return tier === "destructive" ? (
    <TypedConfirmDialog
      open
      onOpenChange={onOpenChange}
      title={`Remove ${secretKey}?`}
      phrase={secretKey}
      confirmLabel="Remove secret"
      onConfirm={confirm}
    >
      {body}
    </TypedConfirmDialog>
  ) : (
    <ConfirmDialog
      open
      onOpenChange={onOpenChange}
      title={`Remove ${secretKey}?`}
      destructive
      contentClassName="sm:max-w-lg"
      description={body}
      confirmLabel="Remove secret"
      onConfirm={confirm}
    />
  );
}
