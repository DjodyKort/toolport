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
import { toastError } from "@/lib/toast";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { UNDO_MS } from "./bodyEdit";
import { editFrontmatter, planFrontmatter, readFrontmatter } from "./commands";
import type { FrontmatterEdit, ListingMeter } from "./data";
import { LineDiffView } from "./DiffView";
import {
  draftsOf,
  listingLength,
  norm,
  patchOf,
  stillAsWritten,
  undoPatchOf,
  LISTING_KEYS,
  type Draft,
  type FieldKind,
  type FieldValue,
} from "./frontmatterModel";
import { Mono } from "./dialogs";
import { lineDiff } from "./lineDiff";
import { count, lengthText, plural } from "./model";

const SELECT =
  "h-8 rounded-md border bg-background px-2 text-sm text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none";

const ADD_KINDS: Array<{ value: FieldKind; label: string }> = [
  { value: "text", label: "Text" },
  { value: "boolean", label: "Yes or no" },
  { value: "number", label: "Number" },
];

/** Puts the fields of the edit back, unless one of them changed again after the edit. */
async function undoFieldsEdit(
  name: string,
  before: Record<string, unknown>,
  written: Record<string, FieldValue>,
  refresh: () => void,
) {
  try {
    const now = await readFrontmatter(name);
    if (!stillAsWritten(now.fields, written)) {
      toastError(`${name} was not changed back`, {
        description:
          "One of the fields changed again after you saved it, so it is left as it is.",
      });
      return;
    }
    await editFrontmatter(name, undoPatchOf(before, written));
    toast.message(`The previous fields of ${name} are back`);
  } catch (error) {
    toastError(`Could not change ${name} back`, { description: errorText(error) });
  } finally {
    refresh();
  }
}

