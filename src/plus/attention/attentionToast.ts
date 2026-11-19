import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import type { View } from "@/lib/types";
import { isPageShown } from "./shownPages";
import { openView } from "../shell/navigate";

/** Sent by the app's notifier when news arrives while the window has the focus. */
export const ATTENTION_TOAST_EVENT = "plus-attention-toast";
const TOAST_ID = "plus-attention";
const TOAST_MS = 60_000;
const PAGES: Record<string, View> = {
  servers: "servers",
  tasks: "tasks",
  library: "library",
};

export interface AttentionToast {
  title: string;
  body: string;
  rows: Array<{ id: string; screen: string }>;
}

/** No toast when every row belongs to a page that is showing its banner right now. */
export function worthShowing(payload: AttentionToast): boolean {
  return (
    payload.rows.length === 0 || payload.rows.some((row) => !isPageShown(row.screen))
  );
}

export function showAttentionToast(payload: AttentionToast) {
  if (!worthShowing(payload)) return;
  const page = payload.rows.map((row) => PAGES[row.screen]).find(Boolean);
  toast.warning(payload.title, {
    id: TOAST_ID,
    description: payload.body,
    duration: TOAST_MS,
    ...(page && { action: { label: "Open", onClick: () => openView(page) } }),
  });
}

/** Listens for the notifier's toasts; returns the function that stops listening. */
export function startAttentionToasts(): () => void {
  const stop = listen<AttentionToast>(ATTENTION_TOAST_EVENT, ({ payload }) =>
    showAttentionToast(payload),
  ).catch(() => () => {});
  return () => void stop.then((unlisten) => unlisten());
}
