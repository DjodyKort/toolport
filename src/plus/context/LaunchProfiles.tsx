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
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { listAccessSets } from "../servers/commands";
import { useLoad } from "../servers/useLoad";
import {
  listLaunchProfiles,
  removeLaunchProfile,
  renderLaunchPolicy,
  saveLaunchProfile,
  type LaunchProfileFields,
} from "./commands";
import type { LaunchProfileRow, LaunchProfileState } from "./data";
import { shortPath } from "./model";
import PlanDialog from "./PlanDialog";
import { Group, RowShell } from "./StackRows";

const NAME = /^[a-z0-9][a-z0-9-]*$/;

const STATE: Record<LaunchProfileState, { label: string; tone: "success" | "warning" }> =
  {
    ok: { label: "generated", tone: "success" },
    "set-missing": { label: "access set gone", tone: "warning" },
    unbound: { label: "not bound", tone: "warning" },
    "file-missing": { label: "file missing", tone: "warning" },
    "stale-file": { label: "file out of date", tone: "warning" },
  };

type Open =
  | { kind: "form"; row: LaunchProfileRow | null }
  | {
      kind: "save";
      name: string;
      fields: LaunchProfileFields;
      before: LaunchProfileRow | null;
    }
  | { kind: "remove"; row: LaunchProfileRow }
  | { kind: "policy"; row: LaunchProfileRow; folder: string };

