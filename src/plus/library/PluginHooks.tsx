import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useLoad } from "../servers/useLoad";
import { listHooks } from "./pluginsCommands";
import type { HookEntry } from "./pluginsData";
import { ownerText } from "./pluginsModel";

export function HookRow({ hook }: { hook: HookEntry }) {
  return (
    <li className="flex flex-col gap-1 px-4 py-2.5">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="outline">{hook.event}</Badge>
        {hook.matcher && <span className="font-mono text-xs">{hook.matcher}</span>}
        <span className="text-meta text-muted-foreground">{ownerText(hook)}</span>
        {hook.async && <Badge variant="off">async</Badge>}
      </div>
      <code className="font-mono text-xs break-all text-muted-foreground">
        {hook.command}
      </code>
      <p className="text-meta text-muted-foreground">
        {hook.switch.method === "none" ? "No switch." : hook.switch.detail}
      </p>
    </li>
  );
}

const TOOLS = ["Bash", "Edit", "Write", "Read"] as const;
const OWNERS: Array<{ id: string; label: string; kinds: string[] }> = [
  { id: "toolport", label: "Toolport", kinds: ["toolport"] },
  { id: "you", label: "you", kinds: ["user", "project", "local"] },
  { id: "managed", label: "administrator", kinds: ["managed"] },
  { id: "skill", label: "skills", kinds: ["skill"] },
  { id: "plugin", label: "plugins", kinds: ["plugin"] },
];
const SHOWN = 7;

/** Every hook Claude Code would start in the folder, read from files; none is run. Filtered by
 * the tool a call is for and by who owns the hook. `bare` leaves the heading to the caller. */
export function HooksInventory({
  folder,
  bare = false,
}: {
  folder: string | null;
  bare?: boolean;
}) {
  const hooks = useLoad(() => listHooks(folder), `hooks|${folder}`);
  const [tool, setTool] = useState<string | null>(null);
  const [owner, setOwner] = useState<string | null>(null);
  const [all, setAll] = useState(false);
  const data = hooks.data;
  const byTool = (data?.hooks ?? []).filter(
    (h) => tool === null || h.tools.includes(tool),
  );
  const counted = (kinds: string[]) =>
    byTool.filter((h) => kinds.includes(h.owner.kind)).length;
  const picked = OWNERS.find((o) => o.id === owner);
  const rows = byTool.filter((h) => !picked || picked.kinds.includes(h.owner.kind));
  const shown = all ? rows : rows.slice(0, SHOWN);
  const pre = tool && data ? (data.counts.perTool[tool]?.pre ?? 0) : null;
  return (
    <section
      aria-label={bare ? "Hook inventory" : "Hooks"}
      className="flex flex-col gap-2"
    >
      {!bare && (
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
            Hooks
            {data && (
              <span className="ml-1 font-normal normal-case">{data.hooks.length}</span>
            )}
          </h2>
          <span className="text-meta text-muted-foreground">
            What Claude Code would start, read from settings, plugins and skills. Nothing
            is run.
          </span>
        </div>
      )}
      {hooks.error && !data && (
        <p role="alert" className="text-meta text-destructive">
          {hooks.error}
        </p>
      )}
      {!data && !hooks.error && (
        <p className="text-meta text-muted-foreground">Reading the hooks…</p>
      )}
      {data?.disabledAll && (
        <p className="rounded-xl border bg-card px-4 py-3 text-meta text-warning">
          disableAllHooks is on: no hook below runs.
        </p>
      )}
      {data?.managedOnly && (
        <p className="rounded-xl border bg-card px-4 py-3 text-meta text-muted-foreground">
          Your administrator allows managed hooks only: hooks of other owners do not run.
        </p>
      )}
      {data && data.hooks.length === 0 && (
        <p className="rounded-xl border bg-card px-4 py-4 text-meta text-muted-foreground">
          No hooks.
        </p>
      )}
      {data && data.hooks.length > 0 && (
        <>
          <div role="group" aria-label="Tool" className="flex flex-wrap gap-1.5">
            {[null, ...TOOLS].map((name) => (
              <Button
                key={name ?? "all"}
                size="sm"
                variant={tool === name ? "secondary" : "outline"}
                aria-pressed={tool === name}
                onClick={() => {
                  setTool(name);
                  setAll(false);
                }}
              >
                {name ?? "All tools"}
              </Button>
            ))}
          </div>
          <p className="text-meta text-muted-foreground">
            {tool
              ? `${pre ?? 0} ${pre === 1 ? "process" : "processes"} start before one ${tool} call, all at once. `
              : ""}
            If two disagree: deny beats defer, ask, then allow. Toolport reads settings
            and hook files; it never runs a hook.
          </p>
          <div role="group" aria-label="Owner" className="flex flex-wrap gap-1.5">
            <Button
              size="sm"
              variant={owner === null ? "secondary" : "outline"}
              aria-pressed={owner === null}
              onClick={() => setOwner(null)}
            >
              All {byTool.length}
            </Button>
            {OWNERS.filter((o) => counted(o.kinds) > 0).map((o) => (
              <Button
                key={o.id}
                size="sm"
                variant={owner === o.id ? "secondary" : "outline"}
                aria-pressed={owner === o.id}
                onClick={() => {
                  setOwner(o.id);
                  setAll(false);
                }}
              >
                {o.label} {counted(o.kinds)}
              </Button>
            ))}
          </div>
          <ul
            aria-label="Processes per tool"
            className="flex flex-wrap gap-2 text-meta text-muted-foreground"
          >
            {Object.entries(data.counts.perTool).map(([name, count]) => (
              <li key={name}>
                <Badge variant="outline">
                  {name}: {count.total} {count.total === 1 ? "process" : "processes"} (
                  {count.pre} before, {count.post} after)
                </Badge>
              </li>
            ))}
          </ul>
          {data.conflicts.map((conflict) => (
            <p
              key={conflict.tools.join()}
              className="rounded-xl border bg-card px-4 py-3 text-meta text-warning"
            >
              {conflict.tools.join(", ")}: {conflict.hooks.join(", ")} all answer before
              the tool runs; {conflict.note}.
            </p>
          ))}
          <ul className="divide-y overflow-hidden rounded-xl border bg-card">
            {shown.map((hook, index) => (
              <HookRow key={`${hook.source}-${hook.event}-${index}`} hook={hook} />
            ))}
          </ul>
          {rows.length > SHOWN && (
            <Button
              variant="ghost"
              size="sm"
              className="self-start"
              onClick={() => setAll((value) => !value)}
            >
              {all ? "Show fewer" : `Show ${rows.length - SHOWN} more`}
            </Button>
          )}
        </>
      )}
    </section>
  );
}

export const HooksSection = HooksInventory;
