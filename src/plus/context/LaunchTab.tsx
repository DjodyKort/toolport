import { useEffect, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Input } from "@/components/ui/input";
import type {
  ContextClientListData,
  ContextPlanData,
  ContextProfileListData,
  ContextStatusData,
} from "../types/context";
import { CopyButton, PlanPreview, type PlanV1 } from "../ui";
import { useRead, useWrite, type WriteControl } from "./hooks";
import { FoldersSection, LoadsSection } from "./InsightCards";
import { CheckpointSection } from "./Checkpoint";
import {
  plural,
  scopeText,
  selectionText,
  shellChecks,
  shimFunctions,
  statusView,
  worst,
  type LayerRow,
  type ProfileRow,
} from "./model";
import {
  CheckField,
  Checks,
  Code,
  Field,
  FormDialog,
  Kv,
  QuerySection,
  SELECT_CLASS,
  useDialog,
  useRows,
} from "./parts";
import { checksOf, contextPlan } from "./plans";
import { useRestoreFocus } from "./useRestoreFocus";
import { WriteDialogs } from "./WriteDialogs";

type Dialog = "profile" | "remove" | "disable" | "layer" | "init" | null;

function DeploySection({
  write,
  flags,
  setFlags,
  version,
}: {
  write: WriteControl;
  flags: DeployFlags;
  setFlags: (flags: DeployFlags) => void;
  version: number;
}) {
  const extra = deployArgv(flags);
  const plan = useRead<ContextPlanData>(["context", "plan", ...extra]);
  const { reload } = plan;
  useEffect(() => {
    if (version > 0) reload();
  }, [version, reload]);
  const apply = (verb: "apply" | "sync") =>
    write.begin({
      command: `context ${verb}`,
      title: verb === "sync" ? "Sync the context files" : "Apply the context files",
      argv: [
        "context",
        verb,
        ...extra,
        ...(verb === "apply" && flags.noPersist ? ["--no-persist"] : []),
      ],
      confirmLabel: verb === "sync" ? "Sync" : "Apply",
      phrase: verb,
    });
  const shown: PlanV1 | null = plan.data
    ? contextPlan("context plan", plan.data, false)
    : null;
  return (
    <QuerySection
      title="Deploy"
      hint="What a context sync writes: the shims file and the generated launch profiles."
      query={plan}
      actions={
        <>
          <Button
            size="sm"
            variant="outline"
            disabled={write.busy}
            onClick={() => apply("apply")}
          >
            Apply…
          </Button>
          <Button size="sm" disabled={write.busy} onClick={() => apply("sync")}>
            Sync…
          </Button>
        </>
      }
    >
      {(data) => (
        <div className="flex flex-col gap-3">
          <fieldset className="flex flex-wrap gap-x-5 gap-y-2">
            <legend className="sr-only">Deploy options</legend>
            <CheckField
              label="Deploy the rules too"
              checked={flags.rules}
              onChange={(rules) => setFlags({ ...flags, rules })}
            />
            <CheckField
              label="Point the shell at Toolport's folder"
              hint="Rewrites the source lines of the shell rc file, after a backup"
              checked={flags.rewriteZshrc}
              onChange={(rewriteZshrc) => setFlags({ ...flags, rewriteZshrc })}
            />
            <CheckField
              label="Do not save the config"
              hint="Apply only"
              checked={flags.noPersist}
              onChange={(noPersist) => setFlags({ ...flags, noPersist })}
            />
          </fieldset>
          {shown && <PlanPreview data={{ plan: shown }} />}
          <Checks checks={checksOf(data.checks)} label="Checks" />
        </div>
      )}
    </QuerySection>
  );
}

interface DeployFlags {
  rules: boolean;
  rewriteZshrc: boolean;
  noPersist: boolean;
}

function deployArgv(flags: DeployFlags): string[] {
  return [
    ...(flags.rules ? ["--rules"] : []),
    ...(flags.rewriteZshrc ? ["--rewrite-zshrc"] : []),
  ];
}

