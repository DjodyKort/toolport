import { toast } from "sonner";
import { toastError } from "@/lib/toast";
import type { AttentionHideResult } from "../bridge/data";
import type { AttentionItem } from "../shell/attentionFeed";
import {
  hideRow,
  refreshAttention,
  snoozeChoices,
  undoLast,
  type HideChoice,
} from "./attentionCtl";

export const UNDO_MS = 8_000;

/** What the menu of one row offers. A needs-you row can only be snoozed, and never for longer than
 * a week; a row that carries a version or commit range can be skipped, which ends by itself when
 * that state changes. */
export function choicesFor(item: AttentionItem, now = new Date()): HideChoice[] {
  if (item.urgency === "needs-you") return snoozeChoices(now, false);
  const snoozes = snoozeChoices(now, true);
  if (item.skip) {
    return [{ kind: "skip", label: `Skip ${item.skip.label}`, args: [] }, ...snoozes];
  }
  return [{ kind: "dismiss", label: "This is on purpose", args: [] }, ...snoozes];
}

function untilText(until: string | undefined): string {
  const at = until === undefined ? NaN : Date.parse(until);
  if (Number.isNaN(at)) return "";
  return new Date(at).toLocaleString(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function hiddenMessage(result: AttentionHideResult, label?: string): string {
  const { kind, until } = result.hidden;
  if (kind === "skip") {
    return `Skipped ${label ?? result.title}. It returns when this changes.`;
  }
  if (kind === "snooze") return `Hidden until ${untilText(until)}: ${result.title}`;
  return `Dismissed: ${result.title}`;
}

/** Shows the toast that offers Undo for 8 s; Cmd-Z or Ctrl-Z undoes while it shows. */
export function offerUndo(message: string) {
  let open = true;
  const undo = async () => {
    if (!open) return;
    open = false;
    window.removeEventListener("keydown", onKey, true);
    toast.dismiss(id);
    try {
      await undoLast();
      refreshAttention();
    } catch (error) {
      toastError("Could not undo", {
        description: error instanceof Error ? error.message : String(error),
      });
    }
  };
  function onKey(event: KeyboardEvent) {
    if ((event.metaKey || event.ctrlKey) && !event.shiftKey && event.key === "z") {
      event.preventDefault();
      void undo();
    }
  }
  const id = toast(message, {
    duration: UNDO_MS,
    action: { label: "Undo", onClick: () => void undo() },
    onDismiss: () => {
      open = false;
      window.removeEventListener("keydown", onKey, true);
    },
    onAutoClose: () => {
      open = false;
      window.removeEventListener("keydown", onKey, true);
    },
  });
  window.addEventListener("keydown", onKey, true);
}

export async function hideWith(item: AttentionItem, choice: HideChoice) {
  try {
    const result = await hideRow(choice.kind, item.id, choice.args);
    refreshAttention();
    offerUndo(hiddenMessage(result, item.skip?.label));
  } catch (error) {
    toastError("Could not hide the row", {
      description: error instanceof Error ? error.message : String(error),
    });
  }
}
