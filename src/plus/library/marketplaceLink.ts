import type { TapRow } from "./data";

/** How a plugin-marketplace tap stands against Claude Code's settings: `here` when the settings
 * point at this clone. A tap without a marketplace has no link to make. */
export type LinkState = "linked" | "not-linked" | "elsewhere" | "unreadable" | null;

export function linkState(tap: TapRow): LinkState {
  const market = tap.marketplace;
  if (!market) return null;
  if (market.error) return "unreadable";
  if (market.declared === "here") return "linked";
  if (market.declared?.startsWith("elsewhere")) return "elsewhere";
  return "not-linked";
}
