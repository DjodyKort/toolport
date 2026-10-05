import { useState, type ReactNode } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { EmptyState } from "@/components/ui/empty-state";
import type { HookEntry, HooksLsData } from "../types/plugins";
import { AsyncView } from "../ui";
import { FolderField, useRecentFolders, type FolderChoice } from "../context/folder";
import { useRead } from "../skills/hooks";
import { plural } from "../skills/model";
import { homeShort } from "../plugins/model";
import {
  around,
  byOwner,
  hooksArgs,
  otherByOwner,
  otherTotal,
  ownerLabel,
  processesText,
  SWITCH_LABEL,
  switchText,
  TOOLS,
  type Tool,
} from "./model";

function Stat({ label, value, note }: { label: string; value: ReactNode; note: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5 rounded-lg border px-3 py-2">
      <small className="text-xs text-muted-foreground">{label}</small>
      <b className="text-base tabular-nums">{value}</b>
      <small className="text-xs text-muted-foreground">{note}</small>
    </div>
  );
}

function HookRow({ hook }: { hook: HookEntry }) {
  return (
    <li className="flex flex-wrap items-center gap-2 px-3 py-2 text-sm">
      <span className="flex min-w-0 flex-1 flex-col">
        <b className="break-all">{hook.runtimeId ?? hook.command}</b>
        <small className="break-all text-muted-foreground">
          {hook.runtimeId ? `${hook.command} · ` : ""}matcher {hook.matcher || "(all)"}
        </small>
        <small className="text-muted-foreground">{homeShort(hook.source)}</small>
      </span>
      <Badge variant="outline">{ownerLabel(hook)}</Badge>
      <Badge variant={hook.async ? "secondary" : "warning"}>
        {hook.async ? "background" : "waits"}
      </Badge>
      <Badge variant="info" title={hook.switch.detail}>
        {SWITCH_LABEL[hook.switch.method] ?? hook.switch.method}
      </Badge>
      {!hook.active && <Badge variant="secondary">off here</Badge>}
    </li>
  );
}

function HookList({ title, hooks }: { title: string; hooks: HookEntry[] }) {
  return (
    <section aria-label={title} className="flex flex-col gap-2">
      <h3 className="flex items-center gap-2 text-sm font-semibold">
        {title}
        <span className="rounded-full bg-secondary px-2 py-px text-2xs font-semibold text-muted-foreground tabular-nums">
          {hooks.length}
        </span>
      </h3>
      {hooks.length === 0 ? (
        <p className="text-sm text-muted-foreground">No hook fires here.</p>
      ) : (
        <ul
          aria-label={`${title} list`}
          className="flex flex-col divide-y rounded-lg border"
        >
          {hooks.map((hook, index) => (
            <HookRow key={`${hook.source}:${index}`} hook={hook} />
          ))}
        </ul>
      )}
    </section>
  );
}