function ProfileForm({ write, onClose }: { write: WriteControl; onClose: () => void }) {
  const [name, setName] = useState("");
  const [org, setOrg] = useState(true);
  const [orgMode, setOrgMode] = useState("import");
  const [rules, setRules] = useState("inherit");
  const [servers, setServers] = useState("inherit");
  const [commands, setCommands] = useState(true);
  const [skills, setSkills] = useState(true);
  const valid = /^[A-Za-z0-9][A-Za-z0-9_-]*$/.test(name.trim());
  const submit = () => {
    const argv = [
      "context",
      "profile",
      "add",
      name.trim(),
      ...(org ? [] : ["--no-org"]),
      ...(org && orgMode !== "import" ? ["--org-mode", orgMode] : []),
      ...(rules.trim() && rules.trim() !== "inherit" ? ["--rules", rules.trim()] : []),
      ...(servers.trim() && servers.trim() !== "inherit"
        ? ["--servers", servers.trim()]
        : []),
      ...(commands ? [] : ["--no-commands"]),
      ...(skills ? [] : ["--no-skills"]),
    ];
    onClose();
    write.begin({
      command: "context profile add",
      title: `Add launch profile ${name.trim()}`,
      argv,
      confirmLabel: "Add profile",
      phrase: "add",
    });
  };
  return (
    <FormDialog
      title="Add a launch profile"
      intro="A launch profile starts Claude in its own settings folder with its own org file, rules and servers. It needs its own sign-in."
      submitLabel="Preview"
      valid={valid}
      onSubmit={submit}
      onClose={onClose}
    >
      <Field
        label="Name"
        hint="Letters, digits, dash and underscore. The shell function is claude-<name>."
      >
        {(id, describedBy) => (
          <Input
            id={id}
            aria-describedby={describedBy}
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoComplete="off"
          />
        )}
      </Field>
      <CheckField label="Include the org file" checked={org} onChange={setOrg} />
      {org && (
        <Field label="Org file">
          {(id) => (
            <select
              id={id}
              className={SELECT_CLASS}
              value={orgMode}
              onChange={(e) => setOrgMode(e.target.value)}
            >
              <option value="import">Import it (follows the org file)</option>
              <option value="copy">Copy it</option>
            </select>
          )}
        </Field>
      )}
      <Field label="Rules" hint="inherit, none, or a comma-separated list of rule names">
        {(id, describedBy) => (
          <Input
            id={id}
            aria-describedby={describedBy}
            value={rules}
            onChange={(e) => setRules(e.target.value)}
            autoComplete="off"
          />
        )}
      </Field>
      <Field
        label="Servers"
        hint="inherit, none, or a comma-separated list of server names"
      >
        {(id, describedBy) => (
          <Input
            id={id}
            aria-describedby={describedBy}
            value={servers}
            onChange={(e) => setServers(e.target.value)}
            autoComplete="off"
          />
        )}
      </Field>
      <CheckField
        label="Link the org commands"
        checked={commands}
        onChange={setCommands}
      />
      <CheckField label="Link the skills" checked={skills} onChange={setSkills} />
    </FormDialog>
  );
}

function ProfilesSection({
  write,
  profiles,
  onAdd,
  onRemove,
}: {
  write: WriteControl;
  profiles: ReturnType<typeof useRead<ContextProfileListData>>;
  onAdd: () => void;
  onRemove: (name: string) => void;
}) {
  return (
    <QuerySection
      title="Launch profiles"
      hint="A launch profile starts Claude in its own settings folder. It needs its own sign-in, so keep few."
      query={profiles}
      isEmpty={(data) => data.profiles.length === 0}
      empty={
        <div className="rounded-lg border border-dashed p-4 text-sm text-muted-foreground">
          No launch profiles.{" "}
          <Button size="sm" variant="outline" onClick={onAdd}>
            <Plus /> Add profile
          </Button>
        </div>
      }
      actions={
        <Button size="sm" variant="outline" disabled={write.busy} onClick={onAdd}>
          <Plus /> Add profile…
        </Button>
      }
    >
      {(data) => (
        <ul
          aria-label="Launch profiles"
          className="flex flex-col divide-y rounded-lg border"
        >
          {(data.profiles as ProfileRow[]).map((profile) => (
            <li
              key={profile.name}
              className="flex flex-wrap items-center gap-3 px-3 py-2 text-sm"
            >
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="flex flex-wrap items-center gap-2">
                  <b>{profile.name}</b>
                  <Code>{profile.shim}</Code>
                  <Badge variant={profile.generated ? "success" : "warning"}>
                    {profile.generated ? "Generated" : "Not generated"}
                  </Badge>
                </span>
                <span className="text-xs text-muted-foreground">
                  {profile.org ? `Org file by ${profile.orgMode}` : "No org file"} ·{" "}
                  {selectionText(profile.rules, "rules")} ·{" "}
                  {selectionText(profile.servers, "servers")}
                </span>
                <span className="font-mono text-xs break-all text-muted-foreground">
                  {profile.dir}
                </span>
              </div>
              <Button
                size="xs"
                variant="outline"
                aria-label={`Regenerate ${profile.name}`}
                disabled={write.busy}
                onClick={() =>
                  write.begin({
                    command: "context sync",
                    title: `Regenerate launch profile ${profile.name}`,
                    argv: ["context", "sync"],
                    confirmLabel: "Regenerate",
                    phrase: "regenerate",
                  })
                }
              >
                Regenerate…
              </Button>
              <Button
                size="xs"
                variant="outline"
                aria-label={`Remove ${profile.name}`}
                disabled={write.busy}
                onClick={() => onRemove(profile.name)}
              >
                <Trash2 /> Remove…
              </Button>
            </li>
          ))}
        </ul>
      )}
    </QuerySection>
  );
}

