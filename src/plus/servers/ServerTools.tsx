import { useEffect, useState, type ReactNode } from "react";
import { GitBranch, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { kindLabel, plural, postUpdateOf, statusOf } from "../system/model";
import { ScreenSkeleton, outcomeOf, useCtlJob } from "../ui";
import { Code, Kv, Pill } from "./atoms";
import {
  NO_MCP_CALL,
  addTagPlan,
  applyUpdatePlan,
  callResult,
  forkSyncArgs,
  forkSyncPlan,
  forkSyncResultPlan,
  gitStateOf,
  hasMcpCall,
  modeResultPlan,
  profileMatch,
  setModePlan,
  setSourceArgs,
  setSourcePlan,
  setSourceResultPlan,
  sourceOf,
  syncBlocker,
  tagResultPlan,
  toolArgs,
  toolArgv,
  updateOf,
  type ForkSyncOptions,
  type GitState,
  type SetSourceOptions,
  type SourceInfo,
} from "./mcpTools";
import type { ServerView } from "./model";
import {
  AddProfileTagDialog,
  ForkSyncDialog,
  ModeDialog,
  SetSourceDialog,
} from "./ServerToolDialogs";
import { useServers } from "./useServers";
import type { ProfileLsData } from "../bridge/data";
import type { WriteControl, WriteSpec } from "./useWrite";

const SHOWN_COMMITS = 5;

const toolSpec = (
  tool: string,
  rest: Omit<WriteSpec, "argv" | "tool" | "command">,
): WriteSpec => ({ ...rest, tool, argv: toolArgv(tool) });

/** One read of a tool through `mcp call`: its arguments go through stdin, and what comes back
 * is the tool's result, an error with its own words, or nothing yet. */
function useToolRead(tool: string, name: string) {
  const job = useCtlJob();
  const outcome = outcomeOf(job.state);
  const parsed = outcome?.kind === "ok" ? callResult(outcome.data) : null;
  const error =
    outcome?.kind === "error"
      ? outcome.message
      : parsed && !parsed.ok
        ? parsed.message
        : null;
  return {
    running: job.state.phase === "running",
    result: parsed?.ok ? parsed.result : null,
    error,
    cancelled: outcome?.kind === "cancelled",
    run: (extra: Record<string, unknown> = {}) =>
      void job.start(toolArgv(tool), { stdinSecret: toolArgs({ name, ...extra }) }),
    cancel: () => void job.cancel(),
    reset: job.reset,
  };
}

type Read = ReturnType<typeof useToolRead>;

function ReadProblem({ read, what }: { read: Read; what: string }) {
  if (read.running) return <ScreenSkeleton rows={1} label={what} />;
  if (read.error)
    return (
      <Callout variant="danger" role="alert">
        <span className="break-words">{read.error}</span>
      </Callout>
    );
  if (read.cancelled)
    return (
      <p className="text-sm text-muted-foreground">
        Cancelled. Nothing more was started.
      </p>
    );
  return null;
}

function Row({ children }: { children: ReactNode }) {
  return <div className="flex flex-col gap-2">{children}</div>;
}

function SourceRow({
  read,
  source,
  canSwitch,
  onSwitch,
}: {
  read: Read;
  source: SourceInfo | null;
  canSwitch: boolean;
  onSwitch: () => void;
}) {
  return (
    <Row>
      <div className="flex flex-wrap items-center gap-2">
        {source ? (
          <>
            <Pill>{kindLabel(source.kind)}</Pill>
            <span className="text-xs text-muted-foreground">
              {source.stored ? "stored in the registry" : "detected, not stored yet"}
            </span>
          </>
        ) : null}
        <Button
          size="sm"
          variant="outline"
          disabled={read.running}
          onClick={() => read.run()}
        >
          {source ? <RefreshCw /> : null}
          {source ? "Detect again" : "Where did this come from?"}
        </Button>
        {source?.kind === "git" && (
          <Button
            size="sm"
            disabled={!canSwitch}
            title={canSwitch ? undefined : NO_MCP_CALL}
            aria-label="Switch branch"
            onClick={onSwitch}
          >
            Switch branch…
          </Button>
        )}
      </div>
      {source && (source.path || source.reason) && (
        <p className="text-xs text-muted-foreground">
          {source.path && <Code>{source.path}</Code>}
          {source.remote && source.branch && (
            <>
              {" "}
              on {source.remote}/{source.branch}
            </>
          )}
          {!source.remote && source.branch && <> on branch {source.branch}</>}
          {source.reason && <> {source.reason}</>}
        </p>
      )}
      <ReadProblem read={read} what="Looking for the source" />
    </Row>
  );
}

function UpdatesRow({
  name,
  read,
  disabled,
  onUpdate,
}: {
  name: string;
  read: Read;
  disabled: boolean;
  onUpdate: (server: NonNullable<ReturnType<typeof updateOf>>) => void;
}) {
  const server = read.result ? updateOf(read.result, undefined) : null;
  const status = server ? statusOf(server.status) : null;
  const hook = server ? postUpdateOf(server) : null;
  return (
    <Row>
      <div className="flex flex-wrap items-center gap-2">
        {status && <Pill>{status.label}</Pill>}
        <Button
          size="sm"
          variant="outline"
          disabled={read.running || disabled}
          title={disabled ? NO_MCP_CALL : undefined}
          onClick={() => read.run()}
        >
          <RefreshCw /> {server ? "Check again" : "Check for updates"}
        </Button>
        {server?.status === "update-available" && (
          <Button
            size="sm"
            disabled={disabled}
            aria-label={`Update ${name}`}
            onClick={() => onUpdate(server)}
          >
            Update…
          </Button>
        )}
      </div>
      {server && (
        <p className="text-xs break-words text-muted-foreground">
          {kindLabel(server.kind)}: {server.message}
          {server.behind ? ` (${plural(server.behind, "commit")} behind)` : ""}
        </p>
      )}
      {hook && (
        <p className="text-xs text-muted-foreground">
          Update command:{" "}
          <Code>
            {hook.command.replace(/\s*\(not run without --allow-commands\)$/, "")}
          </Code>{" "}
          runs when you apply the update.
        </p>
      )}
      <ReadProblem read={read} what="Checking for updates" />
      <p className="text-xs text-muted-foreground">
        Every server's updates are also listed in System, under Updates.
      </p>
    </Row>
  );
}

function GitRow({
  read,
  git,
  hasPostUpdate,
  onSync,
}: {
  read: Read;
  git: GitState | null;
  hasPostUpdate: boolean;
  onSync: () => void;
}) {
  const blocker = git ? syncBlocker(git) : null;
  return (
    <Row>
      {git && git.isGit && git.pathExists && !git.error && (
        <div className="flex flex-col gap-1 text-sm">
          <div className="flex flex-wrap items-center gap-2">
            <GitBranch className="size-4 text-muted-foreground" aria-hidden="true" />
            <Code>{git.branch ?? "no branch"}</Code>
            {git.remoteRef && (
              <span className="text-xs text-muted-foreground">
                tracks {git.remoteRef}
              </span>
            )}
            <Pill>{git.dirty ? "uncommitted changes" : "clean"}</Pill>
            <Pill>{git.ahead ?? 0} ahead</Pill>
            <Pill>{git.behind ?? 0} behind</Pill>
          </div>
          {git.summaries.length > 0 && (
            <ul
              aria-label="Upstream commits"
              className="list-disc pl-5 text-xs text-muted-foreground"
            >
              {git.summaries.slice(0, SHOWN_COMMITS).map((line) => (
                <li key={line} className="break-words">
                  {line}
                </li>
              ))}
              {git.summaries.length > SHOWN_COMMITS && (
                <li>and {git.summaries.length - SHOWN_COMMITS} more</li>
              )}
            </ul>
          )}
        </div>
      )}
      {git && blocker && (
        <p className="text-xs text-muted-foreground" role="status">
          {blocker}
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={read.running}
          onClick={() => read.run()}
        >
          <RefreshCw /> Refresh git state
        </Button>
        <Button
          size="sm"
          disabled={!git || blocker !== null}
          title={blocker ?? undefined}
          aria-label="Sync with upstream"
          onClick={onSync}
        >
          Sync with upstream…
        </Button>
      </div>
      {hasPostUpdate && (
        <p className="text-xs text-muted-foreground">
          This server has a stored update command; the sync can run it afterwards.
        </p>
      )}
      <ReadProblem read={read} what="Reading the git state" />
    </Row>
  );
}

/** The source of a server, its updates, the git state of a checkout and its mode. Everything
 * here is read on request (a click), and every write is a plan to confirm first. */
export function ServerTools({
  view,
  profiles,
  write,
  onChanged,
}: {
  view: ServerView;
  profiles: ProfileLsData;
  write: WriteControl;
  onChanged: () => void;
}) {
  const { registry } = useServers();
  const available = hasMcpCall(registry.data ?? null);
  const detect = useToolRead("servers_detect_source", view.name);
  const gitRead = useToolRead("servers_git_status", view.name);
  const check = useToolRead("servers_check_updates", view.name);
  const source = detect.result ? sourceOf(detect.result) : null;
  const git = gitRead.result ? gitStateOf(gitRead.result) : null;
  const update = check.result ? updateOf(check.result) : null;
  const hasPostUpdate = update ? postUpdateOf(update) !== null : false;
  const isGit = source?.kind === "git";
  const [dialog, setDialog] = useState<"tag" | "mode" | "sync" | "source" | null>(null);

  const startGit = gitRead.run;
  useEffect(() => {
    if (isGit) startGit();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isGit, detect.result]);

  function begin(next: WriteSpec) {
    setDialog(null);
    write.begin(next);
  }

  function addTag(tag: string, exists: boolean) {
    const match = profileMatch(profiles, tag);
    const label = match?.name ?? tag;
    begin(
      toolSpec("servers_add_profile_tag", {
        title: `Add ${view.name} to ${label}`,
        confirmLabel: exists ? "Add to profile" : "Create and add",
        stdin: toolArgs({ name: view.name, profile_tag: match?.id ?? tag }),
        planned: addTagPlan(view.name, label, exists),
        adapt: (data, done) => {
          const parsed = callResult(data);
          return parsed.ok
            ? tagResultPlan(parsed.result, label, done)
            : failedPlan(parsed.message);
        },
        after: onChanged,
      }),
    );
  }

  function setMode(mode: string) {
    begin(
      toolSpec("servers_set_mode", {
        title: `Set the mode of ${view.name}`,
        confirmLabel: "Set mode",
        stdin: toolArgs({ name: view.name, mode, confirm: true }),
        planned: setModePlan(view.name, mode),
        adapt: (data) => {
          const parsed = callResult(data);
          return parsed.ok ? modeResultPlan(parsed.result) : failedPlan(parsed.message);
        },
      }),
    );
  }

  function applyUpdate(server: NonNullable<ReturnType<typeof updateOf>>) {
    begin(
      toolSpec("servers_apply_update", {
        title: `Update ${view.name}`,
        confirmLabel: "Update and run its command",
        stdin: toolArgs({ name: view.name, confirm: true }),
        planned: applyUpdatePlan(view.name, server),
        adapt: (data) => {
          const parsed = callResult(data);
          if (!parsed.ok) return failedPlan(parsed.message);
          const applied = updateOf(parsed.result, view.id);
          return {
            ...applyUpdatePlan(view.name, applied),
            summary: applied
              ? `${view.name}: ${statusOf(applied.status).label.toLowerCase()}`
              : `Updated ${view.name}`,
          };
        },
        after: () => {
          check.reset();
          onChanged();
        },
      }),
    );
  }

  function sync(options: ForkSyncOptions) {
    if (!git) return;
    begin(
      toolSpec("servers_fork_sync", {
        title: `Sync ${view.name} with its upstream`,
        confirmLabel: "Sync",
        stdin: toolArgs(forkSyncArgs(view.name, options)),
        planned: forkSyncPlan(view.name, git, options),
        adapt: (data) => {
          const parsed = callResult(data);
          return parsed.ok
            ? forkSyncResultPlan(parsed.result, view.name)
            : failedPlan(parsed.message);
        },
        after: () => {
          gitRead.run();
          onChanged();
        },
      }),
    );
  }

  function setSource(options: SetSourceOptions) {
    if (!source) return;
    begin(
      toolSpec("servers_set_source", {
        title: `Switch the source of ${view.name}`,
        confirmLabel: "Switch",
        stdin: toolArgs(setSourceArgs(view.name, options)),
        planned: setSourcePlan(view.name, source, options),
        adapt: (data) => {
          const parsed = callResult(data);
          return parsed.ok
            ? setSourceResultPlan(parsed.result, view.name)
            : failedPlan(parsed.message);
        },
        after: () => {
          detect.run();
          onChanged();
        },
      }),
    );
  }

  const memberOf = new Set(view.profiles.map((profile) => profile.id));
  return (
    <section aria-label="Source, updates and mode" className="flex flex-col gap-3">
      <h3 className="text-2xs font-semibold tracking-[0.09em] text-muted-foreground uppercase">
        Source and updates
      </h3>
      {!available && <Callout variant="info">{NO_MCP_CALL}</Callout>}
      <Kv
        rows={[
          [
            "Source",
            <SourceRow
              key="source"
              read={detect}
              source={source}
              canSwitch={!!source && available && !write.busy}
              onSwitch={() => setDialog("source")}
            />,
          ],
          [
            "Updates",
            <UpdatesRow
              key="updates"
              name={view.name}
              read={check}
              disabled={!available || write.busy}
              onUpdate={applyUpdate}
            />,
          ],
          ...(isGit
            ? ([
                [
                  "Git",
                  <GitRow
                    key="git"
                    read={gitRead}
                    git={git}
                    hasPostUpdate={hasPostUpdate}
                    onSync={() => setDialog("sync")}
                  />,
                ],
              ] as Array<[string, ReactNode]>)
            : []),
          [
            "Mode",
            <Row key="mode">
              <div className="flex flex-wrap items-center gap-2">
                <span>Through the shared gateway</span>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!available || write.busy}
                  title={available ? undefined : NO_MCP_CALL}
                  onClick={() => setDialog("mode")}
                >
                  Change mode…
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">
                Toolport+ has no per-server modes; every server is exposed by the one
                gateway.
              </p>
            </Row>,
          ],
          [
            "Profile tags",
            <Row key="tags">
              <div className="flex flex-wrap items-center gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  disabled={!available || write.busy}
                  title={available ? undefined : NO_MCP_CALL}
                  onClick={() => setDialog("tag")}
                >
                  Add to a profile…
                </Button>
              </div>
              <p className="text-xs text-muted-foreground">
                A new name creates the profile. Take a server out of one with its switch
                above.
              </p>
            </Row>,
          ],
        ]}
      />
      {dialog === "tag" && (
        <AddProfileTagDialog
          server={view.name}
          profiles={profiles}
          memberOf={memberOf}
          onOpenChange={(open) => !open && setDialog(null)}
          onContinue={addTag}
        />
      )}
      {dialog === "mode" && (
        <ModeDialog
          server={view.name}
          onOpenChange={(open) => !open && setDialog(null)}
          onContinue={setMode}
        />
      )}
      {dialog === "sync" && git && (
        <ForkSyncDialog
          server={view.name}
          git={git}
          hasPostUpdate={hasPostUpdate}
          onOpenChange={(open) => !open && setDialog(null)}
          onContinue={sync}
        />
      )}
      {dialog === "source" && source && (
        <SetSourceDialog
          server={view.name}
          source={source}
          onOpenChange={(open) => !open && setDialog(null)}
          onContinue={setSource}
        />
      )}
    </section>
  );
}

function failedPlan(message: string) {
  return {
    summary: "The tool did not apply anything",
    steps: [{ op: "note" as const, detail: message }],
    effects: {},
    warnings: [message],
    undo: "",
  };
}
