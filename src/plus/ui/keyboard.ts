import { SHORTCUT_VIEWS, isTextEntry, type ShortcutTarget } from "@/lib/shortcuts";

/**
 * Keyboard policy of the Toolport+ screens.
 *
 * - The number keys stay at the shipped 7 views (`SHORTCUT_VIEWS`). The new screens add no
 *   global chord: they are reached from the sidebar and from the rows of Attention.
 * - A screen that wants a bare key uses `screenKeyAllowed`: never while typing in a field,
 *   never with Ctrl, Cmd or Alt held, never `/`, `?` or Escape, which belong to the shell.
 * - Tabs move with Left/Right (Up/Down when vertical), Home and End, and select as they
 *   move, so a tab list is one Tab stop (`nextTabIndex`).
 * - Escape closes a dialog and never cancels a running job or confirms anything. Enter
 *   confirms a plain dialog but never a typed confirmation: that button is clicked.
 */
export const NUMBER_KEY_VIEWS = SHORTCUT_VIEWS.length;

const SHELL_KEYS = new Set(["/", "?", "Escape"]);

export interface ScreenKeyEvent {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
}

export function screenKeyAllowed(
  event: ScreenKeyEvent,
  target?: ShortcutTarget | null,
): boolean {
  if (event.ctrlKey || event.metaKey || event.altKey) return false;
  if (SHELL_KEYS.has(event.key)) return false;
  return !isTextEntry(target);
}

export type TabOrientation = "horizontal" | "vertical";

/** The tab a navigation key moves to, or `null` when the key is not one. */
export function nextTabIndex(
  key: string,
  current: number,
  count: number,
  orientation: TabOrientation = "horizontal",
): number | null {
  if (count <= 0) return null;
  const forward = orientation === "horizontal" ? "ArrowRight" : "ArrowDown";
  const back = orientation === "horizontal" ? "ArrowLeft" : "ArrowUp";
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  if (key === forward) return (current + 1) % count;
  if (key === back) return (current - 1 + count) % count;
  return null;
}
