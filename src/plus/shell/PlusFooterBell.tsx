import { Bell } from "lucide-react";
import { Popover } from "radix-ui";
import { Badge } from "@/components/ui/badge";
import {
  ageLabel,
  needsYouCount,
  useAttentionSnapshot,
  type AttentionItem,
  type AttentionSnapshot,
} from "./attentionFeed";

const FOCUS_RING =
  "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

function Row({ item }: { item: AttentionItem }) {
  return (
    <li className="flex gap-2.5 border-t px-3.5 py-2.5 first:border-t-0">
      <span
        aria-hidden="true"
        className={`mt-1.5 size-1.5 shrink-0 rounded-full ${item.urgency === "needs-you" ? "bg-destructive" : "bg-warning"}`}
      />
      <div className="min-w-0">
        <p className="text-sm font-medium break-words">{item.title}</p>
        {item.detail && (
          <p className="text-meta break-words text-muted-foreground">{item.detail}</p>
        )}
        <p className="mt-1 flex items-center gap-2 text-xs text-muted-foreground">
          <Badge variant="secondary">{item.source}</Badge>
          {ageLabel(item.since)}
        </p>
      </div>
    </li>
  );
}

function Group({ title, items }: { title: string; items: AttentionItem[] }) {
  if (items.length === 0) return null;
  return (
    <section aria-label={title}>
      <h3 className="px-3.5 pt-3 pb-1 text-xs font-semibold tracking-[0.05em] text-muted-foreground uppercase">
        {title}
      </h3>
      <ul>
        {items.map((item) => (
          <Row key={item.id} item={item} />
        ))}
      </ul>
    </section>
  );
}

function summary(snapshot: AttentionSnapshot): string {
  const needs = needsYouCount(snapshot);
  const worth = snapshot.items.length - needs;
  if (needs > 0) return `Needs attention, ${needs}`;
  if (worth > 0) return `Needs attention, ${worth} worth a look`;
  return "Needs attention, nothing waiting";
}

/** The bell next to Help (design study, mockup 07). It is drawn only while an attention feed is
 * registered; the feed and the row actions belong to slice S9. */
export function PlusFooterBell() {
  const snapshot = useAttentionSnapshot();
  if (!snapshot) return null;
  const needs = needsYouCount(snapshot);
  const worth = snapshot.items.length - needs;
  const urgent = snapshot.items.filter((item) => item.urgency === "needs-you");
  const other = snapshot.items.filter((item) => item.urgency !== "needs-you");
  return (
    <Popover.Root>
      <Popover.Trigger asChild>
        <button
          aria-label={summary(snapshot)}
          title="Needs attention"
          className={`relative mr-1.5 ml-auto flex items-center rounded ${FOCUS_RING}`}
        >
          <Bell className="size-4" />
          {needs > 0 ? (
            <span
              aria-hidden="true"
              className="absolute -top-2 -right-2 grid min-w-4 place-items-center rounded-full bg-destructive px-1 text-[10px] leading-4 font-semibold text-primary-foreground"
            >
              {needs}
            </span>
          ) : worth > 0 ? (
            <span
              aria-hidden="true"
              className="absolute -top-0.5 -right-0.5 size-2 rounded-full bg-warning"
            />
          ) : null}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          side="top"
          align="start"
          collisionPadding={8}
          className="z-50 max-h-[calc(100dvh-2rem)] w-80 max-w-[calc(100vw-1rem)] overflow-y-auto rounded-lg border bg-popover text-popover-foreground shadow-lg"
        >
          <div className="flex items-center gap-2 px-3.5 py-3 text-sm font-semibold">
            <Bell className="size-4" /> Needs attention
          </div>
          <Group title="Needs you" items={urgent} />
          <Group title="Worth a look" items={other} />
          {snapshot.items.length === 0 && (
            <p className="border-t px-3.5 py-6 text-center text-meta text-muted-foreground">
              Nothing needs you
              {snapshot.checkedAt !== null &&
                `. Checked ${ageLabel(snapshot.checkedAt)}.`}
            </p>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
}