function ShellSection({
  write,
  status,
  checks,
  onDisable,
}: {
  write: WriteControl;
  status: ReturnType<typeof useRead<ContextStatusData>>;
  checks: ReturnType<typeof useRead<ContextPlanData>>;
  onDisable: () => void;
}) {
  return (
    <QuerySection
      title="Shell shims"
      hint="Shell functions that launch Claude with the right profile."
      query={status}
    >
      {(raw) => {
        const view = statusView(raw);
        const order = shellChecks(checksOf(checks.data?.checks));
        const old = view.zshrc.legacyLines;
        return (
          <div className="flex flex-col gap-3">
            <Kv
              rows={[
                [
                  "File",
                  <span key="f" className="flex flex-wrap items-center gap-2">
                    <Code>{view.shims.path}</Code>
                    <Badge variant={view.shims.exists ? "success" : "warning"}>
                      {view.shims.exists ? "Written" : "Not written"}
                    </Badge>
                    <CopyButton text={view.shims.path} label="Copy path" size="xs" />
                  </span>,
                ],
                [
                  "Source order",
                  order.length === 0 ? (
                    <span className="text-muted-foreground">
                      Not checked: nothing is deployed yet
                    </span>
                  ) : (
                    <Badge variant={worst(order) === "ok" ? "success" : "warning"}>
                      {worst(order) === "ok" ? "Sourced last" : "Needs a look"}
                    </Badge>
                  ),
                ],
              ]}
            />
            <Checks checks={order} label="Shell checks" />
            {view.shims.legacyExists && (
              <p className="text-xs text-muted-foreground">
                An older copy of the file still exists at{" "}
                <Code>{view.shims.legacyPath}</Code>.
              </p>
            )}
            <div>
              <h4 className="mb-1 text-xs font-semibold text-muted-foreground">
                Functions in the file
              </h4>
              <ul aria-label="Functions" className="flex flex-col gap-1.5 text-sm">
                {shimFunctions(view.profiles).map((fn) => (
                  <li key={fn.name} className="flex flex-wrap items-baseline gap-2">
                    <Code>{fn.name}</Code>
                    {!fn.always && <Badge variant="outline">depends on the setup</Badge>}
                    <span className="min-w-0 text-muted-foreground">{fn.what}</span>
                  </li>
                ))}
              </ul>
            </div>
            {old.length > 0 && (
              <Callout variant="warning" role="status" className="flex flex-col gap-2">
                <p>
                  The shell still reads {plural(old.length, "line")} from the old folder,
                  which goes away with the old tool.
                </p>
                <ul className="list-disc pl-4 font-mono text-xs">
                  {old.map((line) => (
                    <li key={line.line}>
                      {line.line}: {line.text}
                    </li>
                  ))}
                </ul>
                <div>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={write.busy}
                    onClick={() =>
                      write.begin({
                        command: "context sync",
                        title: "Move the shell lines to Toolport's folder",
                        argv: ["context", "sync", "--rewrite-zshrc"],
                        confirmLabel: "Rewrite the shell file",
                        phrase: "move",
                      })
                    }
                  >
                    Move…
                  </Button>
                </div>
              </Callout>
            )}
            {view.zshrc.deadAliases.length > 0 && (
              <ul
                aria-label="Aliases that stop working"
                className="list-disc pl-4 text-xs text-muted-foreground"
              >
                {view.zshrc.deadAliases.map((alias) => (
                  <li key={alias.name}>
                    <Code>{alias.name}</Code> runs <Code>{alias.command}</Code> and stops
                    working with the old tool.
                  </li>
                ))}
              </ul>
            )}
            <div>
              <Button
                size="sm"
                variant="outline"
                disabled={write.busy}
                onClick={onDisable}
              >
                <Trash2 /> Disable shims…
              </Button>
            </div>
          </div>
        );
      }}
    </QuerySection>
  );
}

