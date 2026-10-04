import { useId, useState } from "react";
import { FolderOpen } from "lucide-react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { WriteScope } from "./model";

export type PickKind = "folder" | "file" | "save";

/** A path typed in a text input, with the app's native picker as a convenience. Where the
 * picker is not there (the browser fixture) or fails, the typed path is all there is. */
export function PathField({
  label,
  value,
  onChange,
  kind,
  hint,
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  kind: PickKind;
  hint?: string;
  placeholder?: string;
}) {
  const [note, setNote] = useState<string | null>(null);
  const hintId = useId();
  const id = useId();
  async function choose() {
    setNote(null);
    try {
      const picked =
        kind === "save"
          ? await save({ title: label, filters: [{ name: "Zip", extensions: ["zip"] }] })
          : await open({
              directory: kind === "folder",
              multiple: false,
              title: label,
              filters:
                kind === "file" ? [{ name: "Zip", extensions: ["zip"] }] : undefined,
            });
      if (typeof picked === "string") onChange(picked);
    } catch {
      setNote("The picker is not available here. Type the path instead.");
    }
  }
  return (
    <div className="flex flex-col gap-1.5 text-sm">
      <label htmlFor={id}>{label}</label>
      <span className="flex gap-2">
        <Input
          id={id}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
          autoComplete="off"
          spellCheck={false}
          aria-describedby={hint ? hintId : undefined}
          className="font-mono text-xs"
        />
        <Button
          type="button"
          variant="outline"
          aria-label={`Choose ${label.toLowerCase()}`}
          onClick={() => void choose()}
        >
          <FolderOpen /> Choose…
        </Button>
      </span>
      {hint && (
        <p id={hintId} className="text-xs text-muted-foreground">
          {hint}
        </p>
      )}
      {note && (
        <p role="status" className="text-xs text-warning">
          {note}
        </p>
      )}
    </div>
  );
}

/** Where sync, clean, resolve and uninstall write: the user level (the default) or one project.
 * Reads always show the user level; every write is previewed with the scope it will use. */
export function ScopeBar({
  scope,
  onChange,
}: {
  scope: WriteScope;
  onChange: (scope: WriteScope) => void;
}) {
  return (
    <div
      role="group"
      aria-label="Where writes go"
      className="flex flex-col gap-3 rounded-lg border bg-card p-3 text-sm"
    >
      <fieldset className="flex flex-wrap items-center gap-4">
        <legend className="sr-only">Scope</legend>
        <span className="text-muted-foreground">
          Sync, clean, resolve and uninstall write to
        </span>
        {[
          [false, "your user level"],
          [true, "one project"],
        ].map(([project, text]) => (
          <label key={String(project)} className="flex items-center gap-1.5">
            <input
              type="radio"
              name="write-scope"
              checked={scope.project === project}
              onChange={() => onChange({ ...scope, project: project as boolean })}
            />
            {text as string}
          </label>
        ))}
      </fieldset>
      {scope.project && (
        <PathField
          label="Project folder"
          kind="folder"
          value={scope.dir}
          onChange={(dir) => onChange({ ...scope, dir })}
          placeholder="Empty: the configured skills repository"
          hint="The project's skills repository (--repo) and where its outputs go (--project). Its sync keeps its own choice of clients."
        />
      )}
    </div>
  );
}
