import type { View } from "@/lib/types";

export const PLUS_NAVIGATE_EVENT = "plus-navigate";

/** Opens a page of the app from fork code that has no handle on the shell. The Claude group in the
 * sidebar listens and selects the view, so no upstream file passes a callback down. */
export function openView(view: View) {
  window.dispatchEvent(new CustomEvent<View>(PLUS_NAVIGATE_EVENT, { detail: view }));
}
