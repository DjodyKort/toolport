import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Callout } from "@/components/Callout";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Field, Mono, Toggle } from "./atoms";
import {
  kindLabel,
  optionFlag,
  ownerRepoProblem,
  postUpdateOf,
  type UpdateServer,
} from "./model";

export interface UpdateChoice {
  title: string;
  argv: string[];
}

const RELEASE = new Set(["github-release", "release"]);

/** What a server update does before it runs: which servers, the `post_update` command of each
 * and the flags that let it run. Nothing here changes anything; Preview starts the dry run. */
export function UpdateOptions({
  mode,
  servers,
  single,
  onPreview,
  onClose,
}: {
  mode: "apply" | "init";
  /** The servers the run reaches. */
  servers: UpdateServer[];
  /** The server id when the run is for one server. */
  single?: string;
  onPreview: (choice: UpdateChoice) => void;
  onClose: () => void;
}) {
  const [commands, setCommands] = useState(false);
  const [unverified, setUnverified] = useState(false);
  const [force, setForce] = useState(false);
  const [repo, setRepo] = useState("");
  const hooks = servers.flatMap((server) => {
    const hook = postUpdateOf(server);
    return hook ? [{ id: server.id, command: hook.command }] : [];
  });
  const release = servers.some((server) => RELEASE.has(server.kind));
  const repoError = ownerRepoProblem(repo);
  const init = mode === "init";
  const title = init
    ? "Detect update sources"
    : single
      ? `Update ${single}`
      : "Update every server with an update";

  function preview() {
    onPreview({
      title,
      argv: [
        "update",
        ...(single ? [single] : []),
        init ? "--init" : "--apply",
        ...(init && force ? ["--force"] : []),
        ...(!init && commands ? ["--allow-commands"] : []),
        ...(!init && unverified ? ["--allow-unverified"] : []),
        ...optionFlag("--repo", repo),
      ],
    });
  }

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>
            {init
              ? "Looks at each server and stores where its updates come from."
              : "Nothing runs yet. The next step is a preview of exactly what changes."}
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          {!init && (
            <ul aria-label="Servers to update" className="flex flex-col gap-1 text-sm">
              {servers.map((server) => (
                <li key={server.id} className="flex flex-wrap gap-2">
                  <b>{server.id}</b>
                  <span className="text-muted-foreground">
                    {kindLabel(server.kind)}
                    {server.behind ? `, ${server.behind} behind` : ""}
                    {server.latest ? `, latest ${server.latest}` : ""}
                  </span>
                </li>
              ))}
            </ul>
          )}
          {!init && hooks.length > 0 && (
            <div className="flex flex-col gap-2 rounded-lg border p-3">
              <p className="text-sm font-medium">
                Update command (post_update) of{" "}
                {hooks.length === 1 ? "this server" : "these servers"}
              </p>
              <ul aria-label="Update commands" className="flex flex-col gap-1">
                {hooks.map((hook) => (
                  <li
                    key={hook.id}
                    className="flex flex-wrap items-baseline gap-2 text-sm"
                  >
                    <b>{hook.id}</b>
                    <Mono>{hook.command}</Mono>
                  </li>
                ))}
              </ul>
              <Toggle
                label="Run the update command after the update"
                checked={commands}
                onChange={setCommands}
                hint="It runs on this machine with your permissions. Without this it is only shown."
              />
            </div>
          )}
          {!init && release && (
            <Toggle
              label="Accept updates that could not be verified"
              checked={unverified}
              onChange={setUnverified}
              hint="A release without a checksum is installed anyway."
            />
          )}
          {init && (
            <Toggle
              label="Detect again even when a source is stored"
              checked={force}
              onChange={setForce}
            />
          )}
          <Field
            label="GitHub repository (optional)"
            value={repo}
            onChange={setRepo}
            placeholder="owner/repo"
            hint="Only for a server whose releases live in a repository Toolport cannot find."
            error={repoError}
          />
          {!init && commands && (
            <Callout variant="warning">
              The preview shows the command with the other steps, and you confirm once
              more before anything runs.
            </Callout>
          )}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={repoError !== null} onClick={preview}>
            Preview {init ? "detection" : "update"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
