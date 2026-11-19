import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { editContentBody, getContent, type BodyKind } from "./commands";

/** How long the Undo on the toast stays: an edit of text is worth longer than a switch. */
export const UNDO_MS = 30_000;

/** The writer ends a body with exactly one newline, so text that differs only after its last
 * visible character is the same body. */
export const sameBody = (a: string, b: string) => a.trimEnd() === b.trimEnd();

export interface BodyEdit {
  kind: BodyKind;
  name: string;
  /** The body as it was read before the edit. */
  previous: string;
  /** The body the edit wrote. */
  written: string;
}

/** Puts the previous body back, unless the body changed again after the edit: then it is left
 * alone, the same way Undo of a sync skips a file changed since. */
export async function undoBodyEdit(edit: BodyEdit, refresh: () => void) {
  try {
    const current = await getContent(edit.kind, edit.name);
    if (!sameBody(current.body, edit.written)) {
      toastError(`${edit.name} was not changed back`, {
        description: "Its body changed again after you saved it, so it is left as it is.",
      });
      return;
    }
    await editContentBody(edit.kind, edit.name, edit.previous);
    toast.message(`The previous body of ${edit.name} is back`);
  } catch (error) {
    toastError(`Could not change ${edit.name} back`, { description: errorText(error) });
  } finally {
    refresh();
  }
}

/** Writes the body, then offers Undo on the toast. A failed write throws and changes nothing. */
export async function saveBodyEdit(
  edit: BodyEdit,
  deliveredBy: string,
  refresh: () => void,
) {
  await editContentBody(edit.kind, edit.name, edit.written);
  toast.message(`Saved the body of ${edit.name}`, {
    description: deliveredBy,
    duration: UNDO_MS,
    action: { label: "Undo", onClick: () => void undoBodyEdit(edit, refresh) },
  });
}
