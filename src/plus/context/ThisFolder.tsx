import { useEffect, useState } from "react";
import { Gauge } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { StatusCard } from "@/components/ui/status-card";
import { Switch } from "@/components/ui/switch";
import { toastError } from "@/lib/toast";
import { errorText } from "../settings/useCtl";
import { useLoad } from "../servers/useLoad";
import {
  composeText,
  folderStatus,
  leaveOut,
  loadStack,
  measureFolder,
  switchPlugin,
} from "./commands";
import type { LoadItem } from "./data";
import { UseBundleDialog } from "./Bundles";
import Headline from "./Headline";
import MeasurePage from "./MeasurePage";
import {
  changesIn,
  headlineOf,
  homeOf,
  leversOf,
  plural,
  shortPath,
  splitStack,
  tokens,
} from "./model";
import { Group, InstructionRow, LayerStrip, LeverRow, RowShell } from "./StackRows";
import { changeWithUndo } from "./useWrites";

function ComposedText({ folder, onClose }: { folder: string; onClose: () => void }) {
  const composed = useLoad(() => composeText(folder), folder);
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Composed text</DialogTitle>
          <DialogDescription>
            The instruction text Claude gets in this folder, part by part in load order.
          </DialogDescription>
        </DialogHeader>
        {composed.error && <p role="alert">{composed.error}</p>}
        {!composed.data && !composed.error && <p>Reading the files…</p>}
        {composed.data?.parts.map((part, index) => (
          <section key={`${part.path}-${index}`} className="flex flex-col gap-1">
            <h4 className="text-sm font-semibold">
              {part.name}{" "}
              <span className="font-normal text-muted-foreground">
                ≈ {tokens(part.tokens)} tokens
              </span>
            </h4>
            <p className="break-all font-mono text-meta text-muted-foreground">
              {part.path}
            </p>
            <pre className="max-h-48 overflow-auto whitespace-pre-wrap rounded-md bg-muted p-2 text-xs">
              {part.text || "(empty)"}
            </pre>
          </section>
        ))}
      </DialogContent>
    </Dialog>
  );
}

export interface ThisFolderProps {
  folder: string;
  onOpenBundles: () => void;
  onHome: (home: string | null) => void;
}

