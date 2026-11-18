import type { SettingsSubpage } from "@/lib/settingsPages";
import type { View } from "@/lib/types";

export const PLUS_NAVIGATE_EVENT = "plus-navigate";

/** Opens a page of the app from fork code that has no handle on the shell. The Claude group in the
 * sidebar listens and selects the view, so no upstream file passes a callback down. */
export function openView(view: View) {
  window.dispatchEvent(new CustomEvent<View>(PLUS_NAVIGATE_EVENT, { detail: view }));
}

export const PLUS_SETTINGS_EVENT = "plus-open-settings";

export interface SettingsTarget {
  page: SettingsSubpage;
  /** The `aria-label` of a `<section>` on that page to scroll to once it is drawn. */
  section?: string;
}

/** Opens a page of Settings, and a section of it, from fork code. Upstream keeps the open
 * Settings page in `App` state that only its own sidebar rows set, so the Claude group's listener
 * presses the matching row. */
export function openSettingsPage(target: SettingsTarget) {
  window.dispatchEvent(
    new CustomEvent<SettingsTarget>(PLUS_SETTINGS_EVENT, { detail: target }),
  );
}
