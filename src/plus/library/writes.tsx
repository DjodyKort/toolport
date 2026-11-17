import { useId, useState } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  addRemote,
  addSkill,
  bundleExport,
  bundleImport,
  cleanSynced,
  commitLibrary,
  connectLibrary,
  fixChmod,
  initLibrary,
  moveLink,
  pullLibrary,
  pushLibrary,
  resolveCollisions,
  undoSync,
  uninstallSkill,
} from "./commands";
import { AskDialog, FileLines, Mono, PlanSteps, WriteDialog } from "./dialogs";
import { bytes, plural } from "./model";

export interface Done {
  onClose: () => void;
  onDone: () => void;
}

export function UndoDialog({ onClose, onDone }: Done) {
  return (
    <WriteDialog
      title="Undo the last sync"
      description="Restores the files of the snapshot taken before that Apply. Files changed since are skipped, not overwritten."
      confirmLabel="Undo sync"
      preview={() => undoSync(true)}
      render={(data) => (
        <div className="flex flex-col gap-3 text-meta">
          <p className="font-medium">
            {plural(data.removed, "file")} removed, {plural(data.restored, "file")}{" "}
            restored
            {data.conflicts > 0 && `, ${plural(data.conflicts, "conflict")} skipped`}.
          </p>
          <FileLines
            title="Files"
            files={data.files.map(
              (f) =>
                `${f.action === "remove" ? "−" : f.action === "restore" ? "↩" : "!"} ${f.path}`,
            )}
          />
          <p className="text-muted-foreground">
            Undo restores files, not the library. Auto-sync is paused afterwards so it
            does not redo what you undid.
          </p>
        </div>
      )}
      confirm={() => undoSync(false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

export function PullDialog({ onClose, onDone }: Done) {
  return (
    <WriteDialog
      title="Pull and review the plan"
      description="Fast-forward only. Pull never merges and never commits."
      confirmLabel="Pull and review plan"
      preview={() => pullLibrary(true)}
      render={(data) =>
        data.plan ? (
          <PlanSteps plan={data.plan} />
        ) : (
          <p className="text-meta">Nothing to pull.</p>
        )
      }
      confirm={() => pullLibrary(false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

const DEFAULT_MESSAGE = "Update skills library";

/** Push, and with uncommitted work first the commit it makes: the message is yours to write. */
export function PushDialog({
  onClose,
  onDone,
  uncommitted = 0,
}: Done & { uncommitted?: number }) {
  const [message, setMessage] = useState(DEFAULT_MESSAGE);
  const id = useId();
  return (
    <WriteDialog
      title={uncommitted > 0 ? "Commit and push to origin" : "Push to origin"}
      description="Shows the commit it will make, the commits it sends and the checks before anything leaves this machine."
      confirmLabel={uncommitted > 0 ? "Commit and push" : "Push"}
      preview={() => pushLibrary(true)}
      render={(data, reload) => (
        <div className="flex flex-col gap-3 text-meta">
          {uncommitted > 0 && (
            <div className="flex flex-col gap-1">
              <label htmlFor={id} className="font-medium">
                Commit message for the {plural(uncommitted, "changed file")}
              </label>
              <Input
                id={id}
                value={message}
                onChange={(event) => setMessage(event.target.value)}
              />
            </div>
          )}
          <div className="flex flex-wrap gap-1.5">
            <Badge variant="outline">
              {plural(data.commits?.length ?? 0, "commit")} to send
            </Badge>
            <Badge variant="outline">Never forced</Badge>
          </div>
          <ul className="flex flex-col gap-1" aria-label="Commits">
            {(data.commits ?? []).map((commit) => (
              <li key={commit.sha} className="flex gap-2">
                <Mono>{commit.sha === "new" ? "new" : commit.sha}</Mono>
                <span>{commit.subject}</span>
              </li>
            ))}
          </ul>
          <div className="flex flex-col gap-1 rounded-md border p-2" aria-label="Checks">
            <p>
              <span className="font-medium">Secret scan: </span>
              {data.checks.builtinScan.count === 0
                ? "no findings (built-in rules)"
                : plural(data.checks.builtinScan.count, "possible secret")}
            </p>
            {data.checks.builtinScan.findings.map((f) => (
              <p key={`${f.commit}-${f.file}-${f.line}`}>
                <span className="font-medium">{f.rule}</span>{" "}
                <Mono>
                  {f.file}:{f.line}
                </Mono>{" "}
                <span className="text-muted-foreground">
                  (commit {f.commit}; the value is never shown)
                </span>
              </p>
            ))}
            <p>
              <span className="font-medium">Audit: </span>
              {data.checks.audit.findings.length === 0
                ? "no risky instructions"
                : plural(data.checks.audit.findings.length, "finding")}
            </p>
            {!data.checks.gitleaks.available && (
              <p className="text-muted-foreground">
                gitleaks is not installed, so only the built-in rules ran.
              </p>
            )}
          </div>
          {data.plan?.undo && (
            <p className="text-muted-foreground">
              To undo later: <Mono>{data.plan.undo}</Mono>
            </p>
          )}
          {data.checks.blocked && (
            <Button variant="outline" size="sm" className="self-start" onClick={reload}>
              Check again
            </Button>
          )}
        </div>
      )}
      blocked={(data) =>
        data.checks.blocked
          ? "Refused by the secret scan. Removing the line is not enough for a commit already in history; amend or rebase yourself, then check again."
          : uncommitted > 0 && message.trim() === ""
            ? "Write a commit message."
            : null
      }
      confirm={async () => {
        if (uncommitted > 0) await commitLibrary(message.trim(), false);
        return pushLibrary(false);
      }}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

export function CommitDialog({ onClose, onDone }: Done) {
  const [message, setMessage] = useState(DEFAULT_MESSAGE);
  const id = useId();
  return (
    <WriteDialog
      title="Commit the changes"
      description="Commits everything that changed in the library and sends nothing. The secret scan runs first; a finding stops the commit."
      confirmLabel="Commit"
      preview={() => commitLibrary(message.trim() || DEFAULT_MESSAGE, true)}
      blocked={(data) =>
        data.checks?.blocked
          ? "Refused by the secret scan: fix the line, then check again."
          : message.trim() === ""
            ? "Write a commit message."
            : null
      }
      render={(data, reload) => (
        <div className="flex flex-col gap-3 text-meta">
          <div className="flex flex-col gap-1">
            <label htmlFor={id} className="font-medium">
              Commit message
            </label>
            <Input
              id={id}
              value={message}
              onChange={(event) => setMessage(event.target.value)}
            />
          </div>
          {data.files.length === 0 ? (
            <p>Nothing changed since the last commit.</p>
          ) : (
            <FileLines
              title={plural(data.files.length, "changed file")}
              files={data.files}
            />
          )}
          {data.checks && (
            <div
              className="flex flex-col gap-1 rounded-md border p-2"
              aria-label="Checks"
            >
              <p>
                <span className="font-medium">Secret scan: </span>
                {data.checks.builtinScan.count === 0
                  ? "no findings (built-in rules)"
                  : plural(data.checks.builtinScan.count, "possible secret")}
              </p>
              {data.checks.builtinScan.findings.map((f) => (
                <p key={`${f.file}-${f.line}-${f.rule}`}>
                  <span className="font-medium">{f.rule}</span>{" "}
                  <Mono>
                    {f.file}:{f.line}
                  </Mono>{" "}
                  <span className="text-muted-foreground">
                    (the value is never shown)
                  </span>
                </p>
              ))}
            </div>
          )}
          {data.checks?.blocked && (
            <Button variant="outline" size="sm" className="self-start" onClick={reload}>
              Check again
            </Button>
          )}
        </div>
      )}
      confirm={() => commitLibrary(message.trim(), false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

export function AddRemoteDialog({ onClose, onDone }: Done) {
  return (
    <AskDialog
      title="Add a remote"
      description="Gives the library its origin so Push has somewhere to go. Nothing is fetched or pushed now, and Toolport never stores a credential: git and gh sign in."
      label="Repository url"
      placeholder="git@github.com:you/skills-library.git"
      onClose={onClose}
      next={(url, back) => (
        <WriteDialog
          title="Add a remote"
          confirmLabel="Add remote"
          preview={() => addRemote(url, true)}
          render={(data) => (
            <div className="flex flex-col gap-2">
              {data.plan && <PlanSteps plan={data.plan} />}
              <Button variant="ghost" size="sm" className="self-start" onClick={back}>
                Change the url
              </Button>
            </div>
          )}
          confirm={() => addRemote(url, false)}
          onClose={onClose}
          onDone={onDone}
        />
      )}
    />
  );
}

export function ConnectDialog({ onClose, onDone }: Done) {
  const [folder, setFolder] = useState("");
  const id = useId();
  return (
    <AskDialog
      title="Connect a repository"
      description="Clones a repository you already have as your skills library. Toolport never stores a credential: git and gh sign in. Skills Claude Code already has stay where they are."
      label="Repository url"
      placeholder="https://github.com/you/skills-library.git"
      onClose={onClose}
      next={(url, back) => (
        <WriteDialog
          title="Connect a repository"
          confirmLabel="Clone and connect"
          preview={() => connectLibrary(url, folder.trim(), true)}
          render={(data) => (
            <div className="flex flex-col gap-3">
              {data.plan && <PlanSteps plan={data.plan} />}
              <div className="flex flex-col gap-1 text-meta">
                <label htmlFor={id} className="font-medium">
                  Clone into (optional)
                </label>
                <Input
                  id={id}
                  value={folder}
                  placeholder="~/work/skills-library"
                  onChange={(event) => setFolder(event.target.value)}
                />
                <p className="text-muted-foreground">
                  Default: ~/.config/mcpm/skills_repo. The folder must not hold files yet.
                </p>
              </div>
              <Button variant="ghost" size="sm" className="self-start" onClick={back}>
                Change the url
              </Button>
            </div>
          )}
          confirm={() => connectLibrary(url, folder.trim(), false)}
          onClose={onClose}
          onDone={onDone}
        />
      )}
    />
  );
}

export function CleanDialog({ onClose, onDone }: Done) {
  return (
    <WriteDialog
      title="Clean synced files"
      description="Removes the files this sync installed in ~/.claude and the lock that tracks them. Your library and files you added yourself stay."
      confirmLabel="Clean synced files"
      destructive
      preview={() => cleanSynced(true)}
      render={(data) => (
        <div className="flex flex-col gap-2">
          <p className="text-meta font-medium">
            {plural(data.removed.length, "file")} of{" "}
            {plural(data.managed.length, "skill")}.
          </p>
          <FileLines files={data.removed} />
        </div>
      )}
      confirm={() => cleanSynced(false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

export function ChmodDialog({ skill, onClose, onDone }: Done & { skill: string }) {
  return (
    <WriteDialog
      title="Make the hook script executable"
      description="Claude Code cannot run a hook script that is not executable."
      confirmLabel="Make executable"
      preview={() => fixChmod(skill, true)}
      render={(data) =>
        data.fixed.length === 0 ? (
          <p className="text-meta">Nothing to change.</p>
        ) : (
          <ul className="flex flex-col gap-1 text-meta" aria-label="Scripts">
            {data.fixed.map((f) => (
              <li key={`${f.skill}-${f.file}`}>
                <Mono>
                  {f.skill}/{f.file}
                </Mono>{" "}
                {f.from} → {f.to}
              </li>
            ))}
          </ul>
        )
      }
      confirm={() => fixChmod(skill, false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

export function MoveLinkDialog({
  skill,
  link,
  onClose,
  onDone,
}: Done & { skill: string; link: string }) {
  return (
    <WriteDialog
      title="Move the file into the skill"
      description="A sync only installs what is inside the skill folder, so the link would be dead in Claude Code. The file is copied in and the link rewritten; the original stays where it is."
      confirmLabel="Move into the skill"
      preview={() => moveLink(skill, link, true)}
      render={(data) => (
        <div className="flex flex-col gap-3 text-meta">
          <p>
            {data.reused ? (
              <>
                <Mono>{data.to}</Mono> already holds the same content, so nothing is
                copied.
              </>
            ) : (
              <>
                Copies <Mono>{data.from}</Mono> ({bytes(data.size)}) to{" "}
                <Mono>
                  {skill}/{data.to}
                </Mono>
                .
              </>
            )}
          </p>
          <ul className="flex flex-col gap-2" aria-label="Lines that change">
            {data.rewrites.map((row) => (
              <li key={row.line} className="flex flex-col gap-0.5">
                <span className="text-muted-foreground">SKILL.md line {row.line}</span>
                <code className="font-mono text-xs break-all text-destructive">
                  − {row.before}
                </code>
                <code className="font-mono text-xs break-all text-success">
                  + {row.after}
                </code>
              </li>
            ))}
          </ul>
        </div>
      )}
      confirm={() => moveLink(skill, link, false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

export function ResolveDialog({ migrate, onClose, onDone }: Done & { migrate: boolean }) {
  return (
    <WriteDialog
      title={migrate ? "Replace the command (a backup is kept)" : "Keep both"}
      description={
        migrate
          ? "The hand-written file moves to the backup folder and the library skill takes its place."
          : "Both stay; Claude Code answers to the name with whichever it finds first."
      }
      confirmLabel={migrate ? "Replace the command" : "Keep both"}
      preview={() => resolveCollisions(migrate, true)}
      render={(data) => (
        <p className="text-meta">
          {plural(data.collisions.length, "collision")} found.{" "}
          {data.collisions.map((c) => c.collisionPath).join(", ")}
        </p>
      )}
      confirm={() => resolveCollisions(migrate, false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

export function UninstallDialog({ name, onClose, onDone }: Done & { name: string }) {
  return (
    <WriteDialog
      title={`Delete ${name} from the library`}
      description="Removes the skill folder from your library and its synced copies in ~/.claude. A deletion of work that is not committed cannot be recovered from git."
      confirmLabel="Delete from library"
      destructive
      preview={() => uninstallSkill(name, true)}
      render={(data) => (
        <div className="flex flex-col gap-2">
          <p className="text-meta">
            <Mono>{data.sourcePath}</Mono>
          </p>
          <FileLines title="Synced copies removed" files={data.outputs} />
        </div>
      )}
      confirm={() => uninstallSkill(name, false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

export function NewSkillDialog({ onClose, onDone }: Done) {
  const [type, setType] = useState<"skill" | "rule">("skill");
  return (
    <AskDialog
      title="New skill"
      description="Creates the folder with a SKILL.md in your library. It then shows in the Sync plan like any other change."
      label="Name"
      placeholder="build-triage"
      onClose={onClose}
      next={(name, back) => (
        <WriteDialog
          title={`New ${type}: ${name}`}
          confirmLabel="Create"
          preview={() => addSkill(name, type, true)}
          render={(data) => (
            <div className="flex flex-col gap-2">
              <label className="flex items-center gap-2 text-meta">
                Kind
                <select
                  value={type}
                  onChange={(event) => setType(event.target.value as "skill" | "rule")}
                  className="h-8 rounded-md border bg-background px-2 text-sm"
                >
                  <option value="skill">Skill</option>
                  <option value="rule">Rule</option>
                </select>
              </label>
              <FileLines title="Creates" files={data.files} />
              <Button variant="ghost" size="sm" className="self-start" onClick={back}>
                Change the name
              </Button>
            </div>
          )}
          confirm={() => addSkill(name, type, false)}
          onClose={onClose}
          onDone={onDone}
        />
      )}
    />
  );
}

export function InitDialog({ onClose, onDone }: Done) {
  return (
    <AskDialog
      title="Start a local library"
      description="A library is a folder of skills, rules and agents that Toolport copies into Claude Code for you. Skills Claude Code already has stay where they are."
      label="Folder"
      placeholder="~/work/skills-library"
      onClose={onClose}
      next={(path) => (
        <WriteDialog
          title="Start a local library"
          confirmLabel="Create library"
          preview={() => initLibrary(path, "", true)}
          render={(data) => (
            <div className="flex flex-col gap-2 text-meta">
              <p>
                {data.alreadyExists ? "A library already exists in " : "Creates "}
                <Mono>{data.repo}</Mono>
              </p>
              <FileLines title="Folders and files" files={data.created} />
              <p className="text-muted-foreground">
                A local library has no Pull and Push; sync works.
              </p>
            </div>
          )}
          confirm={() => initLibrary(path, "", false)}
          onClose={onClose}
          onDone={onDone}
        />
      )}
    />
  );
}

export function ExportBundleDialog({ onClose, onDone }: Done) {
  return (
    <WriteDialog
      title="Export bundle"
      description="Packs your skills and rules into one zip you can move or share."
      confirmLabel="Export bundle"
      preview={() => bundleExport(true)}
      render={(data) => (
        <div className="flex flex-col gap-2 text-meta">
          <p>
            {plural(data.fileCount, "file")}, {bytes(data.sourceBytes)}, to{" "}
            <Mono>{data.output}</Mono>
          </p>
        </div>
      )}
      confirm={() => bundleExport(false)}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

export function ImportBundleDialog({ onClose, onDone }: Done) {
  return (
    <AskDialog
      title="Import bundle"
      description="Extracts a bundle zip into your library. Items that exist are listed before anything is overwritten."
      label="Path of the zip"
      placeholder="~/Downloads/skills-library-bundle.zip"
      onClose={onClose}
      next={(zip) => (
        <WriteDialog
          title="Import bundle"
          confirmLabel="Import bundle"
          preview={() => bundleImport(zip, true)}
          render={(data) => (
            <div className="flex flex-col gap-2">
              <p className="text-meta">
                Adds {plural(data.names.length, "item")}: {data.names.join(", ")}
              </p>
              <FileLines title="Files" files={data.files} />
              <FileLines title="Overwritten" files={data.overwritten} />
            </div>
          )}
          confirm={() => bundleImport(zip, false)}
          onClose={onClose}
          onDone={onDone}
        />
      )}
    />
  );
}
