import { useState } from "react";
import { Trash2 } from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import type { ServerEntry } from "@/lib/types";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { plural } from "../settings/model";
import { previewUninstall, uninstallServer, type UninstallOptions } from "./commands";
import type { ServerInfoData } from "./data";
import { OperationRow } from "./OperationRow";
import { useLoad } from "./useLoad";

function RemovePreview({
  server,
  secretKeys,
  options,
  onChange,
}: {
  server: ServerEntry;
  secretKeys: string[];
  options: UninstallOptions;
  onChange: (next: UninstallOptions) => void;
}) {
  const preview = useLoad(
    () => previewUninstall(server.id, options),
    JSON.stringify(options),
  );
  const clients = preview.data?.clients.length ?? 0;
  return (
    <div className="flex flex-col gap-3 text-meta text-foreground">
      <ul className="flex list-disc flex-col gap-1 pl-5">
        <li>The server entry and its settings, in every access set that lists it</li>
        <li>
          {options.keepSecrets || secretKeys.length === 0
            ? "No vault entries"
            : `Vault entries: ${secretKeys.join(", ")}`}
        </li>
        <li>
          {preview.error
            ? `Client entries could not be checked: ${preview.error}`
            : options.keepClients
              ? "Direct entries stay in the clients' files and show as orphans there"
              : clients === 0
                ? "No client entries: clients reach every server through Toolport's one gateway entry"
                : `${plural(clients, "client entry")}`}
        </li>
      </ul>
      {secretKeys.length > 0 && (
        <label className="flex items-start gap-2">
          <input
            type="checkbox"
            className="mt-1"
            checked={!options.keepSecrets}
            onChange={(e) => onChange({ ...options, keepSecrets: !e.target.checked })}
          />
          <span>
            Also remove its vault entries
            <span className="block text-muted-foreground">
              Toolport’s own Remove leaves them in the keychain.
            </span>
          </span>
        </label>
      )}
      <label className="flex items-start gap-2">
        <input
          type="checkbox"
          className="mt-1"
          checked={options.keepClients}
          onChange={(e) => onChange({ ...options, keepClients: e.target.checked })}
        />
        <span>
          Keep client entries
          <span className="block text-muted-foreground">
            Leaves the direct entries Toolport wrote for this server in the clients'
            files; they show as orphans until you remove them.
          </span>
        </span>
      </label>
    </div>
  );
}

export interface RemoveRowProps {
  server: ServerEntry;
  info: ServerInfoData | null;
  /** Runs after the server is gone, so the app's registry follows and the page leaves the server. */
  onRemoved: () => void | Promise<void>;
}

/** Removing a server together with what it leaves behind: the preview names it first. */
export function RemoveRow({ server, info, onRemoved }: RemoveRowProps) {
  const [open, setOpen] = useState(false);
  const [options, setOptions] = useState<UninstallOptions>({
    keepSecrets: false,
    keepClients: false,
  });
  const secretKeys = (info?.env ?? []).filter((e) => e.secret).map((e) => e.key);

  async function confirm() {
    try {
      const done = await uninstallServer(server.id, options);
      toast.success(
        done.secretsRemoved.length > 0
          ? `Removed ${server.name} and ${plural(done.secretsRemoved.length, "vault entry")}`
          : `Removed ${server.name}`,
      );
      await onRemoved();
    } catch (error) {
      toastError(`Could not remove ${server.name}`, { description: errorText(error) });
      throw error;
    }
  }

  return (
    <>
      <OperationRow
        icon={<Trash2 />}
        title="Remove and clean up"
        sub={
          secretKeys.length > 0
            ? `Removes the server and ${plural(secretKeys.length, "vault entry")}; Remove in the Connection card keeps them in the keychain`
            : "Removes the server; you see what goes first"
        }
        actions={
          <Button variant="outline" onClick={() => setOpen(true)}>
            Remove…
          </Button>
        }
      />
      <ConfirmDialog
        open={open}
        onOpenChange={setOpen}
        destructive
        title={`Remove ${server.name}?`}
        contentClassName="sm:max-w-md"
        description={
          <RemovePreview
            server={server}
            secretKeys={secretKeys}
            options={options}
            onChange={setOptions}
          />
        }
        confirmLabel="Remove server"
        onConfirm={confirm}
      />
    </>
  );
}
