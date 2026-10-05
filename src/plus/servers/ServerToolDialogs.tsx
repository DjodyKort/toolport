import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import { Input } from "@/components/ui/input";
import type { ProfileLsData } from "../bridge/data";
import { Code, Field, Problems, SELECT_CLASS } from "./atoms";
import {
  MODE_NOTE,
  MODES,
  forkSyncDefaults,
  forkSyncProblems,
  profileMatch,
  type ForkSyncOptions,
  type GitState,
} from "./mcpTools";

export function AddProfileTagDialog({
  server,
  profiles,
  memberOf,
  onOpenChange,
  onContinue,
}: {
  server: string;
  profiles: ProfileLsData;
  /** Ids of the profiles the server is already in. */
  memberOf: Set<string>;
  onOpenChange: (open: boolean) => void;
  onContinue: (tag: string, exists: boolean) => void;
}) {
  const [tag, setTag] = useState("");
  const free = profiles.profiles.filter((profile) => !memberOf.has(profile.id));
  const match = tag.trim() ? profileMatch(profiles, tag) : undefined;
  const already = match && memberOf.has(match.id);
  const problems = already ? [`${server} is already in ${match.name}.`] : [];
  const ready = tag.trim() !== "" && !already;
  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add {server} to a profile</DialogTitle>
          <DialogDescription>
            Pick one of your profiles, or type a new name to create it. Next you see what
            would change; nothing is written until you confirm.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (ready) onContinue(tag.trim(), !!match);
          }}
        >
          <Field
            label="Profile"
            hint={
              free.length > 0
                ? `Not yet in: ${free.map((profile) => profile.name).join(", ")}`
                : `${server} is already in every profile`
            }
          >
            {(id, describedBy) => (
              <>
                <Input
                  id={id}
                  aria-describedby={describedBy}
                  list={`${id}-profiles`}
                  value={tag}
                  autoFocus
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(event) => setTag(event.target.value)}
                />
                <datalist id={`${id}-profiles`}>
                  {free.map((profile) => (
                    <option key={profile.id} value={profile.name} />
                  ))}
                </datalist>
              </>
            )}
          </Field>
          {tag.trim() !== "" && !match && (
            <Callout variant="info">
              There is no profile called {tag.trim()}. Confirming creates it.
            </Callout>
          )}
          <Problems items={problems} />
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={!ready}>
              Review
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function ModeDialog({
  server,
  onOpenChange,
  onContinue,
}: {
  server: string;
  onOpenChange: (open: boolean) => void;
  onContinue: (mode: string) => void;
}) {
  const [mode, setMode] = useState<string>(MODES[0]);
  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Mode of {server}</DialogTitle>
          <DialogDescription>
            The modes of the earlier tool, kept so a request for one is answered.
          </DialogDescription>
        </DialogHeader>
        <Callout variant="info">{MODE_NOTE}</Callout>
        <Field label="Mode">
          {(id) => (
            <select
              id={id}
              className={SELECT_CLASS}
              value={mode}
              onChange={(event) => setMode(event.target.value)}
            >
              {MODES.map((candidate) => (
                <option key={candidate} value={candidate}>
                  {candidate}
                </option>
              ))}
            </select>
          )}
        </Field>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={() => onContinue(mode)}>Review</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function ForkSyncDialog({
  server,
  git,
  hasPostUpdate,
  onOpenChange,
  onContinue,
}: {
  server: string;
  git: GitState;
  hasPostUpdate: boolean;
  onOpenChange: (open: boolean) => void;
  onContinue: (options: ForkSyncOptions) => void;
}) {
  const [options, setOptions] = useState<ForkSyncOptions>(forkSyncDefaults);
  const [shown, setShown] = useState(false);
  const problems = forkSyncProblems(options);
  const set = (change: Partial<ForkSyncOptions>) =>
    setOptions((previous) => ({ ...previous, ...change }));
  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Sync {server} with its upstream</DialogTitle>
          <DialogDescription>
            Fetches the upstream and puts your work on a new branch. Next you see the
            plan; nothing is touched until you confirm.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            setShown(true);
            if (problems.length === 0) onContinue(options);
          }}
        >
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="Upstream remote">
              {(id) => (
                <Input
                  id={id}
                  value={options.upstreamRemote}
                  spellCheck={false}
                  onChange={(event) => set({ upstreamRemote: event.target.value })}
                />
              )}
            </Field>
            <Field label="Upstream branch">
              {(id) => (
                <Input
                  id={id}
                  value={options.upstreamBranch}
                  spellCheck={false}
                  onChange={(event) => set({ upstreamBranch: event.target.value })}
                />
              )}
            </Field>
          </div>
          <Field
            label="How"
            hint="Rebase keeps every commit; onto-author keeps one author's."
          >
            {(id, describedBy) => (
              <select
                id={id}
                aria-describedby={describedBy}
                className={SELECT_CLASS}
                value={options.mode}
                onChange={(event) =>
                  set({ mode: event.target.value as ForkSyncOptions["mode"] })
                }
              >
                <option value="rebase">Rebase</option>
                <option value="onto-author">
                  Onto the upstream, one author's commits
                </option>
              </select>
            )}
          </Field>
          {options.mode === "onto-author" && (
            <Field label="Author email">
              {(id) => (
                <Input
                  id={id}
                  type="email"
                  value={options.authorEmail}
                  spellCheck={false}
                  onChange={(event) => set({ authorEmail: event.target.value })}
                />
              )}
            </Field>
          )}
          <Field
            label="New branch"
            hint={`Left empty it is called ${git.branch ?? "the current branch"}-synced-<date>.`}
          >
            {(id, describedBy) => (
              <Input
                id={id}
                aria-describedby={describedBy}
                value={options.targetBranch}
                spellCheck={false}
                onChange={(event) => set({ targetBranch: event.target.value })}
              />
            )}
          </Field>
          {hasPostUpdate && (
            <label className="flex items-start gap-2 text-sm">
              <input
                type="checkbox"
                className="mt-0.5"
                checked={options.runPostUpdate}
                onChange={(event) => set({ runPostUpdate: event.target.checked })}
              />
              <span>
                Run the stored update command (<Code>post_update</Code>) when the sync
                worked
              </span>
            </label>
          )}
          {shown && <Problems items={problems} />}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit">Review</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
