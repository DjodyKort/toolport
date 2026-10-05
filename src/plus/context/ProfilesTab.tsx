import { useEffect, useState } from "react";
import { Copy, Plus } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Switch } from "@/components/ui/switch";
import type {
  ContextBundleConfigData,
  ContextBundleLaunchData,
  ContextBundleLsData,
  ContextBundleShowData,
} from "../types/context-bundle";
import { CopyButton } from "../ui";
import { useRead, useWrite } from "./hooks";
import { WriteDialogs } from "./WriteDialogs";
import { ApplyForm, DeleteForm, ProfileForm, type ProfileMode } from "./ProfileDialogs";
import { formOf, bundleParts, type BundleRow } from "./bundleModel";
import { suggestions, useRecentFolders } from "./folder";
import { folderLabel } from "./model";
import { Code, Kv, QuerySection, Section, useDialog, useRows } from "./parts";

type Dialog = "apply" | "delete" | ProfileMode;

function PluginSettingsList({
  config,
}: {
  config: Record<string, Record<string, string>>;
}) {
  const plugins = Object.entries(config);
  if (plugins.length === 0) return <span className="text-muted-foreground">none</span>;
  return (
    <ul aria-label="Plugin settings" className="flex flex-col gap-1">
      {plugins.map(([plugin, knobs]) => (
        <li key={plugin} className="flex flex-wrap items-center gap-1.5">
          <b className="font-mono text-xs">{plugin}</b>
          {Object.entries(knobs).map(([knob, value]) => (
            <Badge key={knob} variant="outline" className="font-mono">
              {knob}: {value}
            </Badge>
          ))}
        </li>
      ))}
    </ul>
  );
}

function Chips({ items, empty }: { items: string[]; empty: string }) {
  return items.length === 0 ? (
    <span className="text-muted-foreground">{empty}</span>
  ) : (
    <span className="flex flex-wrap gap-1.5">
      {items.map((item) => (
        <Badge key={item} variant="outline" className="font-mono">
          {item}
        </Badge>
      ))}
    </span>
  );
}

