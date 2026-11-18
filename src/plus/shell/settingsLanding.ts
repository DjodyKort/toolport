import { SETTINGS_PAGES } from "@/lib/settingsPages";
import type { SettingsTarget } from "./navigate";

const RETRY_MS = 50;
const RETRIES = 60;

function byLabel(label: string): HTMLElement | undefined {
  return Array.from(document.querySelectorAll<HTMLElement>("[aria-label]")).find(
    (node) => node.getAttribute("aria-label") === label,
  );
}

function pageRow(listLabel: string, page: SettingsTarget["page"]): HTMLElement | null {
  const index = SETTINGS_PAGES.findIndex((item) => item.id === page);
  return byLabel(listLabel)?.children[index]?.querySelector("button") ?? null;
}

/** Presses the sidebar row of a Settings page once Settings is open, then scrolls to the
 * section. Gives up quietly after about three seconds; the person is then on the page Settings
 * opens with. */
export function landOnSettingsPage(target: SettingsTarget, listLabel: string) {
  let tries = 0;
  let pressed = false;
  const step = () => {
    tries += 1;
    if (!pressed) {
      const row = pageRow(listLabel, target.page);
      if (row) {
        if (row.getAttribute("aria-current") !== "page") row.click();
        pressed = true;
      }
    }
    const section =
      pressed && target.section
        ? document.querySelector<HTMLElement>(`section[aria-label="${target.section}"]`)
        : null;
    if (section) {
      section.scrollIntoView?.({ block: "start" });
      return;
    }
    if (pressed && !target.section) return;
    if (tries < RETRIES) window.setTimeout(step, RETRY_MS);
  };
  window.setTimeout(step, 0);
}
