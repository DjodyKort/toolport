import { useSyncExternalStore } from "react";

export type AttentionUrgency = "needs-you" | "worth-a-look";

export interface AttentionItem {
  id: string;
  urgency: AttentionUrgency;
  title: string;
  detail?: string;
  /** The page the condition comes from, shown as a chip: Servers, Tasks, Library. */
  source: string;
  /** Epoch milliseconds. */
  since?: number;
}

export interface AttentionSnapshot {
  items: AttentionItem[];
  checkedAt: number | null;
}

/** Where the footer bell gets its rows. Slice S9 supplies the real feed; until a feed is
 * registered the bell is not drawn. */
export interface AttentionFeed {
  /** Must return the same object until the feed notifies its subscribers. */
  snapshot(): AttentionSnapshot;
  subscribe(listener: () => void): () => void;
}

let feed: AttentionFeed | null = null;
let stopFeed: (() => void) | null = null;
const listeners = new Set<() => void>();

function notify() {
  listeners.forEach((listener) => listener());
}

/** Registers the feed, or removes it with `null`. Returns a function that removes this feed. */
export function registerAttentionFeed(next: AttentionFeed | null): () => void {
  stopFeed?.();
  feed = next;
  stopFeed = next?.subscribe(notify) ?? null;
  notify();
  return () => {
    if (feed === next) registerAttentionFeed(null);
  };
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function getSnapshot(): AttentionSnapshot | null {
  return feed?.snapshot() ?? null;
}

/** The current snapshot, or `null` when no feed exists. */
export function useAttentionSnapshot(): AttentionSnapshot | null {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

export function needsYouCount(snapshot: AttentionSnapshot): number {
  return snapshot.items.filter((item) => item.urgency === "needs-you").length;
}

export function ageLabel(since: number | undefined, now = Date.now()): string {
  if (since === undefined) return "";
  const minutes = Math.max(0, Math.round((now - since) / 60_000));
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  return `${Math.round(hours / 24)} d ago`;
}
