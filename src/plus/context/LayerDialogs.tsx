import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import type { WriteControl } from "./hooks";
import { CheckField, Field, FormDialog, SELECT_CLASS } from "./parts";
import { listOf } from "./bundleModel";

export interface LayerForm {
  name: string;
  scope: string;
  glob: string;
  folders: string;
  imports: string;
  delivery: string;
}

export const emptyLayerForm: LayerForm = {
  name: "",
  scope: "glob",
  glob: "",
  folders: "",
  imports: "",
  delivery: "copy",
};

export const LAYER_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;

/** The layer `context client add <name>` creates: `client-` and the name as the CLI slugs it. */
export function layerNameOf(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9-]/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
  return `client-${slug || "client"}`;
}

/** The flags of `context client add|edit`: every filled field for an add, the fields that
 * differ from `before` for an edit. A repeated flag carries one folder or import each. */
export function layerFlags(form: LayerForm, before?: LayerForm): string[] {
  const flags: string[] = [];
  const changed = (key: keyof LayerForm) => !before || form[key] !== before[key];
  if (changed("scope")) flags.push("--scope", form.scope);
  if (changed("glob") && form.glob.trim()) flags.push("--glob", form.glob.trim());
  if (changed("folders"))
    for (const folder of listOf(form.folders)) flags.push("--folder", folder);
  if (changed("imports"))
    for (const path of listOf(form.imports)) flags.push("--import", path);
  if (changed("delivery")) flags.push("--delivery", form.delivery);
  return flags;
}

/** The form of "Add layer…" and "Edit": scope, folders, imports and delivery of a layer. */
export function LayerFormDialog({
  write,
  initial,
  taken,
  onClose,
}: {
  write: WriteControl;
  initial?: LayerForm;
  taken: string[];
  onClose: () => void;
}) {
  const [form, setForm] = useState<LayerForm>(initial ?? emptyLayerForm);
  const edit = !!initial;
  const set = (key: keyof LayerForm) => (value: string) =>
    setForm((now) => ({ ...now, [key]: value }));
  const name = form.name.trim();
  const problem =
    edit || !name
      ? null
      : !LAYER_NAME.test(name)
        ? "Letters, digits, dot, dash and underscore."
        : taken.includes(layerNameOf(name))
          ? "A layer with this name exists."
          : null;
  const needsGlob = form.scope === "glob" && !form.glob.trim();
  const needsFolder = form.scope === "folder" && listOf(form.folders).length === 0;
  const valid =
    !!name &&
    !problem &&
    !needsGlob &&
    !needsFolder &&
    (!edit || layerFlags(form, initial).length > 0);
  return (
    <FormDialog
      title={edit ? `Edit layer ${name}` : "Add a layer"}
      intro="A layer is your own section on top of the org file. Each has a scope: everywhere, a folder pattern, or chosen folders."
      submitLabel="Review the layer"
      valid={valid}
      onClose={onClose}
      onSubmit={() => {
        write.begin({
          command: edit ? "context client edit" : "context client add",
          title: edit ? `Change layer ${name}` : `Add layer ${name}`,
          argv: [
            "context",
            "client",
            edit ? "edit" : "add",
            name,
            ...layerFlags(form, initial),
          ],
          confirmLabel: edit ? "Save changes" : "Add layer",
          phrase: name,
        });
        onClose();
      }}
    >
      <Field label="Name" hint={problem ?? "Letters, digits, dot, dash and underscore."}>
        {(id, hint) => (
          <Input
            id={id}
            aria-describedby={hint}
            value={form.name}
            disabled={edit}
            autoComplete="off"
            onChange={(event) => set("name")(event.target.value)}
          />
        )}
      </Field>
      <Field label="Scope">
        {(id) => (
          <select
            id={id}
            className={SELECT_CLASS}
            value={form.scope}
            onChange={(event) => set("scope")(event.target.value)}
          >
            <option value="global">Everywhere</option>
            <option value="glob">A folder pattern</option>
            <option value="folder">Folders you pick</option>
          </select>
        )}
      </Field>
      {form.scope === "glob" && (
        <Field label="Folder pattern" hint="A glob such as **/clients/acme/**.">
          {(id, hint) => (
            <Input
              id={id}
              aria-describedby={hint}
              value={form.glob}
              autoComplete="off"
              onChange={(event) => set("glob")(event.target.value)}
            />
          )}
        </Field>
      )}
      {form.scope === "folder" && (
        <Field label="Folders" hint="One folder per line.">
          {(id, hint) => (
            <Textarea
              id={id}
              aria-describedby={hint}
              rows={3}
              spellCheck={false}
              value={form.folders}
              onChange={(event) => set("folders")(event.target.value)}
            />
          )}
        </Field>
      )}
      <Field
        label="Imports"
        hint="Files or layers whose text this layer carries, one per line."
      >
        {(id, hint) => (
          <Textarea
            id={id}
            aria-describedby={hint}
            rows={3}
            spellCheck={false}
            value={form.imports}
            onChange={(event) => set("imports")(event.target.value)}
          />
        )}
      </Field>
      <CheckField
        label="Keep the imported text inside the layer (recommended)"
        hint="Copy works everywhere and costs the same tokens. A headless session does not load an import that points outside the project."
        checked={form.delivery === "copy"}
        onChange={(on) => set("delivery")(on ? "copy" : "import")}
      />
    </FormDialog>
  );
}

/** "Delete…": a destructive write, confirmed by typing the name. */
export function LayerDeleteDialog({
  write,
  name,
  onClose,
}: {
  write: WriteControl;
  name: string;
  onClose: () => void;
}) {
  return (
    <FormDialog
      title={`Delete layer ${name}`}
      intro="The layer file leaves your library and its deployed copies are removed on the next sync."
      submitLabel="Review the deletion"
      onClose={onClose}
      onSubmit={() => {
        write.begin({
          command: "context client rm",
          title: `Delete layer ${name}`,
          argv: ["context", "client", "rm", name],
          confirmLabel: "Delete layer",
          phrase: name,
        });
        onClose();
      }}
    >
      <p className="text-sm">
        Folders keep what was already delivered until the next sync.
      </p>
    </FormDialog>
  );
}
