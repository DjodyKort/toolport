import { useState } from "react";
import type { CommandRow } from "../bridge/data";
import { Field, Toggle } from "./atoms";
import {
  optionFlag,
  planOfMigrate,
  planOfRotate,
  planOfSyncInit,
  repoProblem,
} from "./model";
import { SecretWriteDialog } from "./SecretWriteDialog";

export function InitDialog({
  rows,
  configured,
  onClose,
  onDone,
}: {
  rows: CommandRow[] | null;
  configured: boolean;
  onClose: () => void;
  onDone: () => void;
}) {
  const [repo, setRepo] = useState("");
  const [branch, setBranch] = useState("");
  const [machine, setMachine] = useState("");
  const [reconfigure, setReconfigure] = useState(configured);
  const [touched, setTouched] = useState(false);
  const problem = repoProblem(repo);
  const argv = [
    "sync",
    "init",
    "--repo",
    repo.trim(),
    ...optionFlag("--branch", branch),
    ...optionFlag("--machine-id", machine),
    ...(reconfigure ? ["--reconfigure"] : []),
    "--passphrase-stdin",
  ];
  return (
    <SecretWriteDialog
      title={configured ? "Reconfigure sync" : "Set up sync"}
      description="Toolport encrypts the bundle with this passphrase before it leaves the machine. Other machines need the same one."
      command="sync init"
      argv={argv}
      plan={planOfSyncInit({
        repo: repo.trim() || "the repository",
        branch: branch.trim(),
        machineId: machine.trim(),
        reconfigure,
      })}
      rows={rows}
      secretLabel="Passphrase"
      repeat
      canRun={problem === null}
      confirmLabel={configured ? "Reconfigure" : "Set up sync"}
      doneLabel="Sync is set up"
      renderResult={(data) => {
        const result = data as {
          machineId?: string;
          branch?: string;
          freshRemote?: boolean;
        };
        return (
          <p>
            This machine is {result.machineId} on branch {result.branch}.{" "}
            {result.freshRemote
              ? "The remote was empty; push to create the first bundle."
              : "The remote already has a bundle; pull to apply it."}
          </p>
        );
      }}
      onClose={onClose}
      onDone={onDone}
      form={
        <div className="grid gap-3 sm:grid-cols-2">
          <div className="sm:col-span-2" onBlur={() => setTouched(true)}>
            <Field
              label="Git repository"
              value={repo}
              onChange={setRepo}
              placeholder="git@host:you/toolport-sync.git"
              hint="Holds only the encrypted bundle."
              error={touched ? problem : null}
            />
          </div>
          <Field label="Branch" value={branch} onChange={setBranch} placeholder="main" />
          <Field
            label="This machine's name"
            value={machine}
            onChange={setMachine}
            placeholder="work-laptop"
          />
          <div className="sm:col-span-2">
            <Toggle
              label="Replace the existing sync setup"
              checked={reconfigure}
              onChange={setReconfigure}
              hint="Needed when this machine is already set up."
            />
          </div>
        </div>
      }
    />
  );
}

export function RotateDialog({
  rows,
  onClose,
  onDone,
}: {
  rows: CommandRow[] | null;
  onClose: () => void;
  onDone: () => void;
}) {
  return (
    <SecretWriteDialog
      title="Rotate the passphrase"
      description="Every blob in the remote bundle is encrypted again under the new passphrase."
      command="sync rotate-passphrase"
      argv={["sync", "rotate-passphrase", "--passphrase-stdin"]}
      plan={planOfRotate()}
      rows={rows}
      secretLabel="New passphrase"
      repeat
      confirmLabel="Rotate"
      doneLabel="Passphrase rotated"
      renderResult={(data) => {
        const result = data as { rotated?: number; skipped?: unknown[] };
        return (
          <p>
            {result.rotated ?? 0} blob(s) re-encrypted. Other machines must run init with
            the new passphrase.
          </p>
        );
      }}
      onClose={onClose}
      onDone={onDone}
    />
  );
}

export function MigrateDialog({
  rows,
  onClose,
  onDone,
}: {
  rows: CommandRow[] | null;
  onClose: () => void;
  onDone: () => void;
}) {
  const [dir, setDir] = useState("");
  const [projects, setProjects] = useState(false);
  return (
    <SecretWriteDialog
      title="Import an mcpm sync bundle"
      description="Reads the bundle with its passphrase and keeps it as a Toolport bundle."
      command="sync migrate"
      argv={[
        "sync",
        "migrate",
        dir.trim(),
        ...(projects ? ["--include-projects"] : []),
        "--passphrase-stdin",
      ]}
      plan={planOfMigrate({ bundleDir: dir.trim() || "the folder", projects })}
      rows={rows}
      secretLabel="Bundle passphrase"
      canRun={dir.trim() !== ""}
      confirmLabel="Import bundle"
      doneLabel="Bundle imported"
      onClose={onClose}
      onDone={onDone}
      form={
        <div className="flex flex-col gap-3">
          <Field
            label="Bundle folder"
            value={dir}
            onChange={setDir}
            placeholder="/path/to/mcpm-sync-bundle"
          />
          <Toggle
            label="Include the registered project files"
            checked={projects}
            onChange={setProjects}
          />
        </div>
      }
    />
  );
}
