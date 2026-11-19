import type { View } from "@/lib/types";

const shown = new Map<View, number>();

/** An attention banner for `page` is on screen until the returned function runs. A row that
 * belongs to a shown page is already in front of the person, so it needs no toast. */
export function registerShownPage(page: View): () => void {
  shown.set(page, (shown.get(page) ?? 0) + 1);
  return () => {
    const left = (shown.get(page) ?? 1) - 1;
    if (left <= 0) shown.delete(page);
    else shown.set(page, left);
  };
}

export function isPageShown(page: string): boolean {
  return shown.has(page as View);
}
