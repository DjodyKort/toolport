import { useState } from "react";
import { Plug } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { McpDoctorData, McpToolsData } from "../bridge/data";
import { Disclosure } from "./Disclosure";
import {
  accessSetsLine,
  checkLabel,
  plural,
  SELF_CHECK_LABELS,
  selfMcpView,
  TIER_BADGE,
} from "./model";

export interface AccessSetOption {
  id: string;
  name: string;
}

export interface SelfManagementProps {
  mcp: McpDoctorData;
  tools: McpToolsData | null;
  accessSets: AccessSetOption[];
  busy: boolean;
  onInstall: (accessSet: string | null) => void;
  onUninstall: () => void;
  onOpenServers: () => void;
}

function toolTier(tier: number) {
  return tier <= 1
    ? TIER_BADGE.read
    : tier >= 4
      ? TIER_BADGE.destructive
      : TIER_BADGE.write;
}

/** The self-management server: where it stands, connect and remove, the access sets that list it,
 * and the checks and tools behind the status. */
export function SelfManagement({
  mcp,
  tools,
  accessSets,
  busy,
  onInstall,
  onUninstall,
  onOpenServers,
}: SelfManagementProps) {
  const view = selfMcpView(mcp.state);
  const [extraSet, setExtraSet] = useState("");
  const registered = mcp.state !== "missing" && mcp.state !== "opted-out";
  const listable = accessSets.filter(
    (set) =>
      !(mcp.defaultAccess?.id === set.id && mcp.defaultAccess.member) &&
      !mcp.clientAccessSets.some((s) => s.id === set.id && s.member),
  );
  // Before it is registered the first checks fail by definition, so they are not counted.
  const failed = registered ? mcp.checks.filter((c) => !c.ok).length : 0;

  return (
    <div className="toolport-list">
      <div className="flex flex-wrap items-center gap-3 px-4 py-3">
        <div
          aria-hidden="true"
          className="grid size-8 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground [&_svg]:size-4"
        >
          <Plug />
        </div>
        <div className="min-w-48 flex-1">
          <p className="flex flex-wrap items-center gap-2 text-sm font-semibold">
            Self-management MCP
            <Badge variant={view.badge.variant}>{view.badge.text}</Badge>
          </p>
          <p className="text-meta text-muted-foreground">{view.summary}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {view.primary === "connect" && (
            <Button disabled={busy} onClick={() => onInstall(null)}>
              Connect
            </Button>
          )}
          {view.primary === "list" && (
            <Button disabled={busy} onClick={() => onInstall(null)}>
              List it in the default set
            </Button>
          )}
          {view.primary === "open-servers" && (
            <Button variant="outline" onClick={onOpenServers}>
              Open Servers
            </Button>
          )}
          {view.removable && (
            <ConfirmDialog
              destructive
              title="Remove self-management?"
              description="Agents can no longer list, check or change this setup. Toolport keeps it removed until you connect it again."
              confirmLabel="Remove"
              onConfirm={onUninstall}
              trigger={
                <Button variant="outline" disabled={busy}>
                  Remove
                </Button>
              }
            />
          )}
        </div>
      </div>

      {registered && (
        <div className="flex flex-wrap items-center gap-2 border-t border-border px-4 py-3">
          <p className="min-w-48 flex-1 text-meta text-muted-foreground">
            {accessSetsLine(mcp)}
          </p>
          {listable.length > 0 && (
            <>
              <Select value={extraSet} onValueChange={setExtraSet}>
                <SelectTrigger className="w-44" aria-label="Access set to list it in">
                  <SelectValue placeholder="Access set" />
                </SelectTrigger>
                <SelectContent>
                  {listable.map((set) => (
                    <SelectItem key={set.id} value={set.id}>
                      {set.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Button
                variant="outline"
                disabled={busy || extraSet === ""}
                onClick={() => {
                  onInstall(extraSet);
                  setExtraSet("");
                }}
              >
                List in set
              </Button>
            </>
          )}
        </div>
      )}

      <Disclosure
        label={
          failed > 0
            ? `Checks (${mcp.checks.length}, ${failed} failing)`
            : `Checks (${mcp.checks.length})`
        }
        defaultOpen={failed > 0 && registered}
      >
        <ul>
          {mcp.checks.map((check) => (
            <li
              key={check.name}
              className="flex flex-wrap items-center gap-x-3 gap-y-0.5 border-b border-border px-4 py-2 last:border-b-0"
            >
              <span className="w-full text-meta font-medium sm:w-56 sm:shrink-0">
                {checkLabel(check.name, SELF_CHECK_LABELS)}
              </span>
              <Badge variant={check.ok ? "success" : "warning"}>
                {check.ok ? "OK" : "Check"}
              </Badge>
              <span className="min-w-48 flex-1 font-mono text-xs break-all text-muted-foreground">
                {check.detail}
              </span>
            </li>
          ))}
        </ul>
      </Disclosure>

      {tools && (
        <Disclosure
          label={`Offers ${plural(tools.tools.length, "tool")} and ${plural(tools.resources.length, "resource")}`}
        >
          <ul>
            {tools.tools.map((tool) => (
              <li
                key={tool.name}
                className="flex flex-wrap items-center gap-x-3 gap-y-0.5 border-b border-border px-4 py-2"
              >
                <span className="w-full font-mono text-meta font-medium sm:w-56 sm:shrink-0">
                  {tool.name}
                </span>
                <Badge variant={toolTier(tool.tier).variant}>
                  {toolTier(tool.tier).text}
                </Badge>
                <span className="min-w-48 flex-1 text-meta text-muted-foreground">
                  {tool.description}
                </span>
              </li>
            ))}
            {tools.resources.map((resource) => (
              <li
                key={resource.uri}
                className="flex flex-wrap items-center gap-x-3 gap-y-0.5 border-b border-border px-4 py-2 last:border-b-0"
              >
                <span className="w-full font-mono text-meta font-medium sm:w-56 sm:shrink-0">
                  {resource.uri}
                </span>
                <Badge variant="off">Resource</Badge>
                <span className="min-w-48 flex-1 text-meta text-muted-foreground">
                  {resource.description}
                </span>
              </li>
            ))}
          </ul>
        </Disclosure>
      )}
    </div>
  );
}
