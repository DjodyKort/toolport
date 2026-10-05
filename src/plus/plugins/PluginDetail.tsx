import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import type { PluginsShowData } from "../types/plugins";
import { AsyncView, type CtlQuery } from "../ui";
import type { WriteControl } from "../skills/hooks";
import { plural } from "../skills/model";
import { Section } from "../skills/parts";
import { AdapterSettings } from "./AdapterSettings";
import {
  configSpec,
  draftArgs,
  homeShort,
  mcpSpec,
  SCOPE_NAME,
  tokenText,
  undoArgs,
  updateSpec,
  updateText,
  whereText,
  type Draft,
  type Scope,
} from "./model";
import { disableSpec, enableSpec, offSpec, onSpec } from "./switches";

function Brings({ show }: { show: PluginsShowData }) {
  const b = show.brings;
  const items: Array<[number, string]> = [
    [b.skills, "skill"],
    [b.agents, "agent"],
    [b.commands, "command"],
    [b.hooks, "hook"],
    [b.mcpServers, "MCP server"],
    [b.lspServers, "language server"],
  ];
  return (
    <ul aria-label="What it brings" className="flex flex-wrap gap-2 text-sm">
      {items
        .filter(([n]) => n > 0)
        .map(([n, name]) => (
          <li key={name} className="rounded-md border px-2 py-1">
            {plural(n, name)}
          </li>
        ))}
      {items.every(([n]) => n === 0) && (
        <li className="text-muted-foreground">Nothing Toolport can count.</li>
      )}
    </ul>
  );
}

function Cost({ show }: { show: PluginsShowData }) {
  const { projected, measured } = show.cost;
  return (
    <dl
      aria-label="Cost"
      className="grid grid-cols-[10rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm"
    >
      <dt className="text-muted-foreground">Projected by Claude Code</dt>
      <dd>
        <b>{tokenText(projected)}</b>
        <small className="block text-xs text-muted-foreground">
          Claude Code&apos;s own figure for the always-on cost. It counts every skill
          description before the skill list is capped, so it overstates.
        </small>
      </dd>
      <dt className="text-muted-foreground">Measured by Toolport</dt>
      <dd>
        <b>{tokenText(measured)}</b>
        <small className="block text-xs text-muted-foreground">
          {measured
            ? "From a real request in a folder (Context > This folder > Measure without it)."
            : "Not measured yet: run Measure without it in Context > This folder."}
        </small>
      </dd>
    </dl>
  );
}

function Where({ show }: { show: PluginsShowData }) {
  return (
    <ul aria-label="Where it is on" className="flex flex-col gap-1 text-sm">
      {(["user", "project", "local"] as Scope[]).map((scope) => {
        const state = whereText(show.enabled, scope);
        return (
          <li key={scope} className="flex items-center gap-2">
            <Badge variant={state === "on" ? "success" : "secondary"}>{state}</Badge>
            {SCOPE_NAME[scope]}
          </li>
        );
      })}
      <li className="text-xs text-muted-foreground">
        In the folder you chose it is <b>{show.enabled.effective ? "on" : "off"}</b>: this
        folder over the project over your own settings.
      </li>
    </ul>
  );
}