function FieldRow({
  draft,
  onChange,
}: {
  draft: Draft;
  onChange: (next: Draft) => void;
}) {
  const id = useId();
  const required = norm(draft.key) === "description";
  const long =
    draft.kind === "text" &&
    (LISTING_KEYS.includes(norm(draft.key)) ||
      draft.text.length > 80 ||
      draft.text.includes("\n"));
  return (
    <div className="flex flex-col gap-1.5" data-testid={`field-${draft.key}`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <label htmlFor={id} className="font-mono text-xs font-medium break-all">
          {draft.key}
        </label>
        {draft.kind === "locked" && (
          <span className="text-meta text-muted-foreground">
            The name follows the folder; rename the folder to change it.
          </span>
        )}
        {draft.kind === "complex" && (
          <span className="text-meta text-muted-foreground">Edit it in your editor.</span>
        )}
        {!required && draft.kind !== "locked" && draft.kind !== "complex" && (
          <Button
            type="button"
            variant="ghost"
            size="xs"
            aria-label={`${draft.removed ? "Keep" : "Remove"} ${draft.key}`}
            onClick={() => onChange({ ...draft, removed: !draft.removed })}
          >
            {draft.removed ? "Keep" : "Remove"}
          </Button>
        )}
      </div>
      {draft.kind === "locked" || draft.kind === "complex" ? (
        <Mono>{draft.text}</Mono>
      ) : draft.removed ? (
        <p className="text-meta text-muted-foreground">
          Removed when you save. Keep it to leave the field as it is.
        </p>
      ) : draft.kind === "boolean" ? (
        <select
          id={id}
          className={SELECT}
          value={draft.text}
          onChange={(event) => onChange({ ...draft, text: event.target.value })}
        >
          <option value="true">Yes</option>
          <option value="false">No</option>
        </select>
      ) : long ? (
        <textarea
          id={id}
          value={draft.text}
          rows={LISTING_KEYS.includes(norm(draft.key)) ? 4 : 2}
          spellCheck={false}
          onChange={(event) => onChange({ ...draft, text: event.target.value })}
          className="w-full rounded-md border bg-background p-2 font-mono text-xs"
        />
      ) : (
        <Input
          id={id}
          value={draft.text}
          inputMode={draft.kind === "number" ? "decimal" : undefined}
          onChange={(event) => onChange({ ...draft, text: event.target.value })}
        />
      )}
    </div>
  );
}

function AddField({
  taken,
  onAdd,
}: {
  taken: (key: string) => boolean;
  onAdd: (draft: Draft) => void;
}) {
  const [key, setKey] = useState("");
  const [kind, setKind] = useState<FieldKind>("text");
  const [text, setText] = useState("");
  const [why, setWhy] = useState<string | null>(null);
  const keyId = useId();
  const kindId = useId();
  const valueId = useId();

  function add() {
    const name = key.trim();
    if (!/^[A-Za-z][A-Za-z0-9_-]*$/.test(name)) {
      setWhy("A field name is letters, digits, hyphens and underscores.");
      return;
    }
    if (taken(name)) {
      setWhy(`This skill already has a field called ${name}.`);
      return;
    }
    onAdd({
      key: name,
      kind,
      text: kind === "boolean" ? (text === "false" ? "false" : "true") : text,
      removed: false,
      added: true,
    });
    setKey("");
    setText("");
    setWhy(null);
  }

  return (
    <fieldset className="flex flex-col gap-2 rounded-md border p-3">
      <legend className="px-1 text-meta font-medium">Add a field</legend>
      <div className="flex flex-wrap items-end gap-2">
        <div className="flex min-w-32 flex-1 flex-col gap-1">
          <label htmlFor={keyId} className="text-meta text-muted-foreground">
            Field name
          </label>
          <Input
            id={keyId}
            value={key}
            placeholder="when_to_use"
            onChange={(event) => setKey(event.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1">
          <label htmlFor={kindId} className="text-meta text-muted-foreground">
            Type
          </label>
          <select
            id={kindId}
            className={SELECT}
            value={kind}
            onChange={(event) => setKind(event.target.value as FieldKind)}
          >
            {ADD_KINDS.map((item) => (
              <option key={item.value} value={item.value}>
                {item.label}
              </option>
            ))}
          </select>
        </div>
        <div className="flex min-w-32 flex-1 flex-col gap-1">
          <label htmlFor={valueId} className="text-meta text-muted-foreground">
            Value
          </label>
          {kind === "boolean" ? (
            <select
              id={valueId}
              className={SELECT}
              value={text === "false" ? "false" : "true"}
              onChange={(event) => setText(event.target.value)}
            >
              <option value="true">Yes</option>
              <option value="false">No</option>
            </select>
          ) : (
            <Input
              id={valueId}
              value={text}
              onChange={(event) => setText(event.target.value)}
            />
          )}
        </div>
        <Button type="button" variant="outline" size="sm" onClick={add}>
          Add
        </Button>
      </div>
      {why && (
        <p role="alert" className="text-meta text-destructive">
          {why}
        </p>
      )}
    </fieldset>
  );
}

/** The single fields of a skill's frontmatter: change, remove or add one, review what the file
 * would hold and what lint says of it, then save. The write only touches the lines of the fields
 * that change, Undo on the message puts their old values back, and a change the skill check
 * refuses is shown here instead of written. */
export default function SkillFieldsDialog({
  name,
  listing,
  onClose,
  onDone,
}: {
  name: string;
  listing: ListingMeter;
  onClose: () => void;
  onDone: () => void;
}) {
  const loaded = useLoad(() => readFrontmatter(name), `fields:${name}`);
  const [drafts, setDrafts] = useState<Draft[] | null>(null);
  const [plan, setPlan] = useState<FrontmatterEdit | null>(null);
  const [refused, setRefused] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const fields = loaded.data?.fields ?? {};
  const current = drafts ?? (loaded.data ? draftsOf(fields) : []);
  const { patch, problems } = patchOf(fields, current);
  const changed = Object.keys(patch).length;
  const parts = listingLength(current);
  const total = parts.description + parts.whenToUse;
  const over = total > listing.limit;
  const update = (next: Draft, at: number) =>
    setDrafts(current.map((draft, index) => (index === at ? next : draft)));

  async function review() {
    setBusy(true);
    setRefused(null);
    try {
      setPlan(await planFrontmatter(name, patch));
    } catch (error) {
      setRefused(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    setBusy(true);
    try {
      await editFrontmatter(name, patch);
      const before = fields;
      toast.message(`Saved the fields of ${name}`, {
        description: `${plural(changed, "field")} changed. Apply on the Sync tab delivers it to Claude Code.`,
        duration: UNDO_MS,
        action: {
          label: "Undo",
          onClick: () => void undoFieldsEdit(name, before, patch, onDone),
        },
      });
      onDone();
      onClose();
    } catch (error) {
      setPlan(null);
      setRefused(errorText(error));
    } finally {
      setBusy(false);
    }
  }

  const diff = plan ? lineDiff(plan.before, plan.after) : null;
  const errors = plan?.lint.filter((row) => row.level === "error") ?? [];
  const warnings = plan?.lint.filter((row) => row.level === "warning") ?? [];

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>
            {plan ? `Review the fields of ${name}` : `Fields of ${name}`}
          </DialogTitle>
          <DialogDescription>
            {plan
              ? "Nothing is written until you save. Only the lines of the fields below change."
              : "The frontmatter of SKILL.md, one field at a time. The text below it is edited with Edit body."}
          </DialogDescription>
        </DialogHeader>
        {loaded.error && !loaded.data && <p role="alert">{loaded.error}</p>}
        {!loaded.data && !loaded.error && <p className="text-meta">Reading {name}…</p>}

        {loaded.data && !plan && (
          <div className="flex flex-col gap-4">
            <Mono>{loaded.data.sourcePath}</Mono>
            {current.map((draft, index) => (
              <FieldRow
                key={`${draft.key}-${index}`}
                draft={draft}
                onChange={(next) => update(next, index)}
              />
            ))}
            <p
              className={
                over ? "text-meta text-destructive" : "text-meta text-muted-foreground"
              }
              aria-live="polite"
            >
              Claude Code reads{" "}
              {lengthText({
                length: total,
                description: parts.description,
                whenToUse: parts.whenToUse,
              })}{" "}
              of {count(listing.limit)}; the Agent Skills spec asks for a description of
              at most {count(listing.specLimit)}.
              {over && " This is too long for the skill to be read."}
            </p>
            <AddField
              taken={(key) => current.some((draft) => norm(draft.key) === norm(key))}
              onAdd={(draft) => setDrafts([...current, draft])}
            />
            {problems.length > 0 && (
              <ul role="alert" className="text-meta text-destructive">
                {problems.map((why) => (
                  <li key={why}>{why}</li>
                ))}
              </ul>
            )}
            {refused && (
              <p role="alert" className="text-meta text-destructive">
                {refused}
              </p>
            )}
          </div>
        )}

        {plan && diff && (
          <div className="flex flex-col gap-3">
            <p className="text-meta text-muted-foreground">
              {plan.changed.join(", ")} · {plural(diff.added, "line")} added,{" "}
              {plural(diff.removed, "line")} removed
              {plan.rewritten
                ? ". The frontmatter has a form that cannot be edited line by line, so it is written out again as a whole."
                : "."}
            </p>
            <LineDiffView lines={diff.lines} />
            <ul aria-label="Lint of the result" className="flex flex-col gap-1">
              {[...errors, ...warnings].map((row, index) => (
                <li
                  key={`${row.level}-${index}`}
                  className="flex items-start gap-2 text-meta"
                >
                  <Badge variant={row.level === "error" ? "destructive" : "warning"}>
                    {row.level === "error" ? "Error" : "Warning"}
                  </Badge>
                  <span className="min-w-0 break-words">{row.message}</span>
                </li>
              ))}
              {errors.length + warnings.length === 0 && (
                <li className="text-meta text-muted-foreground">Lint finds nothing.</li>
              )}
            </ul>
            {errors.length > 0 && (
              <p role="alert" className="text-meta text-destructive">
                Lint would hold this skill back from the sync. Change the fields so the
                error goes away before you save.
              </p>
            )}
            {refused && (
              <p role="alert" className="text-meta text-destructive">
                {refused}
              </p>
            )}
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          {plan ? (
            <>
              <Button variant="outline" onClick={() => setPlan(null)}>
                Back to editing
              </Button>
              <Button disabled={busy || errors.length > 0} onClick={() => void save()}>
                Save fields
              </Button>
            </>
          ) : (
            <Button
              disabled={!loaded.data || busy || changed === 0 || problems.length > 0}
              onClick={() => void review()}
            >
              Review changes
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
