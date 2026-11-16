import { useEffect, useMemo, useRef, useState } from "react";
import { ArrowLeft, Copy, Play, Search } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { SectionHeader } from "@/components/ui/section-header";
import type { CommandsData, McpToolsData } from "../bridge/data";
import { RunDialog, type RunTarget } from "./RunDialog";
import {
  canRunCommand,
  commandArgv,
  commandLine,
  TIER_BADGE,
  toolEntries,
} from "./model";

function copy(text: string) {
  void navigator.clipboard.writeText(text).then(
    () => toast.success("Command copied"),
    () => toast.error("Could not copy the command"),
  );
}

/** Every toolportctl command and self-management tool the registry lists (D-058), for anything
 * without a page of its own. Read-only commands run here; the rest can be copied. */
export function AllCommands({
  commands,
  tools,
  onBack,
}: {
  commands: CommandsData;
  tools: McpToolsData | null;
  onBack: () => void;
}) {
  const [query, setQuery] = useState("");
  const [target, setTarget] = useState<RunTarget | null>(null);
  const top = useRef<HTMLDivElement>(null);
  // The page scrolls where the "Open" row was; start this one at its heading.
  useEffect(() => {
    top.current?.scrollIntoView?.({ block: "start" });
  }, []);
  const needle = query.trim().toLowerCase();

  const rows = useMemo(
    () =>
      commands.commands
        .filter((row) => row.kind === "command")
        .filter(
          (row) =>
            needle === "" ||
            row.id.toLowerCase().includes(needle) ||
            row.summary.toLowerCase().includes(needle),
        ),
    [commands, needle],
  );
  const toolRows = useMemo(
    () =>
      toolEntries(commands.tools, tools?.tools).filter(
        (tool) =>
          needle === "" ||
          tool.name.toLowerCase().includes(needle) ||
          tool.description.toLowerCase().includes(needle),
      ),
    [commands.tools, tools, needle],
  );

  return (
    <div ref={top} className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" onClick={onBack}>
          <ArrowLeft /> Toolport+
        </Button>
        <div className="min-w-48 flex-1">
          <h2 className="text-base font-semibold">All commands</h2>
          <p className="text-meta text-muted-foreground">
            Every toolportctl command and self-management tool, for anything without its
            own page.
          </p>
        </div>
        <div className="relative">
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground"
          />
          <Input
            aria-label="Search commands"
            placeholder="Search commands"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="h-8 w-48 pl-8"
          />
        </div>
      </div>

      <section aria-label="Commands">
        <SectionHeader count={rows.length}>Commands</SectionHeader>
        <div className="toolport-list">
          {rows.length === 0 ? (
            <EmptyState framed={false} title={`No command matches "${query}"`} />
          ) : (
            <ul>
              {rows.map((row) => {
                const tier = row.tier ? TIER_BADGE[row.tier] : null;
                return (
                  <li
                    key={row.id}
                    className="flex flex-wrap items-center gap-3 border-b border-border px-4 py-2.5 last:border-b-0"
                  >
                    <div className="min-w-56 flex-1">
                      <p className="font-mono text-sm font-semibold">{row.id}</p>
                      <p className="text-meta text-muted-foreground">{row.summary}</p>
                    </div>
                    {tier && <Badge variant={tier.variant}>{tier.text}</Badge>}
                    {canRunCommand(row) ? (
                      <Button
                        variant="outline"
                        size="sm"
                        aria-label={`Run ${row.id}`}
                        onClick={() =>
                          setTarget({ label: commandLine(row), argv: commandArgv(row) })
                        }
                      >
                        <Play /> Run
                      </Button>
                    ) : (
                      <Button
                        variant="ghost"
                        size="sm"
                        aria-label={`Copy ${row.id}`}
                        title="Changes things or needs input: copy it and run it in a terminal"
                        onClick={() => copy(commandLine(row))}
                      >
                        <Copy /> Copy
                      </Button>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </section>

      <section aria-label="Self-management tools">
        <SectionHeader count={toolRows.length}>Self-management tools</SectionHeader>
        <div className="toolport-list">
          {toolRows.length === 0 ? (
            <EmptyState framed={false} title={`No tool matches "${query}"`} />
          ) : (
            <ul>
              {toolRows.map((tool) => {
                const tier = TIER_BADGE[tool.tier];
                return (
                  <li
                    key={tool.name}
                    className="flex flex-wrap items-center gap-3 border-b border-border px-4 py-2.5 last:border-b-0"
                  >
                    <div className="min-w-56 flex-1">
                      <p className="font-mono text-sm font-semibold">{tool.name}</p>
                      <p className="text-meta text-muted-foreground">
                        {tool.description}
                      </p>
                    </div>
                    <Badge variant={tier.variant}>{tier.text}</Badge>
                    <Button
                      variant="outline"
                      size="sm"
                      aria-label={`Run ${tool.name}`}
                      onClick={() =>
                        setTarget({
                          label: `toolportctl mcp call ${tool.name}`,
                          argv: ["mcp", "call", tool.name, "--args-stdin"],
                          tool: {
                            name: tool.name,
                            gated: tool.gated,
                            previews: tool.previews,
                            hasParams: tool.params.length > 0,
                          },
                        })
                      }
                    >
                      <Play /> Run
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </section>

      <RunDialog target={target} onClose={() => setTarget(null)} />
    </div>
  );
}