function Report({ data, tool }: { data: HooksLsData; tool: Tool }) {
  const { before, after } = around(data, tool);
  const all = [...before, ...after];
  const conflicts = data.conflicts.filter((conflict) => conflict.tools.includes(tool));
  const pluginHooks = all.filter((hook) => hook.owner.kind === "plugin").length;
  return (
    <div className="flex flex-col gap-4">
      {data.disabledAll && (
        <Callout variant="info" role="status">
          disableAllHooks is set for this folder: every hook is off except the managed
          ones.
        </Callout>
      )}
      {data.warnings.map((warning) => (
        <Callout key={warning} variant="warning" role="status">
          {warning}
        </Callout>
      ))}
      <p className="text-xs text-muted-foreground">
        Processes are counted from the matchers, not timed. Toolport reads the settings
        and hook files and never runs a hook.
      </p>
      <div
        role="group"
        aria-label="Hook counts"
        className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4"
      >
        <Stat
          label={`Processes for one ${tool} call`}
          value={all.length}
          note={byOwner(all)}
        />
        <Stat
          label="Per answer, besides tool calls"
          value={`${otherTotal(data)} more`}
          note={`at session start and end, on Stop and around compaction: ${otherByOwner(data)}`}
        />
        <Stat
          label="Run order"
          value="all at once"
          note="Claude Code starts every matching hook in parallel; if two disagree, deny beats defer, ask, then allow"
        />
        <Stat
          label="Can Toolport switch them"
          value={
            all.some((h) => h.switch.method === "plugin-setting")
              ? "some"
              : "not per hook"
          }
          note={`${switchText(all)}; Claude Code has no per-hook switch`}
        />
      </div>
      <HookList title={`Before ${tool} runs`} hooks={before} />
      <HookList title={`After ${tool} ran`} hooks={after} />
      {conflicts.map((conflict) => (
        <Callout key={conflict.hooks.join("|")} variant="info" role="status">
          <b>Hooks of different owners watch the same {tool} call.</b>{" "}
          {conflict.hooks.join(", ")}. {conflict.note}.
        </Callout>
      ))}
      {pluginHooks > 0 && (
        <Callout variant="warning" role="note">
          <b>Every entry is its own process.</b> The {plural(pluginHooks, "plugin hook")}{" "}
          start for each {tool} call, even those that do nothing for this tool. A hook
          switched off through its plugin&apos;s settings still starts a process; turning
          the plugin off in the folder removes all {pluginHooks}.
        </Callout>
      )}
      <p className="text-xs text-muted-foreground">
        Your own hooks are shown and never changed here. Hooks that Toolport or a skill
        installed are marked, with the command that changes them.
      </p>
    </div>
  );
}

/** Context > Hooks: which hooks run around a tool in a folder, who owns each and how that owner
 * can switch it. Read only. */
export function HooksTab({
  here,
  onOpenPlugins,
}: {
  here: FolderChoice;
  onOpenPlugins?: () => void;
}) {
  const { recent, remember } = useRecentFolders();
  const [tool, setTool] = useState<Tool>("Bash");
  const query = useRead<HooksLsData>(hooksArgs(here.folder));
  return (
    <div role="group" aria-label="Hooks" className="flex flex-col gap-4">
      <div className="flex flex-wrap items-end gap-3">
        <FolderField
          label="Folder"
          value={here.draft}
          onChange={here.setDraft}
          options={recent}
          placeholder="/path/to/the/folder where Claude Code starts"
          submitLabel="Show"
          onSubmit={() => {
            here.show(here.draft);
            remember(here.draft);
          }}
        />
        <div role="group" aria-label="Tool" className="flex gap-1">
          {TOOLS.map((name) => (
            <Button
              key={name}
              size="sm"
              variant={name === tool ? "default" : "outline"}
              aria-pressed={name === tool}
              onClick={() => setTool(name)}
            >
              {name}
            </Button>
          ))}
        </div>
        <Badge variant="info" title="Other clients have their own hook systems">
          Claude Code only
        </Badge>
        {onOpenPlugins && (
          <Button size="sm" variant="outline" onClick={onOpenPlugins}>
            Plugins
          </Button>
        )}
      </div>
      <p className="text-xs text-muted-foreground">
        {here.folder
          ? `Folder ${homeShort(here.folder)}`
          : "No folder chosen: showing your own settings and plugins"}
        . Pick a tool to see which hooks run around it.
      </p>
      <AsyncView
        query={query}
        errorTitle="Couldn't read the hooks"
        context="hooks ls"
        isEmpty={(data) => data.hooks.length === 0 && !data.disabledAll}
        empty={
          <EmptyState
            title="No hooks"
            description="No settings file or plugin defines a hook for this folder."
          />
        }
      >
        {(data) => <Report data={data} tool={tool} />}
      </AsyncView>
      <span className="sr-only" aria-live="polite">
        {query.data ? processesText(around(query.data, tool).before.length) : ""}
      </span>
    </div>
  );
}
