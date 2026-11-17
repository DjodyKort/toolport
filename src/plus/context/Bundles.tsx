import { useState } from "react";
import { Boxes, Plus } from "lucide-react";
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
import { StatusCard } from "@/components/ui/status-card";
import { Switch } from "@/components/ui/switch";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { useLoad } from "../servers/useLoad";
import {
  addBundle,
  applyBundle,
  bundleConfig,
  bundleStatus,
  editBundle,
  launchCommand,
  listBundles,
  removeBundle,
  showBundle,
  undoBundle,
  contextUse,
  stopUsingBundle,
  type BundleFields,
} from "./commands";
import type { BundleRow, BundleShowData } from "./data";
import { plural, shortPath } from "./model";
import PlanDialog from "./PlanDialog";
import { Group, RowShell } from "./StackRows";

const csv = (list: string[]) => list.join(", ");

function fieldsOf(bundle: BundleShowData | null): BundleFields {
  return {
    description: bundle?.description ?? "",
    skillsOff: csv(bundle?.skills.off ?? []),
    pluginsOff: csv(bundle?.plugins.off ?? []),
    agentsOff: csv(bundle?.agents.off ?? []),
    layersExclude: csv(bundle?.layers.exclude ?? []),
    servers: bundle?.servers ?? "",
    bind: csv(bundle?.bind ?? []),
  };
}

const LABELS: Array<[keyof BundleFields, string, string]> = [
  ["description", "Description", "One line about the bundle"],
  ["layersExclude", "Instruction files left out", "CLAUDE.md globs, comma separated"],
  ["pluginsOff", "Plugins off", "name@marketplace, comma separated"],
  ["skillsOff", "Skills off", "names or globs, comma separated"],
  ["agentsOff", "Agents off", "comma separated"],
  ["servers", "Access set", "The access set this bundle goes with"],
  ["bind", "Applies to", "Folder patterns offered the bundle, comma separated"],
];