function Servers({
  show,
  cwd,
  write,
  busy,
}: {
  show: PluginsShowData;
  cwd: string;
  write: WriteControl;
  busy: boolean;
}) {
  if (show.mcpServers.length === 0)
    return (
      <p className="text-sm text-muted-foreground">This plugin brings no MCP server.</p>
    );
  const reason = cwd ? null : "Choose a folder above: a server is denied per folder.";
  return (
    <div className="flex flex-col gap-2">
      <ul
        aria-label="MCP servers of the plugin"
        className="flex flex-col divide-y rounded-lg border"
      >
        {show.mcpServers.map((server) => {
          const denied =
            server.denied.local ||
            server.denied.project ||
            server.denied.user ||
            server.denied.managed;
          return (
            <li
              key={server.key}
              className="flex flex-wrap items-center gap-2 px-3 py-2 text-sm"
            >
              <span className="flex min-w-0 flex-1 flex-col">
                <b>{server.name}</b>
                <small className="truncate text-muted-foreground">
                  {server.url ?? (server.command ?? []).join(" ")}
                </small>
                <small className="text-muted-foreground">
                  tools start with <code className="font-mono">{server.toolPrefix}</code>
                </small>
              </span>
              <Badge
                variant="warning"
                title="Toolport shows this server and can deny it per folder; it does not route it through the gateway"
              >
                not governed
              </Badge>
              {denied && <Badge variant="secondary">denied</Badge>}
              {server.denied.local ? (
                <Button
                  size="xs"
                  variant="outline"
                  disabled={busy || !cwd}
                  title={reason ?? undefined}
                  onClick={() => write.begin(mcpSpec("allow", show.id, server.name, cwd))}
                >
                  Allow again…
                </Button>
              ) : (
                <Button
                  size="xs"
                  variant="outline"
                  disabled={busy || !cwd || denied}
                  title={
                    reason ??
                    (denied
                      ? "Already denied in a settings file Toolport does not own"
                      : undefined)
                  }
                  onClick={() => write.begin(mcpSpec("deny", show.id, server.name, cwd))}
                >
                  Deny in a folder…
                </Button>
              )}
            </li>
          );
        })}
      </ul>
      <p className="text-xs text-muted-foreground">
        These servers start with the plugin and sit outside the one gateway. Denying one
        writes a <code>deniedMcpServers</code> entry in the folder&apos;s
        settings.local.json.
      </p>
    </div>
  );
}

function Options({ show }: { show: PluginsShowData }) {
  if (show.options.length === 0) return null;
  return (
    <Section title="Plugin options" count={show.options.length}>
      <ul
        aria-label="Plugin options"
        className="flex flex-col divide-y rounded-lg border text-sm"
      >
        {show.options.map((option) => (
          <li key={option.key} className="flex flex-col px-3 py-2">
            <span className="flex items-center gap-2">
              <b>{option.title}</b>
              {option.sensitive && <Badge variant="outline">sensitive</Badge>}
              <Badge variant={option.configured ? "success" : "secondary"}>
                {option.configured ? "set" : "not set"}
              </Badge>
            </span>
            <small className="text-muted-foreground">{option.description}</small>
            {!option.sensitive && option.current != null && (
              <small>
                Now: <code className="font-mono">{String(option.current)}</code>
              </small>
            )}
          </li>
        ))}
      </ul>
      <p className="text-xs text-muted-foreground">
        Options are set with <code>claude plugin configure</code>. Toolport never shows or
        writes a sensitive one.
      </p>
    </Section>
  );
}

