import { useState, type ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { SkeletonRows } from "@/components/ui/skeleton";
import PlanDialog from "../context/PlanDialog";
import { switchPlugin } from "../context/commands";
import { useLoad } from "../servers/useLoad";
import { HookRow } from "./PluginHooks";
import { PluginSettings } from "./PluginSettings";
import { switchMcp, showPlugin } from "./pluginsCommands";
import type { McpServerRow, PluginShowData } from "./pluginsData";
import {
  bringsText,
  deniedAnywhere,
  deniedText,
  layerRows,
  stateText,
  tokenText,
  updateText,
} from "./pluginsModel";

const SHOWN = 12;

function Section({
  title,
  count,
  children,
}: {
  title: string;
  count?: number;
  children: ReactNode;
}) {
  return (
    <section aria-label={title} className="flex flex-col gap-2">
      <h2 className="text-meta font-semibold tracking-wide text-muted-foreground uppercase">
        {title}
        {count !== undefined && (
          <span className="ml-1 font-normal normal-case">{count}</span>
        )}
      </h2>
      {children}
    </section>
  );
}

function Names({ title, names }: { title: string; names: string[] }) {
  const [all, setAll] = useState(false);
  if (names.length === 0) return null;
  const shown = all ? names : names.slice(0, SHOWN);
  return (
    <div className="flex flex-col gap-1.5 px-4 py-3">
      <p className="text-sm font-medium">
        {title} <span className="font-normal text-muted-foreground">{names.length}</span>
      </p>
      <ul className="flex flex-wrap gap-1.5" aria-label={title}>
        {shown.map((name) => (
          <li key={name}>
            <Badge variant="outline" className="font-mono">
              {name}
            </Badge>
          </li>
        ))}
      </ul>
      {names.length > SHOWN && (
        <Button
          variant="ghost"
          size="sm"
          className="self-start"
          onClick={() => setAll((value) => !value)}
        >
          {all ? "Show fewer" : `Show all ${names.length}`}
        </Button>
      )}
    </div>
  );
}

type Dialog =
  | { kind: "switch"; off: boolean }
  | { kind: "mcp"; action: "deny" | "allow"; server: McpServerRow }
  | null;

function McpRow({
  server,
  folder,
  onSwitch,
}: {
  server: McpServerRow;
  folder: string | null;
  onSwitch: (action: "deny" | "allow") => void;
}) {
  const denied = deniedAnywhere(server.denied);
  const note = deniedText(server.denied);
  return (
    <li className="flex flex-wrap items-center gap-3 px-4 py-3">
      <div className="min-w-48 flex-1">
        <p className="text-sm font-medium">{server.name}</p>
        <p className="font-mono text-xs break-all text-muted-foreground">
          {server.command ? server.command.join(" ") : (server.url ?? "no start command")}
        </p>
        <p className="text-meta text-muted-foreground">
          Tools are named <span className="font-mono text-xs">{server.toolPrefix}</span>
        </p>
      </div>
      {note && <Badge variant="warning">{note}</Badge>}
      {!denied && <Badge variant="success">starts</Badge>}
      <Button
        variant="outline"
        size="sm"
        disabled={!folder}
        title={folder ? undefined : "Choose a folder first"}
        aria-label={`${server.denied.local ? "Allow" : "Deny"} ${server.name} in this folder`}
        onClick={() => onSwitch(server.denied.local ? "allow" : "deny")}
      >
        {server.denied.local ? "Allow in this folder" : "Deny in this folder"}
      </Button>
    </li>
  );
}

function Detail({
  plugin,
  folder,
  onChanged,
  onChooseItems,
}: {
  plugin: PluginShowData;
  folder: string | null;
  onChanged: () => void;
  onChooseItems?: (source: string) => void;
}) {
  const [dialog, setDialog] = useState<Dialog>(null);
  const { enabled } = plugin;
  const lockedByAdmin = enabled.managed !== null;
  const offHere = enabled.local === false;
  const cost = tokenText(plugin.cost.measured ?? plugin.cost.projected);
  const items = plugin.components;
  return (
    <>
      <div className="flex flex-wrap items-center gap-2">
        <h1 className="text-lg font-semibold">{plugin.name}</h1>
        <span className="text-sm text-muted-foreground">{plugin.version}</span>
        {plugin.marketplace && <Badge variant="outline">@{plugin.marketplace}</Badge>}
        <Badge variant={enabled.effective ? "success" : "off"}>
          {enabled.effective ? "On" : "Off"}
        </Badge>
        {plugin.update.state !== "unknown" && (
          <Badge variant={plugin.update.state === "update" ? "warning" : "off"}>
            {updateText(plugin)}
          </Badge>
        )}
      </div>
      {plugin.description && <p className="text-sm">{plugin.description}</p>}
      {plugin.update.state === "update" && (
        <p className="text-meta text-muted-foreground">
          Update with{" "}
          <code className="font-mono text-xs">claude plugin update {plugin.id}</code>;
          Claude Code applies it in the next session.
        </p>
      )}

      <Section title="Where it is on">
        <ul className="divide-y overflow-hidden rounded-xl border bg-card">
          {layerRows(enabled).map((row) => (
            <li
              key={row.layer}
              className="flex items-center justify-between gap-3 px-4 py-2.5"
            >
              <span className="text-sm">{row.layer}</span>
              <Badge
                variant={row.state === null ? "off" : row.state ? "success" : "warning"}
              >
                {stateText(row.state)}
              </Badge>
            </li>
          ))}
        </ul>
        <div className="flex flex-wrap items-center gap-2">
          {lockedByAdmin ? (
            <p className="text-meta text-muted-foreground">
              Your administrator decides this plugin; it cannot be switched here.
            </p>
          ) : (
            <Button
              variant="outline"
              size="sm"
              disabled={!folder}
              title={folder ? undefined : "Choose a folder first"}
              onClick={() => setDialog({ kind: "switch", off: !offHere })}
            >
              {offHere ? "Turn on in this folder…" : "Turn off in this folder…"}
            </Button>
          )}
          <span className="text-meta text-muted-foreground">
            {folder
              ? "Written to the folder you chose."
              : "Choose a folder to switch it there."}
          </span>
        </div>
      </Section>

      <Section title="Where it comes from">
        <div className="rounded-xl border bg-card px-4 py-3 text-meta text-muted-foreground">
          <p>
            {plugin.marketplace ? (
              <>
                From the{" "}
                <span className="font-medium text-foreground">{plugin.marketplace}</span>{" "}
                marketplace{plugin.source ? <>, {plugin.source}</> : null}.
              </>
            ) : (
              "Installed without a marketplace."
            )}
          </p>
          {plugin.installPath && (
            <p>
              Installed at{" "}
              <span className="font-mono text-xs break-all">{plugin.installPath}</span>
            </p>
          )}
          {cost && (
            <p>
              About {cost} always loaded
              {plugin.cost.measured ? " (measured)" : " (Claude Code's projection)"}.
            </p>
          )}
          {plugin.errors.map((error) => (
            <p key={error} className="text-destructive">
              {error}
            </p>
          ))}
        </div>
        {plugin.selectSource && onChooseItems && (
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => onChooseItems(plugin.selectSource as string)}
            >
              Choose items of this plugin…
            </Button>
            <span className="text-meta text-muted-foreground">
              Single skills, agents, commands and hooks are chosen in Sources.
            </span>
          </div>
        )}
      </Section>

      <Section title="It provides">
        {bringsText(plugin.brings) === "nothing" ? (
          <p className="rounded-xl border bg-card px-4 py-4 text-meta text-muted-foreground">
            Nothing that Toolport can list.
          </p>
        ) : (
          <div className="divide-y overflow-hidden rounded-xl border bg-card">
            <Names title="Skills" names={items.skills} />
            <Names title="Agents" names={items.agents} />
            <Names title="Commands" names={items.commands} />
            <Names title="LSP servers" names={items.lspServers} />
          </div>
        )}
      </Section>

      <Section title="Hooks" count={plugin.hooks.length}>
        {plugin.hooks.length === 0 ? (
          <p className="rounded-xl border bg-card px-4 py-4 text-meta text-muted-foreground">
            This plugin has no hooks.
          </p>
        ) : (
          <ul className="divide-y overflow-hidden rounded-xl border bg-card">
            {plugin.hooks.map((hook, index) => (
              <HookRow key={`${hook.event}-${index}`} hook={hook} />
            ))}
          </ul>
        )}
      </Section>

      <Section title="MCP servers" count={plugin.mcpServers.length}>
        {plugin.mcpServers.length === 0 ? (
          <p className="rounded-xl border bg-card px-4 py-4 text-meta text-muted-foreground">
            This plugin starts no MCP server.
          </p>
        ) : (
          <>
            <ul className="divide-y overflow-hidden rounded-xl border bg-card">
              {plugin.mcpServers.map((server) => (
                <McpRow
                  key={server.key}
                  server={server}
                  folder={folder}
                  onSwitch={(action) => setDialog({ kind: "mcp", action, server })}
                />
              ))}
            </ul>
            <p className="text-meta text-muted-foreground">
              Claude Code starts these servers itself; they are not behind the Toolport
              gateway. Denying one only tells Claude Code not to start it in the folder.
            </p>
          </>
        )}
      </Section>

      <PluginSettings plugin={plugin} folder={folder} onChanged={onChanged} />

      {dialog?.kind === "switch" && folder && (
        <PlanDialog
          title={`${dialog.off ? "Turn off" : "Turn on"} ${plugin.name} here`}
          confirmLabel={dialog.off ? "Turn off" : "Turn on"}
          preview={() => switchPlugin(dialog.off ? "off" : "on", plugin.id, folder, true)}
          confirm={() =>
            switchPlugin(dialog.off ? "off" : "on", plugin.id, folder, false)
          }
          onClose={() => setDialog(null)}
          onDone={onChanged}
        />
      )}
      {dialog?.kind === "mcp" && folder && (
        <PlanDialog
          title={`${dialog.action === "deny" ? "Deny" : "Allow"} ${dialog.server.name} here`}
          confirmLabel={dialog.action === "deny" ? "Deny server" : "Allow server"}
          preview={() =>
            switchMcp(dialog.action, plugin.id, dialog.server.name, folder, true)
          }
          confirm={() =>
            switchMcp(dialog.action, plugin.id, dialog.server.name, folder, false)
          }
          onClose={() => setDialog(null)}
          onDone={onChanged}
        />
      )}
    </>
  );
}

/** One plugin as a page with Back, the way a skill is. */
export default function PluginDetail({
  id,
  folder,
  onBack,
  onChanged,
  onChooseItems,
}: {
  id: string;
  folder: string | null;
  onBack: () => void;
  onChanged: () => void;
  onChooseItems?: (source: string) => void;
}) {
  const plugin = useLoad(() => showPlugin(id, folder), `${id}|${folder}`);
  return (
    <div className="flex flex-col gap-5">
      <Button variant="ghost" size="sm" className="self-start" onClick={onBack}>
        <ArrowLeft /> Plugins
      </Button>
      {plugin.error && !plugin.data && (
        <p role="alert" className="text-meta text-destructive">
          {plugin.error}
        </p>
      )}
      {!plugin.data && !plugin.error && <SkeletonRows label="Reading the plugin" />}
      {plugin.data && (
        <Detail
          plugin={plugin.data}
          folder={folder}
          onChanged={() => {
            plugin.reload();
            onChanged();
          }}
          onChooseItems={onChooseItems}
        />
      )}
    </div>
  );
}