function BundleForm({
  existing,
  fromFolder,
  onClose,
  onSaved,
}: {
  existing: BundleShowData | null;
  fromFolder: string | null;
  onClose: () => void;
  onSaved: (name: string) => void;
}) {
  const [name, setName] = useState(existing?.name ?? "");
  const [fields, setFields] = useState<BundleFields>(fieldsOf(existing));
  const [reviewing, setReviewing] = useState(false);
  const save = (dryRun: boolean) =>
    existing
      ? editBundle(existing.name, fields, dryRun)
      : addBundle(name.trim(), fields, { fromFolder: fromFolder ?? undefined, dryRun });

  if (reviewing) {
    return (
      <PlanDialog
        title={
          existing ? `Save changes to ${existing.name}` : `Create bundle ${name.trim()}`
        }
        confirmLabel="Save bundle"
        preview={() => save(true)}
        confirm={() => save(false)}
        onClose={() => setReviewing(false)}
        onDone={() => {
          onSaved(existing?.name ?? name.trim());
          onClose();
        }}
      />
    );
  }
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{existing ? `Edit ${existing.name}` : "New bundle"}</DialogTitle>
          <DialogDescription>
            {fromFolder && !existing
              ? `Starts from what ${fromFolder} hides today; what you type below is added.`
              : "Each list you give replaces that list."}
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            setReviewing(true);
          }}
        >
          {!existing && (
            <label className="flex flex-col gap-1 text-sm">
              Name
              <Input value={name} onChange={(e) => setName(e.target.value)} required />
            </label>
          )}
          {LABELS.map(([key, label, hint]) => (
            <label key={key} className="flex flex-col gap-1 text-sm">
              {label}
              <Input
                value={fields[key]}
                placeholder={hint}
                onChange={(e) => setFields({ ...fields, [key]: e.target.value })}
              />
            </label>
          ))}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              Cancel
            </Button>
            <Button type="submit" disabled={!existing && !name.trim()}>
              Review the plan
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function Detail({
  row,
  folder,
  home,
  onChanged,
  onEdit,
}: {
  row: BundleRow;
  folder: string | null;
  home: string | null;
  onChanged: () => void;
  onEdit: (bundle: BundleShowData) => void;
}) {
  const detail = useLoad(() => showBundle(row.name), row.name);
  const [dialog, setDialog] = useState<"apply" | "delete" | { undo: string } | null>(
    null,
  );
  const shown = detail.data;
  const here = folder !== null && row.appliedTo.includes(folder);

  async function copyLaunch() {
    try {
      const data = await launchCommand(row.name, folder ?? undefined);
      await navigator.clipboard.writeText(data.command);
      toast.success("Launch command copied", { description: data.command });
    } catch (error) {
      toastError("Could not write the launch file", { description: errorText(error) });
    }
  }

  return (
    <section aria-label={`Bundle ${row.name}`} className="flex flex-col gap-4">
      <div className="flex flex-col gap-1">
        <h3 className="flex items-center gap-2 text-base font-semibold">
          {row.name}
          {row.appliedTo.length > 0 && (
            <Badge variant="partial">
              applied in {plural(row.appliedTo.length, "folder")}
            </Badge>
          )}
          <Badge variant="outline">not measured</Badge>
        </h3>
        <p className="text-meta text-muted-foreground">
          {row.description || "No description"}
        </p>
      </div>
      <div className="flex flex-wrap gap-2">
        <Button disabled={!folder || here} onClick={() => setDialog("apply")}>
          {folder ? `Apply to ${shortPath(folder, home)}…` : "Apply to a folder…"}
        </Button>
        <Button
          variant="outline"
          disabled={!shown}
          onClick={() => shown && onEdit(shown)}
        >
          Edit
        </Button>
        <Button variant="outline" onClick={() => void copyLaunch()}>
          Copy launch command
        </Button>
        <Button variant="outline" onClick={() => setDialog("delete")}>
          Delete…
        </Button>
      </div>
      {detail.error && <p role="alert">{detail.error}</p>}
      {shown && (
        <>
          <Group title="What it changes">
            <RowShell
              title="Instruction files left out"
              subtitle={shown.layers.exclude.length ? csv(shown.layers.exclude) : "none"}
            />
            <RowShell
              title="Plugins off"
              subtitle={shown.plugins.off.length ? csv(shown.plugins.off) : "none"}
            />
            <RowShell
              title="Auto memory"
              subtitle={shown.memory?.off ? "off" : "as is"}
            />
            <RowShell
              title="Skills"
              subtitle={`${shown.skills.off.length} off · ${shown.skills.nameOnly.length} name only. Hidden skills are hygiene, not savings.`}
            />
            <RowShell
              title="Agents off"
              subtitle={shown.agents.off.length ? csv(shown.agents.off) : "none"}
            />
            <RowShell
              title="Tools blocked"
              subtitle={shown.mcp.deny.length ? csv(shown.mcp.deny) : "none"}
            />
            <RowShell
              title="Access set"
              subtitle={
                shown.servers
                  ? `${shown.servers}: applying the bundle does not change which servers a folder reaches; that lives in Settings › Access.`
                  : "none"
              }
            />
            <RowShell
              title="Applies to"
              subtitle={shown.bind.length ? csv(shown.bind) : "No folder pattern"}
            />
          </Group>
          {shown.issues.length > 0 && (
            <StatusCard
              variant="warning"
              icon={<Boxes />}
              title={plural(shown.issues.length, "thing")}
              detail={shown.issues
                .map((i) => String(i.message ?? JSON.stringify(i)))
                .join(" · ")}
            />
          )}
        </>
      )}
      <Group title="Applied in" count={row.appliedTo.length}>
        {row.appliedTo.length === 0 && <RowShell title="Not applied in any folder" />}
        {row.appliedTo.map((path) => (
          <RowShell
            key={path}
            title={shortPath(path, home)}
            trailing={
              <Button
                size="sm"
                variant="outline"
                onClick={() => setDialog({ undo: path })}
              >
                Undo…
              </Button>
            }
          />
        ))}
      </Group>

      {dialog === "apply" && folder && (
        <PlanDialog
          title={`Apply ${row.name} to ${shortPath(folder, home)}`}
          description="Writes git-ignored local files in that folder. Nothing else is touched."
          confirmLabel="Apply"
          preview={() => applyBundle(row.name, folder, true)}
          confirm={() => applyBundle(row.name, folder, false)}
          onClose={() => setDialog(null)}
          onDone={onChanged}
        />
      )}
      {typeof dialog === "object" && dialog && (
        <PlanDialog
          title={`Undo ${row.name} in ${shortPath(dialog.undo, home)}`}
          description="Puts back exactly what the bundle replaced. Keys changed since are left alone."
          confirmLabel="Undo"
          preview={() => undoBundle(dialog.undo, true)}
          confirm={() => undoBundle(dialog.undo, false)}
          onClose={() => setDialog(null)}
          onDone={onChanged}
        />
      )}
      {dialog === "delete" && (
        <PlanDialog
          title={`Delete ${row.name}`}
          description="Removes the bundle definition. Folders it is applied in keep their settings, and the bundle must be undone there first."
          confirmLabel="Delete bundle"
          destructive
          preview={() => removeBundle(row.name, true)}
          confirm={() => removeBundle(row.name, false)}
          onClose={() => setDialog(null)}
          onDone={onChanged}
        />
      )}
    </section>
  );
}

export interface BundlesProps {
  folder: string | null;
  home: string | null;
}