/** One plugin: what it brings, what it costs, where it is on, and the controls that exist. */
export function PluginDetail({
  query,
  cwd,
  write,
  onOpenHooks,
}: {
  query: CtlQuery<PluginsShowData>;
  cwd: string;
  write: WriteControl;
  onOpenHooks?: () => void;
}) {
  const [draft, setDraft] = useState<Draft>({});
  useEffect(() => setDraft({}), [query.data]);
  const onChange = (key: string, value: string | null | undefined) =>
    setDraft((now) => {
      const next = { ...now };
      if (value === undefined) delete next[key];
      else next[key] = value;
      return next;
    });
  return (
    <AsyncView query={query} errorTitle="Couldn't read the plugin" context="plugins show">
      {(show) => {
        const busy = write.busy;
        const updatable =
          show.update.state === "update" || show.update.state === "unknown";
        return (
          <div
            role="region"
            aria-label={`Plugin ${show.name}`}
            className="flex flex-col gap-4"
          >
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="text-base font-semibold">{show.name}</h3>
              <Badge variant="secondary">{show.version}</Badge>
              <Badge variant={show.update.state === "update" ? "warning" : "secondary"}>
                {updateText(show)}
              </Badge>
              <Badge variant={show.enabled.effective ? "success" : "secondary"}>
                {show.enabled.effective ? "on here" : "off here"}
              </Badge>
              {show.from === "files" && <Badge variant="outline">read from files</Badge>}
            </div>
            <p className="text-sm text-muted-foreground">{show.description}</p>
            <p className="text-xs text-muted-foreground">
              From <code className="font-mono">{show.source}</code>, installed at{" "}
              <code className="font-mono">{homeShort(show.installPath)}</code>
            </p>
            {show.warnings.map((warning) => (
              <Callout key={warning} variant="warning" role="status">
                {warning}
              </Callout>
            ))}
            {show.adapterProblems.length > 0 && (
              <Callout variant="warning" role="status">
                {show.adapterProblems
                  .map((p) => `${homeShort(p.file)}: ${p.message}`)
                  .join(" ")}
              </Callout>
            )}
            <Section title="What it brings">
              <Brings show={show} />
            </Section>
            <Section title="Cost">
              <Cost show={show} />
            </Section>
            <Section title="Where it is on">
              <Where show={show} />
            </Section>
            <div className="flex flex-wrap gap-2">
              <Button
                size="sm"
                disabled={busy || !cwd}
                title={
                  cwd
                    ? undefined
                    : "Choose a folder above: a plugin is turned off per folder"
                }
                onClick={() => write.begin(offSpec(show.id, show.name, cwd))}
              >
                Turn off in a folder…
              </Button>
              {show.enabled.local === false && (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy || !cwd}
                  onClick={() => write.begin(onSpec(show.id, show.name, cwd))}
                >
                  Turn back on in this folder…
                </Button>
              )}
              <Button
                size="sm"
                variant="outline"
                disabled={busy || !updatable}
                title={updatable ? undefined : "This plugin is already up to date"}
                onClick={() => write.begin(updateSpec(show.name))}
              >
                Update…
              </Button>
              {show.enabled.user === false ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() => write.begin(enableSpec(show.id, show.name))}
                >
                  Enable everywhere…
                </Button>
              ) : (
                <Button
                  size="sm"
                  variant="destructive"
                  disabled={busy}
                  onClick={() => write.begin(disableSpec(show.id, show.name))}
                >
                  Disable everywhere…
                </Button>
              )}
            </div>
            <Section title="Settings">
              {show.knobs.length > 0 ? (
                <AdapterSettings
                  knobs={show.knobs}
                  draft={draft}
                  onChange={onChange}
                  folder={cwd}
                  busy={busy}
                  canUndo={undoArgs(show.knobs).length > 0}
                  onApply={() =>
                    write.begin(
                      configSpec(
                        show.id,
                        cwd,
                        draftArgs(draft, show.knobs),
                        `Apply ${show.name} settings to a folder`,
                      ),
                    )
                  }
                  onUndo={() =>
                    write.begin(
                      configSpec(
                        show.id,
                        cwd,
                        undoArgs(show.knobs),
                        `Undo ${show.name} settings in a folder`,
                      ),
                    )
                  }
                />
              ) : (
                <p className="text-sm text-muted-foreground">
                  {show.name} has no adapter, so Toolport knows no switch of its own for
                  it. Claude Code has no per-hook switch: a plugin&apos;s hooks go on or
                  off with the plugin. An adapter file in the Toolport data folder can
                  name a plugin&apos;s own switches.
                </p>
              )}
            </Section>
            <Section title="MCP servers" count={show.mcpServers.length}>
              <Servers show={show} cwd={cwd} write={write} busy={busy} />
            </Section>
            {show.hooks.length > 0 && (
              <Section title="Hooks" count={show.hooks.length}>
                <p className="text-sm">
                  {plural(show.hooks.length, "hook")} in this plugin, counted from the
                  plugin&apos;s files, not timed.{" "}
                  {onOpenHooks && (
                    <Button size="xs" variant="link" onClick={onOpenHooks}>
                      See them in Context &gt; Hooks
                    </Button>
                  )}
                </p>
              </Section>
            )}
            <Options show={show} />
          </div>
        );
      }}
    </AsyncView>
  );
}
