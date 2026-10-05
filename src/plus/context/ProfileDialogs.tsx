import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import type { WriteControl } from "./hooks";
import { FolderField } from "./folder";
import {
  applyArgv,
  applyCommand,
  bundleFlags,
  emptyBundleForm,
  nameProblem,
  type BundleForm,
} from "./bundleModel";
import { CheckField, Field, FormDialog } from "./parts";
import { folderLabel } from "./model";

export type ProfileMode = "new" | "from-folder" | "edit" | "duplicate";

const LISTS: Array<[keyof BundleForm, string, string]> = [
  ["skillsOff", "Skills turned off", "One name or pattern per line, e.g. scratch-*."],
  [
    "skillsNameOnly",
    "Skills shown by name only",
    "Their description stays out of the list.",
  ],
  [
    "skillsAllow",
    "Only these skills (legacy list)",
    "Every other library skill is turned off.",
  ],
  ["pluginsOff", "Plugins turned off", "Plugin ids such as name@market."],
  ["layersAdd", "CLAUDE.md layers added", "Layer names from the Layers tab."],
  ["layersExclude", "CLAUDE.md files left out", "Glob patterns."],
  ["agentsOff", "Agents turned off", "One agent name per line."],
  [
    "bind",
    "Applies to folders",
    "Patterns the profile is offered for. It is never applied by them.",
  ],
];

/** The form of `context bundle add` and `context bundle edit`: a new profile, one made from a
 * folder's settings, an edit of an existing one, or a copy under another name. */
export function ProfileForm({
  write,
  mode,
  initial = emptyBundleForm,
  taken,
  folders,
  onClose,
}: {
  write: WriteControl;
  mode: ProfileMode;
  initial?: BundleForm;
  taken: string[];
  folders: string[];
  onClose: () => void;
}) {
  const [form, setForm] = useState<BundleForm>(
    mode === "duplicate" ? { ...initial, name: `${initial.name}-copy` } : initial,
  );
  const [from, setFrom] = useState("");
  const set = (key: keyof BundleForm) => (value: string) =>
    setForm((now) => ({ ...now, [key]: value }));
  const edit = mode === "edit";
  const problem = edit ? null : nameProblem(form.name, taken);
  const valid =
    !!form.name && !problem && (mode !== "from-folder" || from.trim().length > 0);
  const title = {
    new: "New profile",
    "from-folder": "Create a profile from a folder",
    edit: `Edit profile ${initial.name}`,
    duplicate: `Duplicate profile ${initial.name}`,
  }[mode];
  return (
    <FormDialog
      title={title}
      intro={
        mode === "from-folder"
          ? "Reads the folder's local settings and keeps what Claude Code hides there as a profile. The folder is not changed."
          : "The definition is a file in your skills library. Nothing is applied to a folder yet."
      }
      submitLabel={edit ? "Review the changes" : "Review the profile"}
      valid={valid && (!edit || bundleFlags(form, initial).length > 0)}
      onClose={onClose}
      onSubmit={() => {
        const name = form.name.trim();
        const flags =
          mode === "from-folder"
            ? [
                "--from-folder",
                from.trim(),
                ...(form.description.trim()
                  ? ["--description", form.description.trim()]
                  : []),
              ]
            : bundleFlags({ ...form, name }, edit ? initial : undefined);
        write.begin({
          command: edit ? "context bundle edit" : "context bundle add",
          title: edit ? `Change profile ${name}` : `Create profile ${name}`,
          argv: ["context", "bundle", edit ? "edit" : "add", name, ...flags],
          confirmLabel: edit ? "Save changes" : "Create profile",
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
      <Field label="Description">
        {(id) => (
          <Input
            id={id}
            value={form.description}
            onChange={(event) => set("description")(event.target.value)}
          />
        )}
      </Field>
      {mode === "from-folder" ? (
        <FolderField
          label="Folder to read"
          value={from}
          onChange={setFrom}
          options={folders}
          placeholder="Folder where Claude starts"
        />
      ) : (
        <>
          <Field
            label="Server set"
            hint="The server profile paired under this name. Empty: the server profile with the same name, if there is one."
          >
            {(id, hint) => (
              <Input
                id={id}
                aria-describedby={hint}
                value={form.servers}
                autoComplete="off"
                onChange={(event) => set("servers")(event.target.value)}
              />
            )}
          </Field>
          {LISTS.map(([key, label, hint]) => (
            <Field key={key} label={label} hint={hint}>
              {(id, described) => (
                <Textarea
                  id={id}
                  aria-describedby={described}
                  rows={2}
                  spellCheck={false}
                  value={form[key]}
                  onChange={(event) => set(key)(event.target.value)}
                />
              )}
            </Field>
          ))}
        </>
      )}
    </FormDialog>
  );
}

/** "Apply to a folder…": the folder, then the plan of `context use` or `context bundle apply`. */
export function ApplyForm({
  write,
  profile,
  initialFolder,
  folders,
  onClose,
}: {
  write: WriteControl;
  profile: { name: string; servers: string | null };
  initialFolder: string;
  folders: string[];
  onClose: () => void;
}) {
  const [folder, setFolder] = useState(initialFolder);
  return (
    <FormDialog
      title={`Apply profile ${profile.name} to a folder`}
      intro="Writes only into the folder's git-ignored local files, and shows exactly which first."
      submitLabel="Review the plan"
      valid={folder.trim().length > 0}
      onClose={onClose}
      onSubmit={() => {
        const cwd = folder.trim();
        write.begin({
          command: applyCommand(profile),
          title: `Apply profile ${profile.name} to ${folderLabel(cwd)}`,
          argv: applyArgv(profile, cwd),
          confirmLabel: "Apply profile",
          phrase: profile.name,
        });
        onClose();
      }}
    >
      <FolderField
        value={folder}
        onChange={setFolder}
        options={folders}
        placeholder="Folder where Claude starts"
      />
    </FormDialog>
  );
}

/** "Delete…": a destructive write, confirmed by typing the name. A profile that is applied in
 * a folder is only removed when that is ticked: the folders keep their files. */
export function DeleteForm({
  write,
  name,
  applied,
  onClose,
}: {
  write: WriteControl;
  name: string;
  applied: number;
  onClose: () => void;
}) {
  const [force, setForce] = useState(false);
  return (
    <FormDialog
      title={`Delete profile ${name}`}
      intro="The definition file leaves your skills library. Folders where it was applied keep their files until you undo it there."
      submitLabel="Review the deletion"
      valid={applied === 0 || force}
      onClose={onClose}
      onSubmit={() => {
        write.begin({
          command: "context bundle rm",
          title: `Delete profile ${name}`,
          argv: ["context", "bundle", "rm", name, ...(force ? ["--force"] : [])],
          confirmLabel: "Delete profile",
          phrase: name,
        });
        onClose();
      }}
    >
      {applied > 0 && (
        <CheckField
          label={`Delete it although it is applied in ${applied} folder${applied === 1 ? "" : "s"}`}
          hint="Undo it in those folders first to clean their files."
          checked={force}
          onChange={setForce}
        />
      )}
    </FormDialog>
  );
}
