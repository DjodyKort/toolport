import { useState } from "react";
import { ChevronRight, Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { cn } from "@/lib/utils";
import {
  CLAUDE_LABEL,
  filterItems,
  ITEM_STATE_LABEL,
  plural,
  TARGETS,
  type ItemFilter,
  type ItemState,
  type LibraryItem,
} from "./model";

const PAGE = 12;

const STATE_VARIANT: Record<
  ItemState,
  "info" | "warning" | "success" | "off" | "destructive"
> = {
  new: "info",
  changed: "warning",
  edited: "warning",
  differs: "warning",
  removed: "destructive",
  held: "destructive",
  synced: "success",
  unsynced: "off",
};

const SELECT =
  "h-8 rounded-md border bg-background px-2 text-sm text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none";

export function Targets({
  targeted,
  pending,
  onChange,
}: {
  targeted: readonly string[];
  pending: number;
  onChange: (clients: string[]) => void;
}) {
  return (
    <section aria-label="Targets" className="flex flex-col gap-2">
      <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
        Targets
      </h2>
      <ul className="divide-y rounded-xl border bg-card">
        {TARGETS.map((target) => {
          const on = targeted.includes(target.id);
          return (
            <li key={target.id} className="flex items-center gap-3 px-4 py-3">
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium">{target.label}</p>
                <p className="font-mono text-xs break-all text-muted-foreground">
                  {target.where}
                </p>
              </div>
              {on && pending > 0 && target.id === "claude-code" && (
                <Badge variant="warning">{pending} pending</Badge>
              )}
              <Switch
                checked={on}
                disabled={on && targeted.length === 1}
                aria-label={`Sync to ${target.label}`}
                onCheckedChange={(next) =>
                  onChange(
                    TARGETS.map((t) => t.id).filter((id) =>
                      id === target.id ? next : targeted.includes(id),
                    ),
                  )
                }
              />
            </li>
          );
        })}
      </ul>
      <p className="text-meta text-muted-foreground">
        Turning a client on shows its files in the plan first. Only Claude Code and
        AGENTS.md receive the library; the other clients were removed.
      </p>
    </section>
  );
}

export default function ItemsList({
  items,
  onOpen,
}: {
  items: LibraryItem[];
  onOpen: (name: string) => void;
}) {
  const [filter, setFilter] = useState<ItemFilter>({
    query: "",
    kind: "all",
    state: "any",
  });
  const [shown, setShown] = useState(PAGE);
  const list = filterItems(items, filter);
  const visible = list.slice(0, shown);
  return (
    <section aria-label="Library items" className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
          Library items{" "}
          <span className="ml-1 font-normal normal-case">{items.length}</span>
        </h2>
      </div>
      <div className="flex flex-wrap gap-2">
        <div className="relative min-w-40 flex-1">
          <Search
            aria-hidden
            className="pointer-events-none absolute top-2 left-2.5 size-4 text-muted-foreground"
          />
          <Input
            aria-label="Search library"
            placeholder="Search library"
            className="pl-8"
            value={filter.query}
            onChange={(event) => setFilter({ ...filter, query: event.target.value })}
          />
        </div>
        <select
          aria-label="Kind"
          className={SELECT}
          value={filter.kind}
          onChange={(event) =>
            setFilter({ ...filter, kind: event.target.value as ItemFilter["kind"] })
          }
        >
          <option value="all">All kinds</option>
          <option value="skill">Skills</option>
          <option value="rule">Rules</option>
        </select>
        <select
          aria-label="State"
          className={SELECT}
          value={filter.state}
          onChange={(event) =>
            setFilter({ ...filter, state: event.target.value as ItemFilter["state"] })
          }
        >
          <option value="any">Any state</option>
          <option value="waiting">Waiting for sync</option>
          {(Object.keys(ITEM_STATE_LABEL) as ItemState[]).map((state) => (
            <option key={state} value={state}>
              {ITEM_STATE_LABEL[state]}
            </option>
          ))}
        </select>
      </div>
      {list.length === 0 ? (
        <p className="rounded-xl border bg-card px-4 py-6 text-center text-meta text-muted-foreground">
          No library item matches.
        </p>
      ) : (
        <ul className="divide-y overflow-hidden rounded-xl border bg-card">
          {visible.map((item) => (
            <li key={item.name}>
              <button
                type="button"
                onClick={() => onOpen(item.name)}
                className="flex w-full items-center gap-3 px-4 py-2.5 text-left hover:bg-muted/40"
              >
                <span className="min-w-0 flex-1">
                  <span className="flex items-center gap-2 text-sm font-medium">
                    {item.name}
                    {item.kind === "rule" && <Badge variant="outline">Rule</Badge>}
                  </span>
                  <span className="block truncate text-meta text-muted-foreground">
                    {item.description}
                  </span>
                </span>
                {item.files !== null && (
                  <span className="hidden text-meta text-muted-foreground @md:inline">
                    {plural(item.files, "file")}
                  </span>
                )}
                {item.claude && (
                  <Badge variant={item.claude === "on" ? "outline" : "warning"}>
                    {CLAUDE_LABEL[item.claude]}
                  </Badge>
                )}
                <Badge variant={STATE_VARIANT[item.state]} className={cn("shrink-0")}>
                  {ITEM_STATE_LABEL[item.state]}
                </Badge>
                <ChevronRight
                  aria-hidden
                  className="size-4 shrink-0 text-muted-foreground"
                />
              </button>
            </li>
          ))}
        </ul>
      )}
      {list.length > shown && (
        <div className="flex items-center justify-between text-meta text-muted-foreground">
          <span>
            {shown} of {list.length}
          </span>
          <Button variant="link" size="sm" onClick={() => setShown((n) => n + 30)}>
            Show {list.length - shown} more
          </Button>
        </div>
      )}
      <p className="text-meta text-muted-foreground">
        Turning a skill on or off for Claude Code is not done here: that lives in Skills.
        “Claude: on/off” is read from there.
      </p>
    </section>
  );
}
