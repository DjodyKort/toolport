import { useState } from "react";
import { Download, Plus, RefreshCw, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import type { SkillsTapLsData } from "../types/skills";
import { AsyncView } from "../ui";
import type { WriteControl } from "./hooks";
import { useRead } from "./hooks";
import { plural } from "./model";
import { PathLine } from "./parts";

const SOURCE = /^(https?:\/\/\S+|git@\S+|[\w.-]+\/[\w.-]+)$/;
const CREDENTIAL = /^https?:\/\/[^/@\s]*@/;

function sourceProblem(source: string): string | null {
  if (CREDENTIAL.test(source))
    return "This URL carries a credential, and a command line is not a safe place for one. Use an SSH URL or let git's credential helper sign in";
  return SOURCE.test(source) ? null : "Use user/repo or the URL of a git repository";
}

function AddTapDialog({
  onSubmit,
  onClose,
}: {
  onSubmit: (source: string, alias: string) => void;
  onClose: () => void;
}) {
  const [source, setSource] = useState("");
  const [alias, setAlias] = useState("");
  const [touched, setTouched] = useState(false);
  const problem = sourceProblem(source.trim());
  const submit = () => {
    setTouched(true);
    if (!problem) onSubmit(source.trim(), alias.trim());
  };
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add a tap</DialogTitle>
          <DialogDescription>
            A tap is a git repository of skills. Adding it clones the repository, so it
            needs the network. You see what it will do before anything is written.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <label className="flex flex-col gap-1.5 text-sm">
            Repository
            <Input
              autoFocus
              value={source}
              onChange={(event) => setSource(event.target.value)}
              placeholder="user/repo or https://…"
              autoComplete="off"
              spellCheck={false}
              aria-invalid={touched && problem ? true : undefined}
            />
          </label>
          <p role="status" className="min-h-4 text-xs text-destructive">
            {touched && problem ? problem : ""}
          </p>
          <label className="flex flex-col gap-1.5 text-sm">
            Name (optional)
            <Input
              value={alias}
              onChange={(event) => setAlias(event.target.value)}
              placeholder="Taken from the repository"
              autoComplete="off"
              spellCheck={false}
            />
          </label>
        </form>
        <DialogFooter>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={submit}>Preview</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** The taps: git repositories Toolport searches for skills. Adding and updating clone and pull,
 * so they say they need the network; removing deletes only the local clone. */
export function TapsPanel({ write }: { write: WriteControl }) {
  const taps = useRead<SkillsTapLsData>(["skills", "tap", "ls"]);
  const [adding, setAdding] = useState(false);
  const add = () => setAdding(true);
  return (
    <div className="flex flex-col gap-4">
      {adding && (
        <AddTapDialog
          onClose={() => setAdding(false)}
          onSubmit={(source, alias) => {
            setAdding(false);
            write.begin({
              command: "skills tap add",
              title: `Add tap ${alias || source}`,
              argv: ["skills", "tap", "add", source, ...(alias ? ["--name", alias] : [])],
              confirmLabel: "Add tap",
              phrase: alias || source,
            });
          }}
        />
      )}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="max-w-prose text-sm text-muted-foreground">
          Taps are git repositories of skills. Search looks in them and install copies
          from them. Adding or updating a tap needs the network.
        </p>
        <div className="flex flex-wrap gap-2">
          <Button disabled={write.busy} onClick={add}>
            <Plus /> Add tap…
          </Button>
          <Button
            variant="outline"
            disabled={write.busy || (taps.data?.taps.length ?? 0) === 0}
            onClick={() =>
              write.begin({
                command: "skills tap update",
                title: "Update all taps",
                argv: ["skills", "tap", "update"],
                confirmLabel: "Update",
                phrase: "update",
              })
            }
          >
            <RefreshCw /> Update all…
          </Button>
        </div>
      </div>
      <AsyncView query={taps} errorTitle="Couldn't list the taps">
        {(data) =>
          data.taps.length === 0 ? (
            <EmptyState
              icon={<Download />}
              title="No taps yet"
              description="Add a git repository of skills to search it and install from it."
              action={
                <Button onClick={add}>
                  <Plus /> Add your first tap
                </Button>
              }
            />
          ) : (
            <ul aria-label="Taps" className="flex flex-col gap-2">
              {data.taps.map((tap) => (
                <li
                  key={tap.name}
                  className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-card p-3 text-sm"
                >
                  <span className="flex min-w-0 flex-col gap-1">
                    <b className="flex items-center gap-2">
                      {tap.name}
                      <Badge variant={tap.cloned ? "success" : "warning"}>
                        {tap.cloned ? "cloned" : "clone missing"}
                      </Badge>
                    </b>
                    <code className="font-mono text-xs break-all text-muted-foreground">
                      {tap.url}
                    </code>
                    <PathLine path={tap.path} />
                  </span>
                  <span className="flex gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Update ${tap.name}`}
                      disabled={write.busy}
                      onClick={() =>
                        write.begin({
                          command: "skills tap update",
                          title: `Update tap ${tap.name}`,
                          argv: ["skills", "tap", "update", tap.name],
                          confirmLabel: "Update",
                          phrase: tap.name,
                        })
                      }
                    >
                      <RefreshCw /> Update…
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Remove ${tap.name}`}
                      disabled={write.busy}
                      onClick={() =>
                        write.begin({
                          command: "skills tap remove",
                          title: `Remove tap ${tap.name}`,
                          argv: ["skills", "tap", "remove", tap.name],
                          confirmLabel: "Remove",
                          phrase: tap.name,
                        })
                      }
                    >
                      <Trash2 /> Remove…
                    </Button>
                  </span>
                </li>
              ))}
            </ul>
          )
        }
      </AsyncView>
      {taps.data && taps.data.taps.length > 0 && (
        <p className="text-xs text-muted-foreground">
          {plural(taps.data.taps.length, "tap")} under {taps.data.tapsRoot}.
        </p>
      )}
    </div>
  );
}
