import type { AttentionItem } from "../shell/attentionFeed";
import { openView } from "../shell/navigate";

export interface Reviewed {
  server: string;
  verb: "update" | "switch" | "package";
  fetch: boolean;
}

/** The server a row's button reviews, when its command is only the plan of a server update: `server
 * repo update|switch` for a checkout, `server update` for a pinned package or a GitHub release. The
 * button then opens the plan instead of running the command. */
export function reviewedServer(command: string[]): Reviewed | null {
  const [, group, sub, ...rest] = command;
  if (group !== "server") return null;
  if (sub === "update") {
    const [server] = rest;
    return server && command.includes("--dry-run")
      ? { server, verb: "package", fetch: false }
      : null;
  }
  const [verb, server] = rest;
  if (sub !== "repo" || !server) return null;
  if (verb === "update") return { server, verb, fetch: command.includes("--fetch") };
  return verb === "switch" ? { server, verb, fetch: false } : null;
}

/** The button of a row that lives in a popover or on the Manage page: an update row opens its plan,
 * any other opens the page it is about. */
export function openAction(item: AttentionItem) {
  if (!item.action) return;
  const reviewed = item.action.command ? reviewedServer(item.action.command) : null;
  if (reviewed) requestReview(reviewed);
  else openView(item.action.view);
}

export const REVIEW_EVENT = "plus-review-update";

/** Opens the plan of an update from a row that lives in a popover or another page: the dialog is
 * drawn by the host in the sidebar, which outlives both. */
export function requestReview(reviewed: Reviewed) {
  window.dispatchEvent(new CustomEvent<Reviewed>(REVIEW_EVENT, { detail: reviewed }));
}
