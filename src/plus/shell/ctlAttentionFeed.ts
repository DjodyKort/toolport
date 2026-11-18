import { ctlData } from "../bridge/ctl";
import { ATTENTION_REFRESH_EVENT } from "../attention/attentionCtl";
import type { AttentionData, AttentionRow } from "../bridge/data";
import {
  registerAttentionFeed,
  type AttentionFeed,
  type AttentionItem,
  type AttentionSnapshot,
} from "./attentionFeed";

export const ATTENTION_POLL_MS = 60_000;

const VIEWS = { servers: "servers", tasks: "tasks", library: "library" } as const;

function time(text: string | undefined): number | undefined {
  const at = text === undefined ? NaN : Date.parse(text);
  return Number.isNaN(at) ? undefined : at;
}

export function toItem(row: AttentionRow): AttentionItem {
  return {
    id: row.id,
    urgency: row.urgency,
    title: row.title,
    detail: row.detail || undefined,
    source: row.source,
    since: time(row.since),
    action: row.action && {
      label: row.action.label,
      view: VIEWS[row.action.screen],
    },
    skip: row.skip,
  };
}

/** The snapshot the bell draws, from the `data` of `toolportctl attention ls`. */
export function toSnapshot(data: AttentionData): AttentionSnapshot {
  const items = data.items.map(toItem);
  const unchecked = data.checks.filter((c) => !c.ok).map((c) => c.name);
  return {
    items,
    checkedAt: time(data.checkedAt) ?? null,
    hiddenCount: data.hiddenCount ?? 0,
    ...(unchecked.length > 0 && { unchecked }),
  };
}

/** Feeds the footer bell from `toolportctl attention ls`, now and every minute while the window
 * is visible. The bell stays out until the first answer arrives, so a build without
 * toolportctl shows no bell rather than a false all-clear. Returns the function that stops it. */
export function startAttentionFeed(pollMs = ATTENTION_POLL_MS): () => void {
  let snapshot: AttentionSnapshot | null = null;
  let stopped = false;
  let unregister: (() => void) | null = null;
  const listeners = new Set<() => void>();
  const feed: AttentionFeed = {
    snapshot: () => snapshot as AttentionSnapshot,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };

  const refresh = async () => {
    try {
      const next = toSnapshot(await ctlData<AttentionData>(["attention", "ls"]));
      if (stopped) return;
      snapshot = next;
      if (unregister) listeners.forEach((listener) => listener());
      else unregister = registerAttentionFeed(feed);
    } catch {
      // The last answer stays; a first failure keeps the bell out.
    }
  };
  const onVisible = () => {
    if (document.visibilityState === "visible") void refresh();
  };
  const onRefresh = () => void refresh();

  void refresh();
  const timer = setInterval(onVisible, pollMs);
  document.addEventListener("visibilitychange", onVisible);
  window.addEventListener(ATTENTION_REFRESH_EVENT, onRefresh);
  return () => {
    stopped = true;
    clearInterval(timer);
    document.removeEventListener("visibilitychange", onVisible);
    window.removeEventListener(ATTENTION_REFRESH_EVENT, onRefresh);
    unregister?.();
  };
}