function Detail({
  show,
  row,
  busy,
  launch,
  onDialog,
  onUndo,
  onLaunch,
}: {
  show: ContextBundleShowData;
  row: BundleRow;
  busy: boolean;
  launch: ContextBundleLaunchData | null;
  onDialog: (dialog: Dialog) => void;
  onUndo: (folder: string) => void;
  onLaunch: () => void;
}) {
  const skills = show.skills;
  return (
    <section
      aria-label={`Profile ${show.name}`}
      className="flex flex-col gap-3 rounded-lg border bg-card p-4"
    >
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="text-sm font-semibold">{show.name}</h3>
        {show.legacy && <Badge variant="secondary">legacy skill list</Badge>}
        {row.error && <Badge variant="destructive">cannot be read</Badge>}
      </div>
      {show.description && (
        <p className="text-sm text-muted-foreground">{show.description}</p>
      )}
      {show.issues.length > 0 && (
        <ul
          aria-label="Problems"
          className="list-disc pl-4 text-xs text-amber-700 dark:text-amber-400"
        >
          {show.issues.map((issue) => (
            <li key={`${issue.key}:${issue.message}`}>
              {issue.key}: {issue.message}
            </li>
          ))}
        </ul>
      )}
      <Kv
        rows={[
          [
            "Skills",
            <span key="s" className="flex flex-col gap-1.5">
              <Chips
                items={[
                  ...skills.off.map((name) => `off: ${name}`),
                  ...skills.nameOnly.map((name) => `name only: ${name}`),
                  ...skills.allow.map((name) => `only: ${name}`),
                ]}
                empty="unchanged"
              />
            </span>,
          ],
          [
            "Server set",
            show.servers ? (
              <b key="v">{show.servers}</b>
            ) : (
              <span key="v" className="text-muted-foreground">
                unchanged: the server profile with the same name, if any
              </span>
            ),
          ],
          ["Plugins off", <Chips key="p" items={show.plugins.off} empty="unchanged" />],
          [
            "Plugin settings",
            <PluginSettingsList key="pc" config={show.plugins.config} />,
          ],
          ["MCP denies", <Chips key="md" items={show.mcp.deny} empty="none" />],
          [
            "CLAUDE.md layers",
            <Chips
              key="l"
              items={[
                ...show.layers.add,
                ...show.layers.exclude.map((glob) => `left out: ${glob}`),
              ]}
              empty="none added or hidden"
            />,
          ],
          ["Agents off", <Chips key="a" items={show.agents.off} empty="unchanged" />],
          ["Applies to", <Chips key="b" items={show.bind} empty="no folder pattern" />],
        ]}
      />
      <p className="text-xs text-muted-foreground">
        Plugin settings and MCP denies come from the profile&apos;s yaml, because{" "}
        <code>context bundle edit</code> has no flag for them. Edit{" "}
        <code className="break-all">{show.path}</code>; the change applies the next time
        you apply the profile. Settings are written as <code>env</code> keys and denies as{" "}
        <code>deniedMcpServers</code> in the folder&apos;s settings.local.json.
      </p>
      <div>
        <h4 className="mb-1 text-xs font-semibold text-muted-foreground">Applied in</h4>
        {show.appliedTo.length === 0 ? (
          <p className="text-sm text-muted-foreground">nowhere yet</p>
        ) : (
          <ul
            aria-label="Applied in"
            className="flex flex-col divide-y rounded-lg border"
          >
            {show.appliedTo.map((to) => (
              <li
                key={to.folder}
                className="flex flex-wrap items-center gap-2 px-3 py-2 text-sm"
              >
                <Code>{to.folder}</Code>
                <time className="text-xs text-muted-foreground" dateTime={to.appliedAt}>
                  {to.appliedAt}
                </time>
                {to.drift && <Badge variant="warning">changed since the apply</Badge>}
                <Button
                  size="xs"
                  variant="outline"
                  className="ml-auto"
                  disabled={busy}
                  onClick={() => onUndo(to.folder)}
                >
                  Undo…
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
      <Callout variant="info">
        Hidden skills use Claude Code's own skillOverrides, plugins enabledPlugins and
        extra CLAUDE.md files claudeMdExcludes, all in the folder's git-ignored local
        settings. Toolport lists the keys it owns and leaves every other key alone.
      </Callout>
      <div className="flex flex-wrap gap-2">
        <Button size="sm" disabled={busy} onClick={() => onDialog("apply")}>
          Apply to a folder…
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={busy || show.legacy}
          title={show.legacy ? "A legacy list is edited in its yaml file" : undefined}
          onClick={() => onDialog("edit")}
        >
          Edit
        </Button>
        <Button
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => onDialog("duplicate")}
        >
          <Copy /> Duplicate
        </Button>
        <Button
          size="sm"
          variant="destructive"
          disabled={busy}
          onClick={() => onDialog("delete")}
        >
          Delete…
        </Button>
      </div>
      <div className="flex flex-col gap-2 border-t pt-3">
        <h4 className="flex items-center gap-2 text-sm font-semibold">
          Or launch with it <Badge variant="secondary">terminal</Badge>
        </h4>
        {launch ? (
          <>
            <Code>{launch.command}</Code>
            {launch.notes.map((note) => (
              <p key={note} className="text-xs text-muted-foreground">
                {note}
              </p>
            ))}
          </>
        ) : (
          <p className="text-sm text-muted-foreground">
            Writes the settings file for this profile and gives the line to run in a
            terminal. No new login is needed.
          </p>
        )}
        <div className="flex flex-wrap gap-2">
          {launch ? (
            <CopyButton text={launch.command} label="Copy" />
          ) : (
            <Button size="sm" variant="outline" disabled={busy} onClick={onLaunch}>
              Prepare the launch line…
            </Button>
          )}
          <Button
            size="sm"
            variant="outline"
            disabled
            title="Needs a terminal: copy the line and run it there"
          >
            Open in Terminal
          </Button>
        </div>
      </div>
    </section>
  );
}

function AutoApply({
  busy,
  onToggle,
}: {
  busy: boolean;
  onToggle: (on: boolean) => void;
}) {
  const query = useRead<ContextBundleConfigData>(["context", "bundle", "config"]);
  return (
    <QuerySection title="Apply automatically" query={query}>
      {(data) => (
        <div className="flex items-center gap-3 text-sm">
          <Switch
            checked={data.autoApply}
            disabled={busy}
            aria-label="Apply automatically"
            onCheckedChange={onToggle}
          />
          <span>
            When Toolport syncs, apply a profile to new folders that match its pattern.
            Off by default; you can always apply by hand.
          </span>
        </div>
      )}
    </QuerySection>
  );
}

/** The Context tab "Profiles": a profile pairs a server set and a context bundle under one
 * name (D-066). Every write is previewed, confirmed and shown as a result. */
export function ProfilesTab() {
  const rows = useRows();
  const list = useRead<ContextBundleLsData>(["context", "bundle", "ls"]);
  const [selected, setSelected] = useState<string | null>(null);
  const [launch, setLaunch] = useState<ContextBundleLaunchData | null>(null);
  const [config, setConfig] = useState(0);
  const bundles = list.data?.bundles ?? [];
  const row = bundles.find((one) => one.name === selected) ?? bundles[0] ?? null;
  const show = useRead<ContextBundleShowData>(
    row ? ["context", "bundle", "show", row.name] : null,
  );
  const { reload: reloadList } = list;
  const { reload: reloadShow } = show;
  const write = useWrite(rows, () => {
    reloadList();
    reloadShow();
    setConfig((n) => n + 1);
  });
  const dialog = useDialog<Dialog>();
  const { recent } = useRecentFolders();
  const folders = suggestions(
    recent,
    bundles.flatMap((bundle) => bundle.appliedTo.map((to) => to.folder)),
  );
  const state = write.flow.apply.state;
  useEffect(() => {
    if (state.phase !== "done" || write.spec?.command !== "context bundle launch") return;
    const data = state.result?.envelope?.data as ContextBundleLaunchData | undefined;
    if (data?.command) setLaunch(data);
  }, [state, write.spec]);
  useEffect(() => setLaunch(null), [row?.name]);
  const name = row?.name ?? "";
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <Button onClick={() => dialog.show("from-folder")} disabled={write.busy}>
          <Plus /> Create from a folder…
        </Button>
        <Button
          variant="outline"
          onClick={() => dialog.show("new")}
          disabled={write.busy}
        >
          New profile
        </Button>
        <span className="text-sm text-muted-foreground">
          A profile is a named bundle: which skills, plugins, CLAUDE.md layers and agents
          load in a folder, paired with a server set. Example, not created for you:{" "}
          <code>light-hooks</code> keeps ecc on but quiet with <code>plugins.config</code>{" "}
          <code>ecc@ecc: hook_profile: minimal</code> and the observe hook off; write it
          as a profile yaml if you want it.
        </span>
      </div>
      <QuerySection
        title="Profiles"
        query={list}
        isEmpty={(data) => data.bundles.length === 0}
        empty={
          <p className="text-sm text-muted-foreground">
            No profile yet. Create one from a folder to keep what you hid there.
          </p>
        }
      >
        {() => (
          <div className="grid gap-4 lg:grid-cols-[minmax(0,18rem)_minmax(0,1fr)]">
            <ul
              aria-label="Profile list"
              className="flex flex-col divide-y rounded-lg border self-start"
            >
              {bundles.map((bundle) => (
                <li key={bundle.name}>
                  <button
                    type="button"
                    aria-pressed={bundle.name === name}
                    onClick={() => setSelected(bundle.name)}
                    className="flex w-full flex-col items-start gap-0.5 px-3 py-2 text-left text-sm hover:bg-muted aria-pressed:bg-muted"
                  >
                    <span className="flex w-full items-center gap-2">
                      <b>{bundle.name}</b>
                      {bundle.error && (
                        <Badge variant="destructive">cannot be read</Badge>
                      )}
                      {bundle.appliedTo.length > 0 && (
                        <Badge variant="success" className="ml-auto">
                          applied in {bundle.appliedTo.length}
                        </Badge>
                      )}
                    </span>
                    <small className="text-muted-foreground">
                      {[
                        ...bundleParts(bundle),
                        bundle.servers ? `server set ${bundle.servers}` : "",
                      ]
                        .filter(Boolean)
                        .join(" · ") || "no change"}
                    </small>
                  </button>
                </li>
              ))}
            </ul>
            {row && (
              <Section title="Selected profile">
                {show.status === "loading" ? (
                  <p role="status" className="text-sm text-muted-foreground">
                    Reading the profile…
                  </p>
                ) : show.data ? (
                  <Detail
                    show={show.data}
                    row={row}
                    busy={write.busy}
                    launch={launch}
                    onDialog={dialog.show}
                    onUndo={(folder) =>
                      write.begin({
                        command: "context bundle undo",
                        title: `Undo profile ${row.name} in ${folderLabel(folder)}`,
                        argv: ["context", "bundle", "undo", "--cwd", folder],
                        confirmLabel: "Undo",
                        phrase: row.name,
                      })
                    }
                    onLaunch={() =>
                      write.begin({
                        command: "context bundle launch",
                        title: `Prepare the launch line for ${row.name}`,
                        argv: ["context", "bundle", "launch", row.name],
                        confirmLabel: "Write the settings file",
                        phrase: row.name,
                        planned: {
                          summary: `Write the settings file of profile ${row.name}`,
                          steps: [
                            {
                              op: "create",
                              detail:
                                "A settings file in Toolport's profiles folder, derived from the definition. It is never in git.",
                            },
                          ],
                          effects: {},
                          warnings: [],
                          undo: "",
                        },
                        renderResult: (data) => (
                          <LaunchResult data={data as ContextBundleLaunchData} />
                        ),
                      })
                    }
                  />
                ) : (
                  <p role="alert" className="text-sm text-destructive">
                    Couldn't read the profile.{" "}
                    <Button size="xs" variant="outline" onClick={reloadShow}>
                      Retry
                    </Button>
                  </p>
                )}
              </Section>
            )}
          </div>
        )}
      </QuerySection>
      <AutoApplyBlock key={config} write={write} />
      {(dialog.open === "new" ||
        dialog.open === "from-folder" ||
        ((dialog.open === "edit" || dialog.open === "duplicate") && show.data)) && (
        <ProfileForm
          write={write}
          mode={dialog.open as ProfileMode}
          initial={
            dialog.open === "edit" || dialog.open === "duplicate"
              ? formOf(show.data!)
              : undefined
          }
          taken={bundles.map((bundle) => bundle.name)}
          folders={folders}
          onClose={dialog.close}
        />
      )}
      {dialog.open === "apply" && row && (
        <ApplyForm
          write={write}
          profile={row}
          initialFolder={recent[0] ?? ""}
          folders={folders}
          onClose={dialog.close}
        />
      )}
      {dialog.open === "delete" && row && (
        <DeleteForm
          write={write}
          name={row.name}
          applied={row.appliedTo.length}
          onClose={dialog.close}
        />
      )}
      <WriteDialogs write={write} />
    </div>
  );
}

function LaunchResult({ data }: { data: ContextBundleLaunchData }) {
  return (
    <section aria-label="Launch line" className="flex flex-col gap-2 text-sm">
      <p>The settings file is written. Run this in a terminal:</p>
      <Code>{data.command}</Code>
      <CopyButton text={data.command} label="Copy" />
    </section>
  );
}

function AutoApplyBlock({ write }: { write: ReturnType<typeof useWrite> }) {
  return (
    <AutoApply
      busy={write.busy}
      onToggle={(on) =>
        write.begin({
          command: "context bundle config",
          title: on
            ? "Apply profiles automatically"
            : "Stop applying profiles automatically",
          argv: ["context", "bundle", "config", "--auto-apply", on ? "on" : "off"],
          confirmLabel: on ? "Turn on" : "Turn off",
          phrase: "auto",
          planned: {
            summary: on
              ? "When Toolport syncs, a profile is applied to new folders that match its pattern"
              : "Profiles are applied by hand only",
            steps: [
              {
                op: "update",
                detail: "The context config: apply automatically " + (on ? "on" : "off"),
              },
            ],
            effects: {},
            warnings: [],
            undo: `toolportctl context bundle config --auto-apply ${on ? "off" : "on"}`,
          },
        })
      }
    />
  );
}