function LayersSection({
  write,
  layers,
  status,
  onAdd,
  onInit,
}: {
  write: WriteControl;
  layers: ReturnType<typeof useRead<ContextClientListData>>;
  status: ReturnType<typeof useRead<ContextStatusData>>;
  onAdd: () => void;
  onInit: () => void;
}) {
  const initialised = status.data ? status.data.config.exists : null;
  return (
    <QuerySection
      title="Personal and client layers"
      hint="Your own sections on top of the org file. A layer is always on, or only under a folder pattern."
      query={layers}
      isEmpty={(data) => data.layers.length === 0}
      empty={
        <div className="flex flex-col gap-2 rounded-lg border border-dashed p-4 text-sm text-muted-foreground">
          <p>No layers yet. Set up the personal layer first.</p>
          <div>
            <Button size="sm" variant="outline" onClick={onInit}>
              Set up…
            </Button>
          </div>
        </div>
      }
      actions={
        <>
          {initialised === false && (
            <Button size="sm" variant="outline" disabled={write.busy} onClick={onInit}>
              Set up…
            </Button>
          )}
          <Button size="sm" variant="outline" disabled={write.busy} onClick={onAdd}>
            <Plus /> Add client layer…
          </Button>
        </>
      }
    >
      {(data) => (
        <ul aria-label="Layers" className="flex flex-col divide-y rounded-lg border">
          {(data.layers as LayerRow[]).map((layer) => (
            <li key={layer.name} className="flex flex-col gap-0.5 px-3 py-2 text-sm">
              <span className="flex flex-wrap items-center gap-2">
                <b>{layer.name}</b>
                <Badge variant={layer.globs.length === 0 ? "secondary" : "info"}>
                  {scopeText(layer)}
                </Badge>
              </span>
              {layer.description && (
                <span className="text-xs text-muted-foreground">{layer.description}</span>
              )}
              <span className="font-mono text-xs break-all text-muted-foreground">
                {layer.path}
              </span>
            </li>
          ))}
        </ul>
      )}
    </QuerySection>
  );
}

/** The Launch & shell tab: deploy, launch profiles, shell shims, layers, and the reads that
 * show what a session loads (loads, folder routing, checkpoint). */
export function LaunchTab() {
  const rows = useRows();
  const status = useRead<ContextStatusData>(["context", "status"]);
  const profiles = useRead<ContextProfileListData>(["context", "profile", "list"]);
  const layers = useRead<ContextClientListData>(["context", "client", "list"]);
  const plan = useRead<ContextPlanData>(["context", "plan"]);
  const [version, setVersion] = useState(0);
  const [flags, setFlags] = useState<DeployFlags>({
    rules: false,
    rewriteZshrc: false,
    noPersist: false,
  });
  const { reload: reloadStatus } = status;
  const write = useWrite(rows, () => {
    reloadStatus();
    profiles.reload();
    layers.reload();
    plan.reload();
    setVersion((n) => n + 1);
  });
  const dialog = useDialog<Exclude<Dialog, null>>();
  const [target, setTarget] = useState("");
  useRestoreFocus();
  return (
    <div className="flex flex-col gap-4">
      <DeploySection write={write} flags={flags} setFlags={setFlags} version={version} />
      <ProfilesSection
        write={write}
        profiles={profiles}
        onAdd={() => dialog.show("profile")}
        onRemove={(name) => {
          setTarget(name);
          dialog.show("remove");
        }}
      />
      <ShellSection
        write={write}
        status={status}
        checks={plan}
        onDisable={() => dialog.show("disable")}
      />
      <LayersSection
        write={write}
        layers={layers}
        status={status}
        onAdd={() => dialog.show("layer")}
        onInit={() => dialog.show("init")}
      />
      <LoadsSection
        version={version}
        profiles={profiles.data?.profiles as ProfileRow[] | undefined}
      />
      <FoldersSection write={write} version={version} />
      <CheckpointSection profiles={profiles.data?.profiles as ProfileRow[] | undefined} />
      {dialog.open === "profile" && <ProfileForm write={write} onClose={dialog.close} />}
      {dialog.open === "remove" && (
        <RemoveForm write={write} name={target} onClose={dialog.close} />
      )}
      {dialog.open === "disable" && <DisableForm write={write} onClose={dialog.close} />}
      {dialog.open === "layer" && <LayerForm write={write} onClose={dialog.close} />}
      {dialog.open === "init" && <InitForm write={write} onClose={dialog.close} />}
      <WriteDialogs write={write} />
    </div>
  );
}