export default function Bundles({ folder, home }: BundlesProps) {
  const list = useLoad(listBundles, folder);
  const config = useLoad(() => bundleConfig(), "config");
  const [selected, setSelected] = useState<string | null>(null);
  const [form, setForm] = useState<{
    existing: BundleShowData | null;
    fromFolder: string | null;
  } | null>(null);

  async function toggleAuto(on: boolean) {
    try {
      await bundleConfig(on);
      config.reload();
    } catch (error) {
      toastError("Could not change the switch", { description: errorText(error) });
    }
  }

  if (list.error && !list.data) {
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<Boxes />}
        title="The bundles could not be read"
        detail={list.error}
        actions={
          <Button size="sm" variant="outline" onClick={list.reload}>
            Try again
          </Button>
        }
      />
    );
  }
  if (!list.data)
    return <p className="text-meta text-muted-foreground">Reading the bundles…</p>;

  const rows = list.data.bundles;
  const current = rows.find((r) => r.name === selected) ?? rows[0] ?? null;

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <label className="flex items-center gap-2 text-sm">
          <Switch
            aria-label="Apply automatically in bound folders"
            checked={config.data?.autoApply ?? false}
            disabled={!config.data}
            onCheckedChange={(on) => void toggleAuto(on)}
          />
          Apply automatically in bound folders
        </label>
        <div className="flex gap-2">
          <Button
            variant="outline"
            disabled={!folder}
            onClick={() => setForm({ existing: null, fromFolder: folder })}
          >
            Create from this folder…
          </Button>
          <Button onClick={() => setForm({ existing: null, fromFolder: null })}>
            <Plus data-icon="inline-start" />
            New bundle
          </Button>
        </div>
      </div>

      {rows.length === 0 ? (
        <StatusCard
          variant="info"
          icon={<Boxes />}
          title="No bundles yet"
          detail={`A bundle is a saved choice of what Claude loads in a folder. Shape a folder with the switches on This folder and save it from there, or start empty. Bundles live in ${list.data.directory}.`}
        />
      ) : (
        <div className="grid gap-4 md:grid-cols-[16rem_1fr]">
          <Group title="Bundles" count={rows.length}>
            {rows.map((row) => (
              <button
                key={row.name}
                type="button"
                aria-pressed={current?.name === row.name}
                onClick={() => setSelected(row.name)}
                className="flex flex-col gap-0.5 px-3 py-2.5 text-left hover:bg-muted aria-pressed:bg-muted"
              >
                <span className="text-sm font-medium">{row.name}</span>
                <span className="text-meta text-muted-foreground">{row.description}</span>
                <span className="flex gap-1.5 pt-0.5">
                  {row.appliedTo.length > 0 ? (
                    <Badge variant="partial">
                      applied in {plural(row.appliedTo.length, "folder")}
                    </Badge>
                  ) : (
                    <Badge variant="off">not applied</Badge>
                  )}
                  {row.issues > 0 && (
                    <Badge variant="warning">{plural(row.issues, "issue")}</Badge>
                  )}
                </span>
              </button>
            ))}
          </Group>
          {current && (
            <Detail
              key={current.name}
              row={current}
              folder={folder}
              home={home}
              onChanged={list.reload}
              onEdit={(bundle) => setForm({ existing: bundle, fromFolder: null })}
            />
          )}
        </div>
      )}

      {form && (
        <BundleForm
          existing={form.existing}
          fromFolder={form.fromFolder}
          onClose={() => setForm(null)}
          onSaved={(name) => {
            setSelected(name);
            list.reload();
          }}
        />
      )}
    </div>
  );
}

/** From This folder: pick a bundle for the folder, see the plan, apply it with `context use`;
 * or stop using the applied one. */
export function UseBundleDialog({
  folder,
  applied,
  onClose,
  onDone,
}: {
  folder: string;
  applied: string | null;
  onClose: () => void;
  onDone: () => void;
}) {
  const list = useLoad(listBundles, folder);
  const status = useLoad(() => bundleStatus(folder), folder);
  const [name, setName] = useState<string | null>(null);

  if (applied || name) {
    const stopping = applied !== null && !name;
    return (
      <PlanDialog
        title={stopping ? `Stop using ${applied} here` : `Use ${name} here`}
        description={
          stopping
            ? "Puts back exactly what the bundle replaced. Keys changed since are left alone."
            : "Writes git-ignored local files in this folder. Nothing else is touched."
        }
        confirmLabel={stopping ? "Stop using" : "Use bundle"}
        preview={() =>
          stopping ? stopUsingBundle(folder, true) : contextUse(name!, folder, true)
        }
        confirm={() =>
          stopping ? stopUsingBundle(folder, false) : contextUse(name!, folder, false)
        }
        onClose={onClose}
        onDone={onDone}
      />
    );
  }
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Apply a bundle here</DialogTitle>
          <DialogDescription>
            {status.data?.conflicts.length
              ? "Something in this folder blocks a bundle."
              : "Choose a bundle; the plan comes next."}
          </DialogDescription>
        </DialogHeader>
        {list.data?.bundles.length === 0 && (
          <p className="text-meta">There are no bundles yet.</p>
        )}
        <ul className="flex flex-col gap-1">
          {(list.data?.bundles ?? []).map((b) => (
            <li key={b.name}>
              <button
                type="button"
                className="flex w-full flex-col rounded-md px-2 py-1.5 text-left hover:bg-muted"
                onClick={() => setName(b.name)}
              >
                <span className="text-sm font-medium">{b.name}</span>
                <span className="text-meta text-muted-foreground">{b.description}</span>
              </button>
            </li>
          ))}
        </ul>
      </DialogContent>
    </Dialog>
  );
}
