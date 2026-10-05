import { useState } from "react";
import { FolderPlus, Trash2 } from "lucide-react";
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
import type { SourcesRootLsData } from "../../bridge/data";
import { AsyncView, type CtlQuery } from "../../ui";
import { PathField } from "../fields";
import type { WriteControl } from "../hooks";
import { plural } from "../model";
import { PathLine, Section } from "../parts";
import { baseName, plain } from "./model";

const DEFAULT_WHY =
  "A default folder comes from clients_root in context.json and cannot be removed here.";

export function AddRootDialog({
  onSubmit,
  onClose,
}: {
  onSubmit: (path: string) => void;
  onClose: () => void;
}) {
  const [path, setPath] = useState("");
  const [touched, setTouched] = useState(false);
  const value = path.trim();
  const problem = value === "" ? "Choose or type a folder" : null;
  const submit = () => {
    setTouched(true);
    if (!problem) onSubmit(value);
  };
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Add a folder to scan</DialogTitle>
          <DialogDescription>
            Toolport reads the git checkouts inside this folder for repository skills and
            CLAUDE.md files. Reading never changes them. You see what will be written to
            context.json before anything is.
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
        >
          <PathField
            label="Folder"
            kind="folder"
            value={path}
            onChange={setPath}
            placeholder="~/work"
          />
          <p role="status" className="min-h-4 text-xs text-destructive">
            {touched && problem ? problem : ""}
          </p>
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

/** The folders the repo, client and vendored detectors walk. Defaults come from the context
 * config and cannot be removed here; a folder added here can, after a typed confirmation. */
export function RootsCard({
  query,
  write,
  onAdd,
}: {
  query: CtlQuery<SourcesRootLsData>;
  write: WriteControl;
  onAdd: () => void;
}) {
  return (
    <Section title="Folders Toolport scans" count={query.data?.roots.length}>
      <AsyncView
        query={query}
        errorTitle="Couldn't list the scanned folders"
        isEmpty={(data) => data.roots.length === 0}
        empty={
          <EmptyState
            icon={<FolderPlus />}
            title="No folders to scan yet"
            description="Add the folder that holds your repositories, and Toolport lists the skills and CLAUDE.md files they ship."
            action={
              <Button onClick={onAdd}>
                <FolderPlus /> Add a folder to scan
              </Button>
            }
          />
        }
      >
        {(data) => (
          <>
            <ul aria-label="Scanned folders" className="flex flex-col gap-2">
              {data.roots.map((root) => (
                <li
                  key={root.path}
                  className="flex flex-wrap items-center justify-between gap-3 rounded-lg border bg-card p-3 text-sm"
                >
                  <span className="flex min-w-0 flex-col gap-1">
                    <b className="flex flex-wrap items-center gap-2">
                      {baseName(root.path)}
                      <Badge variant={root.origin === "default" ? "secondary" : "info"}>
                        {root.origin === "default" ? "default" : "added by you"}
                      </Badge>
                      {root.repo && <Badge variant="outline">git checkout</Badge>}
                      {!root.exists && <Badge variant="destructive">not found</Badge>}
                    </b>
                    <PathLine path={plain(root.path)} />
                  </span>
                  {root.origin === "config" ? (
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Stop scanning ${baseName(root.path)}`}
                      disabled={write.busy}
                      onClick={() =>
                        write.begin({
                          command: "sources root rm",
                          title: `Stop scanning ${baseName(root.path)}`,
                          argv: ["sources", "root", "rm", root.path],
                          confirmLabel: "Stop scanning",
                          phrase: baseName(root.path),
                          typed: true,
                        })
                      }
                    >
                      <Trash2 /> Remove…
                    </Button>
                  ) : (
                    <span className="max-w-xs text-xs text-muted-foreground">
                      {DEFAULT_WHY}
                    </span>
                  )}
                </li>
              ))}
            </ul>
            <p className="text-xs text-muted-foreground">
              {plural(data.roots.length, "folder")} walked to a depth of 4, read-only.
              Removing one stops the scan; nothing in it is touched.
            </p>
          </>
        )}
      </AsyncView>
    </Section>
  );
}