function RemoveForm({
  write,
  name,
  onClose,
}: {
  write: WriteControl;
  name: string;
  onClose: () => void;
}) {
  const [purge, setPurge] = useState(false);
  return (
    <FormDialog
      title={`Remove launch profile ${name}`}
      intro="The profile leaves the config and its shell function goes away."
      submitLabel="Preview"
      onSubmit={() => {
        onClose();
        write.begin({
          command: "context profile remove",
          title: `Remove launch profile ${name}`,
          argv: ["context", "profile", "remove", name, ...(purge ? ["--purge"] : [])],
          confirmLabel: "Remove profile",
          phrase: name,
        });
      }}
      onClose={onClose}
    >
      <CheckField
        label="Also delete its folder"
        hint="Its sign-in and history in that folder are deleted with it"
        checked={purge}
        onChange={setPurge}
      />
    </FormDialog>
  );
}

function DisableForm({ write, onClose }: { write: WriteControl; onClose: () => void }) {
  const [purge, setPurge] = useState(false);
  return (
    <FormDialog
      title="Disable the shell shims"
      intro="Removes the generated shims file. Your shell then starts the plain claude again."
      submitLabel="Preview"
      onSubmit={() => {
        onClose();
        write.begin({
          command: "context disable",
          title: "Disable the shell shims",
          argv: ["context", "disable", ...(purge ? ["--purge-profiles"] : [])],
          confirmLabel: "Disable",
          phrase: "disable",
        });
      }}
      onClose={onClose}
    >
      <CheckField
        label="Also delete the launch profile folders"
        hint="Every generated profile folder is removed, with its sign-in"
        checked={purge}
        onChange={setPurge}
      />
    </FormDialog>
  );
}

function LayerForm({ write, onClose }: { write: WriteControl; onClose: () => void }) {
  const [name, setName] = useState("");
  const [glob, setGlob] = useState("");
  return (
    <FormDialog
      title="Add a client layer"
      intro="Creates a rule that loads only when Claude works under the folder pattern."
      submitLabel="Preview"
      valid={name.trim() !== ""}
      onSubmit={() => {
        onClose();
        write.begin({
          command: "context client add",
          title: `Add client layer ${name.trim()}`,
          argv: [
            "context",
            "client",
            "add",
            name.trim(),
            ...(glob.trim() ? ["--glob", glob.trim()] : []),
          ],
          confirmLabel: "Add layer",
          phrase: "add",
        });
      }}
      onClose={onClose}
    >
      <Field label="Name">
        {(id) => (
          <Input
            id={id}
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoComplete="off"
          />
        )}
      </Field>
      <Field label="Folder pattern" hint="Optional. The default is **/clients/<name>/**">
        {(id, describedBy) => (
          <Input
            id={id}
            aria-describedby={describedBy}
            value={glob}
            onChange={(e) => setGlob(e.target.value)}
            autoComplete="off"
          />
        )}
      </Field>
    </FormDialog>
  );
}

function InitForm({ write, onClose }: { write: WriteControl; onClose: () => void }) {
  return (
    <FormDialog
      title="Set up the personal layer"
      intro="Three things happen: the personal layer file is scaffolded, an existing config is migrated or created, and the next steps are listed. The preview shows each file."
      submitLabel="Preview"
      onSubmit={() => {
        onClose();
        write.begin({
          command: "context init",
          title: "Set up the personal layer",
          argv: ["context", "init", "--yes"],
          confirmLabel: "Set up",
          phrase: "init",
        });
      }}
      onClose={onClose}
    >
      <ol className="list-decimal pl-5 text-sm text-muted-foreground">
        <li>Scaffold your personal layer.</li>
        <li>Save the context config.</li>
        <li>Add per-client layers and launch profiles when you need them.</li>
      </ol>
    </FormDialog>
  );
}
