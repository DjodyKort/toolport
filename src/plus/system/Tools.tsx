import { useState } from "react";
import { Search } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Mono, Tag } from "./atoms";
import {
  GATE_LABEL,
  TIER_LABEL,
  TIER_TONE,
  filterTools,
  plural,
  type ToolEntry,
} from "./model";

/** A tool catalogue: name, what it does, its tier and the confirmation it needs. */
export function ToolList({
  tools,
  label,
  filterable = false,
}: {
  tools: ToolEntry[];
  label: string;
  filterable?: boolean;
}) {
  const [text, setText] = useState("");
  const [tier, setTier] = useState<number | null>(null);
  const shown = filterTools(tools, text, tier);
  const tiers = [...new Set(tools.map((tool) => tool.tier))].sort();
  return (
    <div className="flex flex-col gap-2">
      {filterable && (
        <div className="flex flex-wrap items-center gap-2">
          <div className="relative min-w-48 flex-1">
            <Search
              className="pointer-events-none absolute top-2 left-2 size-4 text-muted-foreground"
              aria-hidden="true"
            />
            <Input
              aria-label="Filter tools"
              placeholder="Filter by name or description"
              value={text}
              onChange={(event) => setText(event.target.value)}
              className="pl-8"
            />
          </div>
          <select
            aria-label="Tier"
            value={tier ?? ""}
            onChange={(event) =>
              setTier(event.target.value === "" ? null : Number(event.target.value))
            }
            className="h-8 rounded-lg border bg-background px-2 text-sm"
          >
            <option value="">All tiers</option>
            {tiers.map((value) => (
              <option key={value} value={value}>
                {TIER_LABEL[value] ?? `Tier ${value}`}
              </option>
            ))}
          </select>
          <span className="text-xs text-muted-foreground" role="status">
            {plural(shown.length, "tool")}
          </span>
        </div>
      )}
      {shown.length === 0 ? (
        <p className="text-sm text-muted-foreground">No tool matches the filter.</p>
      ) : (
        <ul aria-label={label} className="flex max-h-96 flex-col divide-y overflow-auto">
          {shown.map((tool) => (
            <li
              key={tool.name}
              className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-3 gap-y-0.5 py-1.5"
            >
              <div className="min-w-0">
                <Mono>{tool.name}</Mono>
                <p className="text-xs text-muted-foreground">{tool.description}</p>
                {tool.gate && (
                  <p className="text-xs text-muted-foreground">
                    {GATE_LABEL[tool.gate] ?? tool.gate}
                  </p>
                )}
              </div>
              <Tag tone={TIER_TONE[tool.tier] ?? "secondary"}>
                {TIER_LABEL[tool.tier] ?? `Tier ${tool.tier}`}
              </Tag>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
