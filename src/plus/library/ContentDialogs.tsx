import { useId, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { toastError } from "@/lib/toast";
import { useLoad } from "../servers/useLoad";
import { errorText } from "../settings/useCtl";
import { saveBodyEdit, sameBody } from "./bodyEdit";
import {
  addContent,
  cleanContent,
  getContent,
  type BodyKind,
  type ContentKind,
} from "./commands";
import { LineDiffView } from "./DiffView";
import { AskDialog, FileLines, Mono, WriteDialog } from "./dialogs";
import { lineDiff } from "./lineDiff";
import { plural } from "./model";

interface Done {
  onClose: () => void;
  onDone: () => void;
}

const NOUN: Record<BodyKind, string> = {
  skills: "skill",
  agents: "agent",
  styles: "output style",
};
const FILE: Record<ContentKind, string> = { agents: "AGENT.md", styles: "STYLE.md" };

export function NewContentDialog({
  kind,
  onClose,
  onDone,
}: Done & { kind: ContentKind }) {
  const noun = NOUN[kind];
  return (
    <AskDialog
      title={`New ${noun}`}
      description={`Creates a ${FILE[kind]} from a template in your library. It then shows in the Sync plan like any other change.`}
      label="Name"
      placeholder={kind === "agents" ? "code-scout" : "plain"}
      onClose={onClose}
      next={(name, back) => (
        <WriteDialog
          title={`New ${noun}: ${name}`}
          confirmLabel="Create"
          preview={() => addContent(kind, name, true)}
          render={(data) => (
            <div className="flex flex-col gap-2">
              <FileLines title="Creates" files={[data.path]} />
              <Button variant="ghost" size="sm" className="self-start" onClick={back}>
                Change the name
              </Button>
            </div>
          )}
          confirm={() => addContent(kind, name, false)}
          onClose={onClose}
          onDone={onDone}
        />
      )}
    />
  );
}

export function CleanContentDialog({
  kind,
  onClose,
  onDone,
}: Done & { kind: ContentKind }) {
  const noun = NOUN[kind];
  return (
    <WriteDialog
      title={`Remove synced ${noun}s`}
      description={`Removes the copies Toolport wrote for Claude Code and the other clients. Your library keeps the ${noun}s; the next Apply writes them again.`}
      confirmLabel="Remove synced copies"
      destructive
      preview={() => cleanContent(kind, true)}
      render={(data) =>
        data.removed.length === 0 ? (
          <p className="text-meta">There are no synced copies to remove.</p>
        ) : (
          <FileLines
            title={`${plural(data.removed.length, "file")} removed`}
            files={data.removed}
          />
        )
      }
      confirm={() => cleanContent(kind, false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

/** The body of a skill, an agent or a style as text. Review changes shows what the save would
 * change as a diff; saving replaces the body and leaves the frontmatter as it is. The toast has
 * Undo, and the next Apply delivers the change. */
export function EditBodyDialog({
  kind,
  name,
  onClose,
  onDone,
}: Done & { kind: BodyKind; name: string }) {
  const loaded = useLoad(() => getContent(kind, name), `${kind}:${name}`);
  const [draft, setDraft] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [busy, setBusy] = useState(false);
  const id = useId();
  const original = loaded.data?.body ?? "";
  const text = draft ?? original;
  const changed = loaded.data !== null && !sameBody(text, original);
  const diff = useMemo(
    () => (reviewing ? lineDiff(original, text) : null),
    [reviewing, original, text],
  );

  async function save() {
    setBusy(true);
    try {
      await saveBodyEdit(
        { kind, name, previous: original, written: text },
        "Apply on the Sync tab delivers it to Claude Code.",
        onDone,
      );
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
      <DialogContent className="max-h-[90vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>
            {reviewing ? `Review the changes to ${name}` : `Edit ${name}`}
          </DialogTitle>
          <DialogDescription>
            {reviewing
              ? "Nothing is written until you save. Undo stays on the message after saving."
              : `The text below the frontmatter of the ${NOUN[kind]}. The frontmatter stays as it is; edit it in your editor.`}
          </DialogDescription>
        </DialogHeader>
        {loaded.error && !loaded.data && <p role="alert">{loaded.error}</p>}
        {!loaded.data && !loaded.error && <p className="text-meta">Reading {name}…</p>}
        {loaded.data && !reviewing && (
          <div className="flex flex-col gap-2">
            <Mono>{loaded.data.path}</Mono>
            <label htmlFor={id} className="text-meta font-medium">
              Body
            </label>
            <textarea
              id={id}
              value={text}
              rows={16}
              spellCheck={false}
              onChange={(event) => setDraft(event.target.value)}
              className="w-full rounded-md border bg-background p-2 font-mono text-xs"
            />
          </div>
        )}
        {loaded.data && diff && (
          <div className="flex flex-col gap-2">
            <Mono>{loaded.data.path}</Mono>
            <p className="text-meta text-muted-foreground">
              {plural(diff.added, "line")} added, {plural(diff.removed, "line")} removed.
              {diff.coarse
                ? " The body is too different to line up, so it is shown as removed and added."
                : ""}
            </p>
            <LineDiffView lines={diff.lines} />
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          {reviewing ? (
            <>
              <Button variant="outline" onClick={() => setReviewing(false)}>
                Back to editing
              </Button>
              <Button disabled={busy} onClick={() => void save()}>
                Save body
              </Button>
            </>
          ) : (
            <Button disabled={!changed} onClick={() => setReviewing(true)}>
              Review changes
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
