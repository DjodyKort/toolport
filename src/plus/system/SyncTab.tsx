import { useState } from "react";
import { GitBranch, KeyRound, RefreshCw, RotateCcw, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { EmptyState } from "@/components/ui/empty-state";
import type { SyncStatusData } from "../bridge/data";
import type { SyncDiffData, SyncGitSyncData } from "../types";
import { AsyncView, ErrorState } from "../ui";
import { Card, Field, Intro, Kv, Mono, OfflineNote, Tag, Toggle } from "./atoms";
import { rowsOf, useRead, useRegistry, useWrite } from "./hooks";
import {
  changeCount,
  changesOf,
  itemText,
  optionFlag,
  planOfAddProject,
  planOfGitSync,
  planOfRemoveProject,
  planOfSyncPull,
  planOfSyncPush,
  planOfSyncReset,
  plural,
  type SyncChanges,
} from "./model";
import { InitDialog, MigrateDialog, RotateDialog } from "./SyncDialogs";
import { WriteDialogs } from "./WriteDialogs";

const GROUPS: Array<[keyof SyncChanges, string, string, string]> = [
  ["new", "+", "text-success", "new"],
  ["modified", "~", "text-warning", "changed"],
  ["removed", "−", "text-destructive", "removed"],
  ["conflicts", "!", "text-destructive", "conflict"],
];

function Differences({ data }: { data: SyncDiffData }) {
  const changes = changesOf(data.changes);
  const total = changeCount(changes);
  return (
    <div className="flex flex-col gap-2">
      {data.noRemote && (
        <Callout variant="info" role="status">
          The remote has no bundle yet. Push to create the first one.
        </Callout>
      )}
      {!data.noRemote && total === 0 && (
        <p className="flex items-center gap-2 text-sm">
          <Tag tone="success">In sync</Tag> This machine matches the remote bundle (
          {plural(changes?.unchanged.length ?? 0, "file")}).
        </p>
      )}
      {total > 0 && changes && (
        <ul aria-label="Differences" className="flex flex-col gap-1 text-sm">
          {GROUPS.flatMap(([key, glyph, tone, word]) =>
            changes[key].map((entry) => (
              <li key={`${key}-${itemText(entry)}`} className="flex gap-2">
                <span className={`w-4 text-center font-mono ${tone}`} aria-hidden="true">
                  {glyph}
                </span>
                <Mono>{itemText(entry)}</Mono>
                <span className="text-muted-foreground">({word})</span>
              </li>
            )),
          )}
        </ul>
      )}
      {total > 0 && changes && changes.unchanged.length > 0 && (
        <p className="text-xs text-muted-foreground">
          {changes.unchanged.length} unchanged
        </p>
      )}
    </div>
  );
}

function projectsOf(status: SyncStatusData) {
  return Object.entries(status.projects).map(([name, value]) => {
    const row = (value ?? {}) as { local_path?: string; files?: string[] };
    return { name, path: row.local_path ?? "", files: row.files ?? [] };
  });
}

/** Sync: the encrypted bundle (D-059). Everything runs through `toolportctl sync`; the
 * passphrase goes to its stdin and is never in the DOM, the argv or a log. */
export function SyncTab() {
  const registry = useRegistry();
  const rows = rowsOf(registry);
  const status = useRead<SyncStatusData>(["sync", "status"]);
  const configured = status.data?.configured === true;
  const diff = useRead<SyncDiffData>(["sync", "diff"], { enabled: configured });
  const git = useRead<SyncGitSyncData>(["sync", "git-sync", "--status"]);
  const [dialog, setDialog] = useState<"init" | "rotate" | "migrate" | null>(null);
  const [include, setInclude] = useState(false);
  const [force, setForce] = useState(false);
  const [noResolve, setNoResolve] = useState(false);
  const [runSetup, setRunSetup] = useState(false);
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [files, setFiles] = useState("");
  const [gitRepo, setGitRepo] = useState("");
  const [gitBranch, setGitBranch] = useState("");
  const [gitAuto, setGitAuto] = useState(false);

  const refresh = () => {
    status.reload();
    diff.reload();
    git.reload();
  };
  const write = useWrite(rows, refresh);
  const closeDialog = () => setDialog(null);
  const sync = (...extra: string[]) =>
    include ? [...extra, "--include-projects"] : extra;

  const push = () =>
    write.begin({
      command: "sync push",
      title: "Push to the sync repository",
      argv: ["sync", "push", ...sync()],
      confirmLabel: "Push",
      adapt: planOfSyncPush,
    });
  const pull = () =>
    write.begin({
      command: "sync pull",
      title: "Pull from the sync repository",
      argv: [
        "sync",
        "pull",
        ...sync(),
        ...(force ? ["--force"] : []),
        ...(noResolve ? ["--no-resolve"] : []),
        ...(runSetup ? ["--run-setup"] : []),
      ],
      confirmLabel: "Pull",
      adapt: planOfSyncPull,
    });

  return (
    <div className="flex flex-col gap-4">
      <OfflineNote />
      <div className="grid gap-4 lg:grid-cols-2">
        <Card
          title="Encrypted sync"
          actions={
            <Button
              size="xs"
              variant="ghost"
              aria-label="Check the sync settings again"
              onClick={refresh}
            >
              <RefreshCw />
            </Button>
          }
        >
          <AsyncView query={status} errorTitle="Couldn't read the sync settings">
            {(s) =>
              s.configured ? (
                <div className="flex flex-col gap-3">
                  <Kv
                    rows={[
                      ["Machine", s.machineId ?? "none"],
                      ["Backend", s.backend ?? "none"],
                      ["Repository", <Mono key="r">{s.repoUrl ?? "none"}</Mono>],
                      ["Branch", s.branch ?? "default"],
                      [
                        "Last sync",
                        s.lastSyncAt
                          ? `${s.lastSyncAt}${s.lastDirection ? ` (${s.lastDirection})` : ""}`
                          : "Never",
                      ],
                      [
                        "Key file",
                        s.keyfilePresent ? (
                          <Tag tone="success">present</Tag>
                        ) : (
                          <Tag tone="destructive">missing</Tag>
                        ),
                      ],
                      ["Tracked files", String(s.tracked)],
                    ]}
                  />
                  <div className="flex flex-wrap gap-2">
                    <Button size="sm" variant="outline" onClick={() => setDialog("init")}>
                      Reconfigure
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => setDialog("rotate")}
                    >
                      <KeyRound /> Rotate passphrase
                    </Button>
                    <Button
                      size="sm"
                      variant="destructive"
                      onClick={() =>
                        write.begin({
                          command: "sync reset",
                          title: "Reset sync",
                          argv: ["sync", "reset"],
                          confirmLabel: "Reset sync",
                          planned: planOfSyncReset(s.repoUrl),
                        })
                      }
                    >
                      <Trash2 /> Reset…
                    </Button>
                  </div>
                </div>
              ) : (
                <EmptyState
                  className="py-8"
                  title="Not set up"
                  description="Sync keeps your Toolport data identical on two machines. The bundle never contains your keychain keys."
                  action={
                    <Button size="sm" onClick={() => setDialog("init")}>
                      Set up sync
                    </Button>
                  }
                />
              )
            }
          </AsyncView>
        </Card>
        <Card title="What a bundle holds">
          <Kv
            rows={[
              ["Included", "servers, profiles, skills, settings"],
              ["Never", "keychain keys, tokens, secret values"],
              ["Passphrase", "entered once, never shown"],
            ]}
          />
        </Card>
      </div>

      {configured && (
        <>
          <Card
            title="Sync now"
            actions={
              <>
                <Button size="sm" variant="outline" onClick={pull}>
                  Pull…
                </Button>
                <Button size="sm" onClick={push}>
                  Push…
                </Button>
              </>
            }
          >
            <Intro>
              Both preview first: you see which files change before anything is written.
            </Intro>
            <div className="grid gap-2 sm:grid-cols-2">
              <Toggle
                label="Include the registered project files"
                checked={include}
                onChange={setInclude}
              />
              <Toggle
                label="Overwrite local changes on pull"
                checked={force}
                onChange={setForce}
                hint="Local files that changed since the last sync are replaced."
              />
              <Toggle
                label="Keep both copies on a conflict"
                checked={noResolve}
                onChange={setNoResolve}
              />
              <Toggle
                label="Run the setup step after pulling"
                checked={runSetup}
                onChange={setRunSetup}
              />
            </div>
          </Card>

          <Card
            title="Differences from the remote"
            actions={
              <Button
                size="xs"
                variant="ghost"
                aria-label="Check differences again"
                onClick={diff.reload}
              >
                <RefreshCw />
              </Button>
            }
          >
            <AsyncView query={diff} errorTitle="Couldn't compare with the remote">
              {(data) => <Differences data={data} />}
            </AsyncView>
          </Card>

          <Card title="Projects in the sync set">
            {status.data && projectsOf(status.data).length === 0 && (
              <p className="text-sm text-muted-foreground">
                No project folders are synced. Add one below to include its files.
              </p>
            )}
            {status.data && projectsOf(status.data).length > 0 && (
              <ul aria-label="Projects" className="flex flex-col divide-y">
                {projectsOf(status.data).map((project) => (
                  <li
                    key={project.name}
                    className="flex flex-wrap items-center justify-between gap-2 py-1.5"
                  >
                    <div className="min-w-0 text-sm">
                      <b>{project.name}</b>
                      <div className="text-xs text-muted-foreground">
                        <Mono>{project.path}</Mono>
                        {project.files.length > 0 && <> · {project.files.join(", ")}</>}
                      </div>
                    </div>
                    <Button
                      size="xs"
                      variant="outline"
                      aria-label={`Remove ${project.name} from the sync set`}
                      onClick={() =>
                        write.begin({
                          command: "sync remove-project",
                          title: `Remove ${project.name} from the sync set`,
                          argv: ["sync", "remove-project", project.name],
                          confirmLabel: "Remove",
                          planned: planOfRemoveProject(project.name),
                        })
                      }
                    >
                      Remove…
                    </Button>
                  </li>
                ))}
              </ul>
            )}
            <div className="grid gap-3 sm:grid-cols-3">
              <Field
                label="Project folder"
                value={path}
                onChange={setPath}
                placeholder="/path/to/project"
              />
              <Field
                label="Name in the sync set"
                value={name}
                onChange={setName}
                placeholder="my-project"
              />
              <Field
                label="Files"
                value={files}
                onChange={setFiles}
                placeholder="CLAUDE.md, .env.example"
                hint="Comma separated; empty for the default."
              />
            </div>
            <div>
              <Button
                size="sm"
                variant="outline"
                disabled={path.trim() === "" || name.trim() === ""}
                onClick={() => {
                  const list = files
                    .split(",")
                    .map((f) => f.trim())
                    .filter(Boolean);
                  write.begin({
                    command: "sync add-project",
                    title: `Add ${name.trim()} to the sync set`,
                    argv: [
                      "sync",
                      "add-project",
                      path.trim(),
                      "--name",
                      name.trim(),
                      ...optionFlag("--files", list.join(",")),
                    ],
                    confirmLabel: "Add project",
                    planned: planOfAddProject({
                      path: path.trim(),
                      name: name.trim(),
                      files: list,
                    }),
                  });
                }}
              >
                Add project…
              </Button>
            </div>
          </Card>
        </>
      )}

      <Card title="Git sync of the data folder">
        <Intro>
          A separate, plain git sync of the Toolport data folder, for skills and files
          that do not need encryption.
        </Intro>
        <AsyncView query={git} errorTitle="Couldn't read the git sync status">
          {(g) =>
            g.configured ? (
              <div className="flex flex-col gap-3">
                <Kv
                  rows={[
                    ["Repository", <Mono key="r">{g.repo ?? "none"}</Mono>],
                    ["Branch", g.branch ?? "default"],
                    ["Local copy", <Mono key="p">{g.localPath ?? "none"}</Mono>],
                    ["Automatic", g.autoSync ? "yes" : "no"],
                  ]}
                />
                <div>
                  <Button
                    size="sm"
                    variant="destructive"
                    onClick={() =>
                      write.begin({
                        command: "sync git-sync",
                        title: "Remove the git sync setup",
                        argv: ["sync", "git-sync", "--clear"],
                        confirmLabel: "Remove setup",
                        planned: planOfGitSync({
                          repo: "",
                          branch: "",
                          auto: false,
                          clear: true,
                        }),
                      })
                    }
                  >
                    <GitBranch /> Remove setup…
                  </Button>
                </div>
              </div>
            ) : (
              <div className="flex flex-col gap-3">
                <p className="text-sm text-muted-foreground">Git sync is not set up.</p>
                <div className="grid gap-3 sm:grid-cols-2">
                  <Field
                    label="Git repository"
                    value={gitRepo}
                    onChange={setGitRepo}
                    placeholder="git@host:you/toolport-data.git"
                  />
                  <Field
                    label="Git branch"
                    value={gitBranch}
                    onChange={setGitBranch}
                    placeholder="main"
                  />
                </div>
                <Toggle
                  label="Sync automatically from now on"
                  checked={gitAuto}
                  onChange={setGitAuto}
                />
                <div>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={gitRepo.trim() === ""}
                    onClick={() =>
                      write.begin({
                        command: "sync git-sync",
                        title: "Set up git sync",
                        argv: [
                          "sync",
                          "git-sync",
                          "--repo",
                          gitRepo.trim(),
                          ...optionFlag("--branch", gitBranch),
                          ...(gitAuto ? ["--auto"] : []),
                        ],
                        confirmLabel: "Set up git sync",
                        planned: planOfGitSync({
                          repo: gitRepo.trim(),
                          branch: gitBranch.trim(),
                          auto: gitAuto,
                          clear: false,
                        }),
                      })
                    }
                  >
                    Set up git sync…
                  </Button>
                </div>
              </div>
            )
          }
        </AsyncView>
      </Card>

      <Card title="Import an mcpm sync bundle">
        <Intro>
          Bring the encrypted bundle of the old mcpm over. It needs the bundle folder and
          its passphrase.
        </Intro>
        <div>
          <Button size="sm" variant="outline" onClick={() => setDialog("migrate")}>
            <RotateCcw /> Import bundle…
          </Button>
        </div>
      </Card>

      {registry.status === "error" && (
        <ErrorState
          error={registry.error}
          title="Couldn't load the command list"
          onRetry={registry.reload}
        />
      )}
      <WriteDialogs write={write} />
      {dialog === "init" && (
        <InitDialog
          rows={rows}
          configured={configured}
          onClose={closeDialog}
          onDone={refresh}
        />
      )}
      {dialog === "rotate" && (
        <RotateDialog rows={rows} onClose={closeDialog} onDone={refresh} />
      )}
      {dialog === "migrate" && (
        <MigrateDialog rows={rows} onClose={closeDialog} onDone={refresh} />
      )}
    </div>
  );
}