function ProfileForm({
  row,
  onCancel,
  onPreview,
}: {
  row: LaunchProfileRow | null;
  onCancel: () => void;
  onPreview: (name: string, fields: LaunchProfileFields) => void;
}) {
  const id = useId();
  const sets = useLoad(listAccessSets, "sets");
  const [name, setName] = useState(row?.name ?? "");
  const [set, setSet] = useState(row?.accessSet?.id ?? "");
  const [deny, setDeny] = useState(row?.denyDirect ?? false);
  const nameOk = NAME.test(name) && name.length <= 64;
  return (
    <Dialog open onOpenChange={(open) => !open && onCancel()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{row ? `Change ${row.name}` : "Add launch profile"}</DialogTitle>
          <DialogDescription>
            Claude Code started with this profile carries its own client id, so the
            gateway applies the access set you pick to that start only.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3 text-meta">
          <div className="flex flex-col gap-1">
            <label htmlFor={`${id}-name`} className="font-medium">
              Name
            </label>
            <Input
              id={`${id}-name`}
              value={name}
              disabled={row !== null}
              placeholder="bare"
              aria-invalid={name !== "" && !nameOk}
              onChange={(event) => setName(event.target.value)}
            />
            {name !== "" && !nameOk && (
              <span role="alert" className="text-destructive">
                Lower case letters, digits and hyphens, starting with a letter or digit.
              </span>
            )}
          </div>
          <div className="flex flex-col gap-1">
            <label htmlFor={`${id}-set`} className="font-medium">
              Access set
            </label>
            <select
              id={`${id}-set`}
              value={set}
              onChange={(event) => setSet(event.target.value)}
              className="h-9 rounded-md border border-input bg-background px-2 text-sm focus-visible:ring-1 focus-visible:ring-ring focus-visible:outline-none"
            >
              <option value="">Choose an access set…</option>
              {(sets.data?.accessSets ?? []).map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
            </select>
            {sets.error && (
              <span role="alert" className="text-destructive">
                {sets.error}
              </span>
            )}
          </div>
          <label className="flex items-center justify-between gap-3">
            <span>
              <span className="font-medium">Keep direct entries out</span>
              <span className="block text-muted-foreground">
                Off by default. When on, a folder can get deniedMcpServers for the direct
                entries this set leaves out.
              </span>
            </span>
            <Switch
              size="sm"
              checked={deny}
              onCheckedChange={setDeny}
              aria-label="Keep direct entries out"
            />
          </label>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          <Button
            disabled={!nameOk || !set}
            onClick={() => onPreview(name, { set, denyDirect: deny })}
          >
            Preview the plan
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function undoSave(before: LaunchProfileRow | null, name: string) {
  return before
    ? () =>
        saveLaunchProfile(
          name,
          {
            set: before.accessSet?.id ?? before.accessSetRef,
            denyDirect: before.denyDirect,
          },
          false,
        )
    : () => removeLaunchProfile(name, false);
}

function offerUndo(done: string, undo: () => Promise<unknown>, reload: () => void) {
  toast.success(done, {
    duration: 10_000,
    action: {
      label: "Undo",
      onClick: () => {
        undo().then(reload, (error: unknown) =>
          toastError("Could not undo", { description: errorText(error) }),
        );
      },
    },
  });
}

/** The launch profiles of Rules & launch: each one a named start of Claude Code with its own
 * client id and access set. Every change shows its plan first and offers Undo. */
export default function LaunchProfiles({
  home,
  folder,
}: {
  home: string | null;
  folder: string | null;
}) {
  const list = useLoad(() => listLaunchProfiles(folder ?? undefined), folder);
  const [dialog, setDialog] = useState<Open | null>(null);
  const rows = list.data?.profiles ?? [];
  const close = () => setDialog(null);

  return (
    <Group
      title="Launch profiles"
      count={rows.length}
      hint="Advanced"
      action={
        <Button
          size="sm"
          variant="outline"
          onClick={() => setDialog({ kind: "form", row: null })}
        >
          Add profile…
        </Button>
      }
    >
      {list.error && !list.data && (
        <RowShell title="The launch profiles could not be read" subtitle={list.error} />
      )}
      {list.data && rows.length === 0 && (
        <RowShell
          title="No launch profiles"
          subtitle="A profile starts Claude Code with only the servers of one access set, through its own client id."
        />
      )}
      {rows.map((row) => (
        <RowShell
          key={row.name}
          title={row.name}
          badge={
            <Badge variant={STATE[row.state]?.tone ?? "warning"}>
              {STATE[row.state]?.label ?? row.state}
            </Badge>
          }
          subtitle={
            <>
              Narrows servers through its own client id{" "}
              <code className="font-mono">{row.clientId}</code> and the access set{" "}
              <code className="font-mono">{row.accessSet?.name ?? row.accessSetRef}</code>
              .
            </>
          }
          trailing={
            <>
              <Button
                size="sm"
                variant="ghost"
                aria-label={`Copy the start command of ${row.name}`}
                onClick={() => void navigator.clipboard?.writeText(row.command)}
              >
                Copy command
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => setDialog({ kind: "form", row })}
              >
                Change…
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => setDialog({ kind: "remove", row })}
              >
                Remove…
              </Button>
            </>
          }
        >
          {row.problems.map((problem) => (
            <p key={problem} role="status" className="text-meta text-warning">
              {problem}
            </p>
          ))}
          {row.folderWarnings.map((warning) => (
            <p
              key={`${warning.folder}-${warning.text}`}
              role="note"
              className="text-meta text-warning"
            >
              {warning.text}
            </p>
          ))}
          <div className="flex flex-wrap items-center gap-3 text-meta text-muted-foreground">
            <span>
              Keeps direct entries out: {row.denyDirect ? "on" : "off"}
              {row.policyFolders.length > 0 &&
                `, written in ${row.policyFolders.length} folder(s)`}
            </span>
            {row.denyDirect && folder && (
              <Button
                size="sm"
                variant="outline"
                onClick={() => setDialog({ kind: "policy", row, folder })}
              >
                Write to {shortPath(folder, home)}…
              </Button>
            )}
          </div>
        </RowShell>
      ))}
      {dialog?.kind === "form" && (
        <ProfileForm
          row={dialog.row}
          onCancel={close}
          onPreview={(name, fields) =>
            setDialog({ kind: "save", name, fields, before: dialog.row })
          }
        />
      )}
      {dialog?.kind === "save" && (
        <PlanDialog
          title={
            dialog.before ? `Change ${dialog.name}` : `Add launch profile ${dialog.name}`
          }
          description="Binds the client id in the registry and writes one gateway entry. Nothing is written until you confirm."
          confirmLabel={dialog.before ? "Change profile" : "Add profile"}
          preview={() => saveLaunchProfile(dialog.name, dialog.fields, true)}
          confirm={async () => {
            await saveLaunchProfile(dialog.name, dialog.fields, false);
            offerUndo(
              dialog.before ? `Changed ${dialog.name}` : `Added ${dialog.name}`,
              undoSave(dialog.before, dialog.name),
              list.reload,
            );
          }}
          onClose={close}
          onDone={list.reload}
        />
      )}
      {dialog?.kind === "remove" && (
        <PlanDialog
          title={`Remove ${dialog.row.name}`}
          description="Removes the binding, the generated file and any policy written for it."
          confirmLabel="Remove profile"
          destructive
          preview={() => removeLaunchProfile(dialog.row.name, true)}
          confirm={async () => {
            await removeLaunchProfile(dialog.row.name, false);
            const before = dialog.row;
            offerUndo(
              `Removed ${before.name}`,
              () =>
                saveLaunchProfile(
                  before.name,
                  {
                    set: before.accessSet?.id ?? before.accessSetRef,
                    denyDirect: before.denyDirect,
                  },
                  false,
                ),
              list.reload,
            );
          }}
          onClose={close}
          onDone={list.reload}
        />
      )}
      {dialog?.kind === "policy" && (
        <PlanDialog
          title={`Keep direct entries out of ${shortPath(dialog.folder, home)}`}
          description="Writes deniedMcpServers into the folder's git-ignored settings. Unverified against a real Claude Code launch."
          confirmLabel="Write keys"
          preview={() => renderLaunchPolicy(dialog.row.name, dialog.folder, true)}
          confirm={() => renderLaunchPolicy(dialog.row.name, dialog.folder, false)}
          onClose={close}
          onDone={list.reload}
        />
      )}
    </Group>
  );
}
