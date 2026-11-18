import { Ellipsis } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { AttentionItem } from "../shell/attentionFeed";
import { openView } from "../shell/navigate";
import { choicesFor, hideWith } from "./hideActions";

/** The ellipsis menu of one row: snooze, skip or dismiss, open its page, copy its id. */
export function RowMenu({
  item,
  onOpen,
}: {
  item: AttentionItem;
  /** Runs before the page opens, e.g. to close the popover. */
  onOpen?: () => void;
}) {
  const choices = choicesFor(item);
  const page = item.action?.view;
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          size="icon-xs"
          variant="ghost"
          aria-label={`Actions for ${item.title}`}
          className="shrink-0"
        >
          <Ellipsis />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-64">
        {choices.map((choice) => (
          <DropdownMenuItem
            key={choice.label}
            onSelect={() => void hideWith(item, choice)}
          >
            {choice.label}
          </DropdownMenuItem>
        ))}
        <DropdownMenuSeparator />
        {page && (
          <DropdownMenuItem
            onSelect={() => {
              onOpen?.();
              openView(page);
            }}
          >
            Open in{" "}
            {page === "servers" ? "Servers" : page === "tasks" ? "Tasks" : "Library"}
          </DropdownMenuItem>
        )}
        <DropdownMenuItem
          onSelect={() => void navigator.clipboard?.writeText(item.id)}
          className="text-muted-foreground"
        >
          Copy id: {item.id}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