export default function ThisFolder({ folder, onOpenBundles, onHome }: ThisFolderProps) {
  const status = useLoad(() => folderStatus(folder), folder);
  const stack = useLoad(() => loadStack(folder), folder);
  const lastRun = useLoad(() => measureFolder(folder), folder);
  const [busy, setBusy] = useState(false);
  const [drill, setDrill] = useState(false);
  const [composed, setComposed] = useState(false);
  const [pluginId, setPluginId] = useState("");
  const [using, setUsing] = useState(false);
  const homeNow = stack.data ? homeOf(stack.data.items) : null;
  useEffect(() => onHome(homeNow), [homeNow, onHome]);

  const reload = () => {
    status.reload();
    stack.reload();
    lastRun.reload();
  };

  if (drill) {
    return (
      <MeasurePage
        folder={folder}
        onBack={() => {
          setDrill(false);
          reload();
        }}
      />
    );
  }

  const error = status.error ?? stack.error;
  if (error && !status.data) {
    return (
      <StatusCard
        role="alert"
        variant="danger"
        icon={<Gauge />}
        title="This folder could not be read"
        detail={error}
        actions={
          <Button variant="outline" size="sm" onClick={reload}>
            Try again
          </Button>
        }
      />
    );
  }
  if (!status.data || !stack.data) {
    return <p className="text-meta text-muted-foreground">Reading {folder}…</p>;
  }

  const data = status.data;
  const loads = stack.data;
  const home = homeOf(loads.items);
  const parts = splitStack(loads.items);
  const instructionRows = loads.items.filter(
    (i) => i.kind !== "memory-index" && !i.lazy && (i.loaded || i.leftOutBy !== null),
  );
  const levers = leversOf(loads.items);
  const headline = headlineOf(data, loads);
  const known = lastRun.data?.ran ? lastRun.data : null;
  const pluginIds = [...new Set([...(known?.plugins ?? []), ...data.pluginsOff])];

  async function run<T>(task: () => Promise<T>): Promise<boolean> {
    setBusy(true);
    try {
      await task();
      return true;
    } finally {
      setBusy(false);
    }
  }

  const toggleFile = (item: LoadItem, on: boolean) =>
    run(() =>
      changeWithUndo(
        on ? `${item.name} loads here again` : `${item.name} is left out of this folder`,
        () => leaveOut(on ? "rm" : "add", { file: item.path! }, folder),
        () => leaveOut(on ? "add" : "rm", { file: item.path! }, folder),
        reload,
      ),
    );

  const toggleMemory = (on: boolean) =>
    run(() =>
      changeWithUndo(
        on ? "Auto memory is on again here" : "Auto memory is off in this folder",
        () => leaveOut(on ? "rm" : "add", { memory: true }, folder),
        () => leaveOut(on ? "add" : "rm", { memory: true }, folder),
        reload,
      ),
    );

  const togglePlugin = (id: string, on: boolean) =>
    run(() =>
      changeWithUndo(
        on ? `${id} is on again here` : `${id} is off in this folder`,
        () => switchPlugin(on ? "on" : "off", id, folder),
        () => switchPlugin(on ? "off" : "on", id, folder),
        reload,
      ),
    );

  async function addPlugin() {
    const id = pluginId.trim();
    if (!id) return;
    if (await togglePlugin(id, false)) setPluginId("");
  }

  const applied = data.bundle.applied;
  const savingAll = levers.reduce((sum, lever) => sum + lever.saving, 0);
  const memoryRow = parts.memory[0];

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border bg-card px-3 py-2 text-meta">
        <span className="text-muted-foreground">
          Applies to sessions started in this folder ·{" "}
          {plural(instructionRows.length, "instruction file")} read
        </span>
        <span className="flex items-center gap-2">
          Bundle here:{" "}
          {applied ? (
            <Badge variant={applied.drift ? "warning" : "partial"}>
              {applied.bundle}
              {applied.drift ? " · drift" : ""}
            </Badge>
          ) : (
            <span>none</span>
          )}
          <Button size="sm" variant="outline" onClick={() => setUsing(true)}>
            {applied ? "Stop using…" : "Apply a bundle…"}
          </Button>
        </span>
      </div>

      <Headline
        state={headline}
        status={data}
        estimate={loads.totalTokens}
        onMeasure={() => setDrill(true)}
      />

      <Group
        title="Biggest levers here"
        hint="Estimates. Measure to see the real saving."
        action={
          <Button size="sm" variant="ghost" onClick={onOpenBundles}>
            Save as a bundle…
          </Button>
        }
      >
        {levers.length === 0 ? (
          <RowShell
            title="Nothing to cut"
            subtitle="No instruction file is left to leave out."
          />
        ) : (
          levers.map((lever, index) => (
            <LeverRow
              key={lever.item.path}
              lever={lever}
              rank={index + 1}
              busy={busy}
              home={home}
              onLeaveOut={(item) => void toggleFile(item, false)}
            />
          ))
        )}
        <p className="px-3 py-2 text-meta text-muted-foreground">
          {levers.length > 1 && (
            <>
              All {levers.length}: about −{tokens(savingAll)} (estimate).{" "}
            </>
          )}
          {loads.notItemised}.
        </p>
      </Group>

      <Group
        title="Instruction files"
        count={instructionRows.length}
        hint="Switch off to leave out here"
        action={
          <Button size="sm" variant="ghost" onClick={() => setComposed(true)}>
            Show composed text
          </Button>
        }
      >
        <div className="px-3 py-2">
          <LayerStrip items={loads.items} cwd={data.cwd} />
        </div>
        {instructionRows.length === 0 && (
          <RowShell
            title="No instruction files"
            subtitle="Nothing is read from this folder or above it."
          />
        )}
        {instructionRows.map((item, index) => (
          <InstructionRow
            key={`${item.path}-${index}`}
            item={item}
            cwd={data.cwd}
            home={home}
            verified={loads.verified}
            busy={busy}
            onToggle={(row, on) => void toggleFile(row, on)}
          />
        ))}
      </Group>
      {loads.verified && loads.verified.unknown.length > 0 && (
        <StatusCard
          variant="warning"
          icon={<Gauge />}
          title="Claude Code loaded files this page has no row for"
          detail={loads.verified.unknown.map((p) => shortPath(p, home)).join(", ")}
        />
      )}

      {(memoryRow || data.leftOut.memoryOff) && (
        <Group title="Memory" count={1}>
          <RowShell
            title="Auto memory index"
            subtitle={
              memoryRow
                ? `MEMORY.md · ${shortPath(memoryRow.path ?? "", home)}`
                : "MEMORY.md"
            }
            trailing={
              <>
                <span className="text-sm tabular-nums">
                  ≈ {memoryRow ? tokens(memoryRow.tokens) : 0}
                </span>
                <Switch
                  aria-label="Auto memory loads here"
                  checked={!data.leftOut.memoryOff}
                  disabled={busy}
                  onCheckedChange={(on) => void toggleMemory(on)}
                />
              </>
            }
          />
        </Group>
      )}

      <Group
        title="Plugins"
        count={pluginIds.length}
        hint="One switch per plugin, for this folder only"
      >
        {pluginIds.length === 0 && (
          <RowShell
            title="No plugin is known here yet"
            subtitle="Measure for real lists the plugins Claude Code loaded in this folder. A plugin can also be named below."
          />
        )}
        {pluginIds.map((id) => {
          const off = data.pluginsOff.includes(id);
          return (
            <RowShell
              key={id}
              title={id}
              badge={off ? <Badge variant="partial">Off here</Badge> : undefined}
              subtitle="Its cost is part of Not itemised. Measure without it to see the real saving."
              trailing={
                <Switch
                  aria-label={`${id} loads here`}
                  checked={!off}
                  disabled={busy}
                  onCheckedChange={(on) => void togglePlugin(id, on)}
                />
              }
            />
          );
        })}
        <form
          className="flex gap-2 px-3 py-2.5"
          onSubmit={(event) => {
            event.preventDefault();
            void addPlugin();
          }}
        >
          <Input
            aria-label="Plugin id"
            placeholder="name@marketplace"
            value={pluginId}
            onChange={(e) => setPluginId(e.target.value)}
          />
          <Button type="submit" variant="outline" disabled={busy || !pluginId.trim()}>
            Turn off here
          </Button>
        </form>
      </Group>

      <Group
        title="Skills, agents and tools"
        hint="Not itemised until the Library scan lands (slice S4)"
      >
        <RowShell
          title="Skills, commands and agents"
          subtitle="Skills cost their descriptions only. Library skills are delivered by Library, and on and off for every folder lives on the Skills page."
        />
        <RowShell
          title="Tools (MCP)"
          subtitle={
            known?.mcpServers?.length
              ? `In the last measurement: ${known.mcpServers.join(", ")}. A direct entry skips the gateway and loads every tool definition; a bundle blocks one with mcp.deny.`
              : "Measure for real lists the servers Claude Code connected."
          }
        />
      </Group>

      {parts.lazy.length > 0 && (
        <Group
          title="Loads on demand"
          count={parts.lazy.length}
          hint={`≈ ${tokens(parts.lazy.reduce((sum, i) => sum + i.tokens, 0))} tokens, not counted above`}
        >
          {parts.lazy.map((item, index) => (
            <RowShell
              key={`${item.path}-${index}`}
              title={shortPath(item.path ?? item.name, home)}
              subtitle={item.reason || "loads when Claude reads matching files"}
              trailing={
                <span className="text-sm tabular-nums">≈ {tokens(item.tokens)}</span>
              }
            />
          ))}
        </Group>
      )}
      {parts.notHere.length > 0 && (
        <Group title="Not loaded here" count={parts.notHere.length}>
          {parts.notHere.map((item, index) => (
            <RowShell
              key={`${item.path}-${index}`}
              title={item.name}
              subtitle={item.reason}
              badge={<Badge variant="off">not here</Badge>}
            />
          ))}
        </Group>
      )}

      <Group title="What Toolport changed here" count={changesIn(data)}>
        {changesIn(data) === 0 && (
          <RowShell
            title="Toolport has written nothing in this folder"
            subtitle="Leave-outs and bundles add entries to .claude/settings.local.json; each one is listed here and Undo puts it back."
          />
        )}
        {applied && (
          <RowShell
            title={`Bundle ${applied.bundle}`}
            badge={
              <Badge variant={applied.drift ? "warning" : "success"}>
                {applied.drift ? "drift" : "no drift"}
              </Badge>
            }
            subtitle={`${plural(Object.values(applied.ownedKeys).flat().length, "key")} in .claude/settings.local.json${applied.changedKeys.length ? ` · changed since: ${applied.changedKeys.join(", ")}` : ""}`}
            trailing={
              <Button size="sm" variant="outline" onClick={onOpenBundles}>
                Open
              </Button>
            }
          />
        )}
        {data.leftOut.files.map((file) => (
          <RowShell
            key={file}
            title={`Left out: ${shortPath(file, home)}`}
            subtitle="claudeMdExcludes"
            trailing={
              <Button
                size="sm"
                variant="outline"
                disabled={busy}
                onClick={() =>
                  void run(async () => {
                    try {
                      await leaveOut("rm", { file }, folder);
                      reload();
                    } catch (e) {
                      toastError("Could not undo", { description: errorText(e) });
                    }
                  })
                }
              >
                Undo
              </Button>
            }
          />
        ))}
        {data.leftOut.memoryOff && (
          <RowShell
            title="Auto memory off"
            subtitle="autoMemoryEnabled"
            trailing={
              <Button
                size="sm"
                variant="outline"
                disabled={busy}
                onClick={() => void toggleMemory(true)}
              >
                Undo
              </Button>
            }
          />
        )}
        {data.pluginsOff.map((id) => (
          <RowShell
            key={id}
            title={`Plugin off: ${id}`}
            subtitle={`enabledPlugins.${id}`}
            trailing={
              <Button
                size="sm"
                variant="outline"
                disabled={busy}
                onClick={() => void togglePlugin(id, true)}
              >
                Undo
              </Button>
            }
          />
        ))}
      </Group>

      {using && (
        <UseBundleDialog
          folder={folder}
          applied={applied?.bundle ?? null}
          onClose={() => setUsing(false)}
          onDone={reload}
        />
      )}
      {composed && <ComposedText folder={folder} onClose={() => setComposed(false)} />}
    </div>
  );
}
